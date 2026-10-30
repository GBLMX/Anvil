// Package execrunner applies the kernel sandbox used by sys.exec.
//
// The package is deliberately small: the gateway resolves policy and opens
// workspace/tool descriptors; this runner only applies Landlock, seccomp, and
// no_new_privs before the final exec.
package execrunner

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
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

// Apply confines the current process to read-only access beneath workspace
// plus read-only, executable runtime paths. Runtime execute access lets the
// dynamic loader and the allowlisted tool start.
//
// This function is irreversible. It must run in the exec-runner child, never in
// the gateway or editor process.
func Apply(workspace string, runtimePaths []string) error {
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
		var access uint64 = allReadExec
		if path == workspaceAbs {
			access = readAccess
		}
		if err := addPathRule(ruleset, path, access); err != nil {
			return err
		}
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
	return int(fd), nil
}

func addPathRule(ruleset int, path string, access uint64) error {
	info, err := os.Stat(path)
	if err != nil {
		return fmt.Errorf("stat landlock path %q: %w", path, err)
	}
	if !info.IsDir() {
		access = unix.LANDLOCK_ACCESS_FS_READ_FILE
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

// RuntimePaths returns the minimal read-only, executable runtime roots needed
// to start allowlisted tools. Missing paths are ignored.
func RuntimePaths() []string {
	candidates := []string{
		"/usr", "/lib", "/lib64", "/bin", "/sbin", "/etc/ld.so.cache",
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
