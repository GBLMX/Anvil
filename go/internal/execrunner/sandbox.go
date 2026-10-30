// Package execrunner applies the kernel sandbox used by sys.exec.
//
// The package is deliberately small: the gateway resolves policy and opens
// workspace/tool descriptors; this runner only applies Landlock, seccomp, and
// no_new_privs before the final exec.
package execrunner

import (
	"debug/elf"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"time"
	"unsafe"

	"golang.org/x/sys/unix"
)

const (
	readAccess  = unix.LANDLOCK_ACCESS_FS_READ_FILE | unix.LANDLOCK_ACCESS_FS_READ_DIR
	execAccess  = unix.LANDLOCK_ACCESS_FS_EXECUTE
	allReadExec = readAccess | execAccess
	writeAccess = unix.LANDLOCK_ACCESS_FS_WRITE_FILE |
		unix.LANDLOCK_ACCESS_FS_REMOVE_DIR | unix.LANDLOCK_ACCESS_FS_REMOVE_FILE |
		unix.LANDLOCK_ACCESS_FS_MAKE_CHAR | unix.LANDLOCK_ACCESS_FS_MAKE_DIR |
		unix.LANDLOCK_ACCESS_FS_MAKE_REG | unix.LANDLOCK_ACCESS_FS_MAKE_SOCK |
		unix.LANDLOCK_ACCESS_FS_MAKE_FIFO | unix.LANDLOCK_ACCESS_FS_MAKE_BLOCK |
		unix.LANDLOCK_ACCESS_FS_MAKE_SYM
)

// SandboxError reports a missing or unusable kernel feature.
type SandboxError struct {
	Feature string
	Err     error
}

func (e *SandboxError) Error() string {
	return fmt.Sprintf("%s sandbox unavailable: %v", e.Feature, e.Err)
}

func (e *SandboxError) Unwrap() error { return e.Err }

// LandlockABI returns the supported Landlock ABI version.
func LandlockABI() (int, error) {
	version, _, errno := unix.Syscall(
		unix.SYS_LANDLOCK_CREATE_RULESET,
		0,
		0,
		unix.LANDLOCK_CREATE_RULESET_VERSION,
	)
	if errno != 0 {
		return 0, &SandboxError{Feature: "landlock", Err: errno}
	}
	if version == 0 {
		return 0, &SandboxError{Feature: "landlock", Err: errors.New("ABI 0")}
	}
	return int(version), nil
}

// Apply confines the current process to read-only workspace and runtime paths.
// It permits no file execution; call ApplyTool when executing an allowlisted
// tool.
//
// This function is irreversible. It must run in the exec-runner child, never in
// the gateway or editor process.
func Apply(workspace string, runtimePaths []string) error {
	return ApplyTool(workspace, "", runtimePaths)
}

// ApplyTool also grants execute access to the tool and its ELF interpreter.
// Runtime directories stay read-only, so another binary cannot be exec'd.
// It locks the calling OS thread; the caller must exec on the same goroutine.
func ApplyTool(workspace, tool string, runtimePaths []string) error {
	runtime.LockOSThread()
	threadID := unix.Gettid()
	abi, err := LandlockABI()
	if err != nil {
		return err
	}
	workspaceAbs, err := filepathAbs(workspace)
	if err != nil {
		return err
	}
	paths := append([]string{workspace}, runtimePaths...)
	seen := make(map[string]struct{}, len(paths))
	for _, path := range paths {
		if path == "" {
			continue
		}
		absolute, err := filepathAbs(path)
		if err != nil {
			return err
		}
		if _, ok := seen[absolute]; ok {
			continue
		}
		seen[absolute] = struct{}{}
	}

	ruleset, err := createRuleset(handledAccess(abi))
	if err != nil {
		return err
	}
	defer unix.Close(ruleset)
	for path := range seen {
		access := uint64(readAccess)
		if path == workspaceAbs {
			access = readAccess
		}
		if err := addPathRule(ruleset, path, access); err != nil {
			return err
		}
	}
	if tool != "" {
		absolute, err := filepathAbs(tool)
		if err != nil {
			return err
		}
		absolute, err = filepath.EvalSymlinks(absolute)
		if err != nil {
			return fmt.Errorf("resolve tool %q: %w", tool, err)
		}
		if err := addPathRule(ruleset, absolute, readAccess|execAccess); err != nil {
			return err
		}
		interpreter, err := elfInterpreter(absolute)
		if err != nil {
			return err
		}
		if interpreter != "" {
			if err := addPathRule(ruleset, interpreter, readAccess|execAccess); err != nil {
				return err
			}
		}
	}
	if err := limitTaskCount(64); err != nil {
		return err
	}
	if err := unix.Prctl(unix.PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0); err != nil {
		return fmt.Errorf("no_new_privs: %w", err)
	}
	if err := restrictSelf(ruleset); err != nil {
		return fmt.Errorf("landlock restrict_self: %w", err)
	}
	if err := applySeccomp(); err != nil {
		return fmt.Errorf("seccomp: %w", err)
	}
	if unix.Gettid() != threadID {
		return errors.New("sandbox thread migrated during setup")
	}
	return nil
}

func filepathAbs(path string) (string, error) {
	absolute, err := filepath.Abs(path)
	if err != nil {
		return "", fmt.Errorf("resolve sandbox path %q: %w", path, err)
	}
	if _, err := os.Stat(absolute); err != nil {
		return "", fmt.Errorf("stat sandbox path %q: %w", path, err)
	}
	return absolute, nil
}

func elfInterpreter(path string) (string, error) {
	file, err := elf.Open(path)
	if err != nil {
		return "", nil
	}
	defer file.Close()
	for _, program := range file.Progs {
		if program.Type != elf.PT_INTERP {
			continue
		}
		data, err := io.ReadAll(program.Open())
		if err != nil {
			return "", fmt.Errorf("read ELF interpreter for %s: %w", path, err)
		}
		return strings.TrimRight(string(data), "\x00"), nil
	}
	return "", nil
}

// limitTaskCount prefers the gateway's per-sandbox cgroup and falls back to a
// user-wide RLIMIT_NPROC headroom. An absolute RLIMIT can be below the user's
// existing process count and would break the Go runtime and the tool.
func limitTaskCount(headroom uint64) error {
	if path := os.Getenv("ANVIL_CGROUP_PATH"); path != "" {
		if err := applyCgroupTaskLimit(path, headroom); err == nil {
			if err := relaxNproc(); err != nil {
				return err
			}
			return nil
		}
	}
	return limitNproc(headroom)
}

// applyCgroupTaskLimit runs before Landlock/seccomp and before the final execve.
// The runner does not fork after moving itself into the cgroup, so the tool
// inherits the limit.
// CleanupStaleCgroups removes empty cgroups under root older than olderThan.
// The gateway calls this at startup to recover directories left by crashes or
// failed cleanup. Non-cgroup entries and non-empty cgroups are left alone.
// defaultCgroupRemover must stay rmdir-only. os.RemoveAll on cgroupfs can
// recursively destroy child cgroups; a non-empty directory is intentionally
// left for the next safe cleanup attempt.
var defaultCgroupRemover = func(path string) error {
	return os.Remove(path)
}

func CleanupStaleCgroups(root string, olderThan time.Duration) error {
	return cleanupStaleCgroups(root, olderThan, defaultCgroupRemover)
}

func cleanupStaleCgroups(root string, olderThan time.Duration, remove func(string) error) error {
	entries, err := os.ReadDir(root)
	if err != nil {
		return fmt.Errorf("read cgroup root: %w", err)
	}
	for _, entry := range entries {
		path := filepath.Join(root, entry.Name())
		info, err := entry.Info()
		if err != nil || !info.IsDir() || time.Since(info.ModTime()) < olderThan {
			continue
		}
		procs, err := os.ReadFile(filepath.Join(path, "cgroup.procs"))
		if err != nil || strings.TrimSpace(string(procs)) != "" {
			continue
		}
		_ = remove(path)
	}
	return nil
}

func applyCgroupTaskLimit(dir string, maxTasks uint64) error {
	if err := os.WriteFile(filepath.Join(dir, "pids.max"), []byte(strconv.FormatUint(maxTasks, 10)), 0o644); err != nil {
		return fmt.Errorf("write cgroup pids.max: %w", err)
	}
	if err := os.WriteFile(filepath.Join(dir, "cgroup.procs"), []byte(strconv.Itoa(os.Getpid())), 0o644); err != nil {
		return fmt.Errorf("write cgroup.procs: %w", err)
	}
	return nil
}

// relaxNproc removes the soft RLIMIT_NPROC clamp when a per-sandbox cgroup is
// active. The cgroup is then the effective limit; a lower hard RLIMIT still
// makes the effective limit min(pids.max, RLIMIT_NPROC).
func relaxNproc() error {
	var current unix.Rlimit
	if err := unix.Getrlimit(unix.RLIMIT_NPROC, &current); err != nil {
		return fmt.Errorf("getrlimit RLIMIT_NPROC: %w", err)
	}
	if current.Cur == current.Max {
		return nil
	}
	current.Cur = current.Max
	if err := unix.Setrlimit(unix.RLIMIT_NPROC, &current); err != nil {
		return fmt.Errorf("setrlimit RLIMIT_NPROC: %w", err)
	}
	return nil
}

func limitNproc(headroom uint64) error {
	tasks, err := countUserTasks()
	if err != nil {
		return err
	}
	limit := tasks + headroom
	var current unix.Rlimit
	if err := unix.Getrlimit(unix.RLIMIT_NPROC, &current); err != nil {
		return fmt.Errorf("getrlimit RLIMIT_NPROC: %w", err)
	}
	if current.Max != unix.RLIM_INFINITY && current.Max < limit {
		limit = current.Max
	}
	if current.Cur < limit {
		limit = current.Cur
	}
	if limit == 0 {
		limit = 1
	}
	next := unix.Rlimit{Cur: limit, Max: limit}
	if err := unix.Setrlimit(unix.RLIMIT_NPROC, &next); err != nil {
		return fmt.Errorf("setrlimit RLIMIT_NPROC: %w", err)
	}
	return nil
}

func countUserTasks() (uint64, error) {
	uid := os.Getuid()
	entries, err := os.ReadDir("/proc")
	if err != nil {
		return 0, fmt.Errorf("read /proc: %w", err)
	}
	var total uint64
	for _, entry := range entries {
		if _, err := strconv.Atoi(entry.Name()); err != nil {
			continue
		}
		status, err := os.ReadFile(filepath.Join("/proc", entry.Name(), "status"))
		if err != nil {
			continue
		}
		var processUID, threads uint64
		for _, line := range strings.Split(string(status), "\n") {
			switch {
			case strings.HasPrefix(line, "Uid:"):
				fields := strings.Fields(line)
				if len(fields) > 1 {
					processUID, _ = strconv.ParseUint(fields[1], 10, 64)
				}
			case strings.HasPrefix(line, "Threads:"):
				fields := strings.Fields(line)
				if len(fields) > 1 {
					threads, _ = strconv.ParseUint(fields[1], 10, 64)
				}
			}
		}
		if processUID == uint64(uid) {
			total += threads
		}
	}
	return total, nil
}

func handledAccess(abi int) uint64 {
	handled := uint64(readAccess | execAccess | writeAccess)
	if abi >= 2 {
		handled |= unix.LANDLOCK_ACCESS_FS_REFER
	}
	if abi >= 3 {
		handled |= unix.LANDLOCK_ACCESS_FS_TRUNCATE
	}
	return handled
}

func createRuleset(access uint64) (int, error) {
	attr := unix.LandlockRulesetAttr{Access_fs: access}
	fd, _, errno := unix.Syscall(
		unix.SYS_LANDLOCK_CREATE_RULESET,
		uintptr(unsafe.Pointer(&attr)),
		unsafe.Sizeof(attr),
		0,
	)
	if errno != 0 {
		return -1, fmt.Errorf("landlock_create_ruleset: %w", errno)
	}
	unix.CloseOnExec(int(fd))
	return int(fd), nil
}

func addPathRule(ruleset int, path string, access uint64) error {
	info, err := os.Stat(path)
	if err != nil {
		return fmt.Errorf("stat landlock path %q: %w", path, err)
	}
	if !info.IsDir() {
		access &^= unix.LANDLOCK_ACCESS_FS_READ_DIR
	}
	fd, err := unix.Open(path, unix.O_PATH|unix.O_CLOEXEC, 0)
	if err != nil {
		return fmt.Errorf("open landlock path %q: %w", path, err)
	}
	defer unix.Close(fd)
	rule := unix.LandlockPathBeneathAttr{
		Allowed_access: access,
		Parent_fd:      int32(fd),
	}
	_, _, errno := unix.Syscall6(
		unix.SYS_LANDLOCK_ADD_RULE,
		uintptr(ruleset),
		unix.LANDLOCK_RULE_PATH_BENEATH,
		uintptr(unsafe.Pointer(&rule)),
		0,
		0,
		0,
	)
	if errno != 0 {
		return fmt.Errorf("landlock_add_rule %q: %w", path, errno)
	}
	return nil
}

func restrictSelf(ruleset int) error {
	_, _, errno := unix.Syscall(
		unix.SYS_LANDLOCK_RESTRICT_SELF,
		uintptr(ruleset),
		0,
		0,
	)
	if errno != 0 {
		return errno
	}
	return nil
}

// RuntimePaths returns the minimal read-only runtime roots needed to start an
// allowlisted tool. Missing paths are ignored.
func RuntimePaths() []string {
	candidates := []string{
		"/usr/lib", "/usr/lib64", "/lib", "/lib64", "/etc/ld.so.cache",
		"/proc/self/maps", "/proc/self/fd",
	}
	var paths []string
	for _, path := range candidates {
		if _, err := os.Stat(path); err == nil {
			paths = append(paths, path)
		}
	}
	return paths
}
