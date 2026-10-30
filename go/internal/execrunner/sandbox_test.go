package execrunner

import (
	"bufio"
	"bytes"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"testing"
	"time"

	"golang.org/x/sys/unix"
)

func TestSandboxHelper(t *testing.T) {
	if os.Getenv("ANVIL_SANDBOX_HELPER") != "1" {
		return
	}
	workspace := os.Getenv("ANVIL_SANDBOX_WORKSPACE")
	if err := Apply(workspace, RuntimePaths()); err != nil {
		os.Stderr.WriteString(err.Error() + "\n")
		os.Exit(2)
	}
	if _, err := os.ReadFile(filepath.Join(workspace, "ok")); err != nil {
		os.Exit(3)
	}
	if err := os.WriteFile(filepath.Join(workspace, "new"), []byte("no"), 0o600); !errors.Is(err, unix.EACCES) {
		os.Exit(14)
	}
	if _, err := os.ReadFile("/etc/passwd"); err == nil {
		os.Exit(4)
	}
	if _, err := os.ReadFile(filepath.Join(workspace, "escape")); !errors.Is(err, unix.EACCES) {
		os.Exit(5)
	}
	if fd, err := unix.Socket(unix.AF_INET, unix.SOCK_STREAM, 0); err == nil {
		_ = unix.Close(fd)
		os.Exit(6)
	}
	if err := unix.Unshare(unix.CLONE_NEWUSER); !errors.Is(err, unix.EPERM) {
		os.Exit(15)
	}
	if os.Getenv("ANVIL_CGROUP_PATH") == "" {
		var taskLimit unix.Rlimit
		if err := unix.Getrlimit(unix.RLIMIT_NPROC, &taskLimit); err != nil || taskLimit.Cur == unix.RLIM_INFINITY {
			os.Exit(16)
		}
	}
	var usage unix.Rusage
	if err := unix.Getrusage(unix.RUSAGE_SELF, &usage); !errors.Is(err, unix.EPERM) {
		os.Exit(7)
	}
	if _, err := unix.Mmap(-1, 0, 4096, unix.PROT_READ|unix.PROT_WRITE|unix.PROT_EXEC, unix.MAP_PRIVATE|unix.MAP_ANONYMOUS); !errors.Is(err, unix.EPERM) {
		os.Exit(8)
	}
	page, err := unix.Mmap(-1, 0, 4096, unix.PROT_READ|unix.PROT_WRITE, unix.MAP_PRIVATE|unix.MAP_ANONYMOUS)
	if err != nil {
		os.Exit(9)
	}
	if err := unix.Mprotect(page, unix.PROT_EXEC); !errors.Is(err, unix.EPERM) {
		os.Exit(10)
	}
	_ = unix.Munmap(page)
	_, _, errno := unix.Syscall(unix.SYS_CLONE3, 0, 0, 0)
	if errno != unix.ENOSYS {
		os.Exit(11)
	}
	os.Exit(0)
}

func TestApplyDeniesOutsideReadAndNetwork(t *testing.T) {
	if _, err := LandlockABI(); err != nil {
		t.Skipf("Landlock unavailable: %v", err)
	}
	workspace := t.TempDir()
	if err := os.WriteFile(filepath.Join(workspace, "ok"), []byte("ok"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink("/etc/passwd", filepath.Join(workspace, "escape")); err != nil {
		t.Fatal(err)
	}
	cmd := exec.Command(os.Args[0], "-test.run=TestSandboxHelper")
	cmd.Env = append(os.Environ(),
		"ANVIL_SANDBOX_HELPER=1",
		"ANVIL_SANDBOX_WORKSPACE="+workspace,
	)
	out, err := cmd.CombinedOutput()
	if err != nil {
		var exitErr *exec.ExitError
		if errors.As(err, &exitErr) && exitErr.ExitCode() == 2 {
			t.Skipf("kernel sandbox unavailable in this environment: %s", out)
		}
		t.Fatalf("sandbox helper failed: %v\n%s", err, out)
	}
	if string(out) != "" {
		t.Fatalf("unexpected helper output: %s", out)
	}
}

func TestSandboxToolHelper(t *testing.T) {
	if os.Getenv("ANVIL_SANDBOX_TOOL_HELPER") != "1" {
		return
	}
	workspace := os.Getenv("ANVIL_SANDBOX_WORKSPACE")
	tool := os.Getenv("ANVIL_SANDBOX_TOOL")
	if err := ApplyTool(workspace, tool, RuntimePaths()); err != nil {
		os.Stderr.WriteString(err.Error() + "\n")
		os.Exit(2)
	}
	if err := Exec(tool, []string{tool, "--version"}, []string{"PATH=/usr/bin:/bin", "LANG=C.UTF-8"}); err != nil {
		os.Stderr.WriteString(err.Error() + "\n")
		os.Exit(3)
	}
}

func TestSandboxedToolsStart(t *testing.T) {
	if _, err := LandlockABI(); err != nil {
		t.Skipf("Landlock unavailable: %v", err)
	}
	workspace := t.TempDir()
	for _, tool := range []string{"/usr/bin/rg", "/usr/bin/fd"} {
		if _, err := os.Stat(tool); err != nil {
			t.Skipf("%s unavailable", tool)
		}
		cmd := exec.Command(os.Args[0], "-test.run=TestSandboxToolHelper")
		cmd.Env = append(os.Environ(),
			"ANVIL_SANDBOX_TOOL_HELPER=1",
			"ANVIL_SANDBOX_TOOL="+tool,
			"ANVIL_SANDBOX_WORKSPACE="+workspace,
		)
		out, err := cmd.CombinedOutput()
		if err != nil {
			t.Fatalf("%s failed under sandbox: %v\n%s", tool, err, out)
		}
	}
}

func TestSandboxForbiddenExecHelper(t *testing.T) {
	if os.Getenv("ANVIL_SANDBOX_FORBIDDEN_EXEC") != "1" {
		return
	}
	workspace := os.Getenv("ANVIL_SANDBOX_WORKSPACE")
	if err := ApplyTool(workspace, "/usr/bin/rg", RuntimePaths()); err != nil {
		os.Exit(2)
	}
	if err := Exec("/bin/sh", []string{"/bin/sh", "-c", "exit 0"}, []string{"PATH=/usr/bin:/bin"}); !errors.Is(err, unix.EACCES) {
		os.Exit(3)
	}
	os.Exit(0)
}

func TestSandboxDeniesNonAllowlistedExec(t *testing.T) {
	if _, err := LandlockABI(); err != nil {
		t.Skipf("Landlock unavailable: %v", err)
	}
	if _, err := os.Stat("/usr/bin/rg"); err != nil {
		t.Skip("/usr/bin/rg unavailable")
	}
	cmd := exec.Command(os.Args[0], "-test.run=TestSandboxForbiddenExecHelper")
	cmd.Env = append(os.Environ(),
		"ANVIL_SANDBOX_FORBIDDEN_EXEC=1",
		"ANVIL_SANDBOX_WORKSPACE="+t.TempDir(),
	)
	out, err := cmd.CombinedOutput()
	if err != nil {
		t.Fatalf("forbidden exec test failed: %v\n%s", err, out)
	}
}

func TestCloseExtraFDsHelper(t *testing.T) {
	if os.Getenv("ANVIL_FD_HELPER") != "1" {
		return
	}
	file, err := os.Open(os.DevNull)
	if err != nil {
		os.Exit(2)
	}
	if file.Fd() < 3 {
		os.Exit(3)
	}
	if err := CloseExtraFDs(); err != nil {
		os.Exit(4)
	}
	flags, err := unix.FcntlInt(file.Fd(), unix.F_GETFD, 0)
	if err != nil || flags&unix.FD_CLOEXEC == 0 {
		os.Exit(5)
	}
	os.Exit(0)
}

func TestCloseExtraFDs(t *testing.T) {
	cmd := exec.Command(os.Args[0], "-test.run=TestCloseExtraFDsHelper")
	cmd.Env = append(os.Environ(), "ANVIL_FD_HELPER=1")
	out, err := cmd.CombinedOutput()
	if err != nil {
		t.Fatalf("fd helper failed: %v\n%s", err, out)
	}
}

func TestCgroupTaskLimitWrites(t *testing.T) {
	dir := t.TempDir()
	if err := applyCgroupTaskLimit(dir, 64); err != nil {
		t.Fatal(err)
	}
	limit, err := os.ReadFile(filepath.Join(dir, "pids.max"))
	if err != nil {
		t.Fatal(err)
	}
	if string(limit) != "64" {
		t.Fatalf("pids.max = %q", limit)
	}
	procs, err := os.ReadFile(filepath.Join(dir, "cgroup.procs"))
	if err != nil {
		t.Fatal(err)
	}
	if string(procs) != strconv.Itoa(os.Getpid()) {
		t.Fatalf("cgroup.procs = %q", procs)
	}
}

func TestCleanupStaleCgroupsDefaultRemoverDoesNotRecurse(t *testing.T) {
	root := t.TempDir()
	stale := filepath.Join(root, "stale")
	fresh := filepath.Join(root, "fresh")
	active := filepath.Join(root, "active")
	nonCgroup := filepath.Join(root, "not-a-cgroup")
	for _, dir := range []string{stale, fresh, active, nonCgroup} {
		if err := os.Mkdir(dir, 0o755); err != nil {
			t.Fatal(err)
		}
	}
	for _, dir := range []string{stale, fresh, active} {
		if err := os.WriteFile(filepath.Join(dir, "pids.max"), []byte("64"), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.WriteFile(filepath.Join(stale, "cgroup.procs"), nil, 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(fresh, "cgroup.procs"), nil, 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(active, "cgroup.procs"), []byte("123"), 0o644); err != nil {
		t.Fatal(err)
	}
	oldTime := time.Now().Add(-2 * time.Hour)
	for _, dir := range []string{stale, active, nonCgroup} {
		if err := os.Chtimes(dir, oldTime, oldTime); err != nil {
			t.Fatal(err)
		}
	}
	if err := CleanupStaleCgroups(root, time.Hour); err != nil {
		t.Fatal(err)
	}
	for _, dir := range []string{stale, fresh, active, nonCgroup} {
		if _, err := os.Stat(dir); err != nil {
			t.Fatalf("default remover recursed into %s: %v", dir, err)
		}
	}
}

func TestCleanupStaleCgroupsSelectsOnlyEmptyOldCgroups(t *testing.T) {
	root := t.TempDir()
	stale := filepath.Join(root, "stale")
	fresh := filepath.Join(root, "fresh")
	active := filepath.Join(root, "active")
	for _, dir := range []string{stale, fresh, active} {
		if err := os.Mkdir(dir, 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(dir, "cgroup.procs"), nil, 0o644); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.WriteFile(filepath.Join(active, "cgroup.procs"), []byte("123"), 0o644); err != nil {
		t.Fatal(err)
	}
	oldTime := time.Now().Add(-2 * time.Hour)
	if err := os.Chtimes(stale, oldTime, oldTime); err != nil {
		t.Fatal(err)
	}
	if err := os.Chtimes(active, oldTime, oldTime); err != nil {
		t.Fatal(err)
	}
	var removed []string
	remove := func(path string) error {
		removed = append(removed, filepath.Base(path))
		return nil
	}
	if err := cleanupStaleCgroups(root, time.Hour, remove); err != nil {
		t.Fatal(err)
	}
	if len(removed) != 1 || removed[0] != "stale" {
		t.Fatalf("removed = %v", removed)
	}
}

func infoMode(info os.FileInfo) string {
	if info == nil {
		return "missing"
	}
	return info.Mode().String()
}

func TestSymlinkRaceHelper(t *testing.T) {
	if os.Getenv("ANVIL_SYMLINK_RACE_HELPER") != "1" {
		return
	}
	workspace := os.Getenv("ANVIL_SANDBOX_WORKSPACE")
	if err := Apply(workspace, RuntimePaths()); err != nil {
		os.Stderr.WriteString(err.Error() + "\n")
		os.Exit(2)
	}
	fmt.Fprintln(os.Stdout, "ready")
	race := filepath.Join(workspace, "race")
	for i := 0; i < 5000; i++ {
		data, err := os.ReadFile(race)
		if err != nil {
			if errors.Is(err, unix.EACCES) {
				continue
			}
			info, statErr := os.Lstat(race)
			target, targetErr := os.Readlink(race)
			fmt.Fprintf(
				os.Stderr,
				"unexpected read error: %v; lstat=%s (%v), target=%q (%v)\n",
				err,
				infoMode(info),
				statErr,
				target,
				targetErr,
			)
			os.Exit(3)
		}
		if string(data) != "SAFE" {
			fmt.Fprintf(os.Stderr, "escaped symlink content: %q\n", data)
			os.Exit(4)
		}
	}
	os.Exit(0)
}

func TestSymlinkSwapCannotEscape(t *testing.T) {
	if _, err := LandlockABI(); err != nil {
		t.Skipf("Landlock unavailable: %v", err)
	}
	workspace := t.TempDir()
	if err := os.WriteFile(filepath.Join(workspace, "safe"), []byte("SAFE"), 0o600); err != nil {
		t.Fatal(err)
	}
	race := filepath.Join(workspace, "race")
	other := filepath.Join(workspace, "race.other")
	if err := os.Symlink("safe", race); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink("/etc/passwd", other); err != nil {
		t.Fatal(err)
	}
	cmd := exec.Command(os.Args[0], "-test.run=TestSymlinkRaceHelper")
	cmd.Env = append(os.Environ(),
		"ANVIL_SYMLINK_RACE_HELPER=1",
		"ANVIL_SANDBOX_WORKSPACE="+workspace,
	)
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	var stderr bytes.Buffer
	cmd.Stderr = &stderr
	if err := cmd.Start(); err != nil {
		t.Fatal(err)
	}
	line, err := bufio.NewReader(stdout).ReadString('\n')
	if err != nil || line != "ready\n" {
		_ = cmd.Process.Kill()
		_ = cmd.Wait()
		t.Fatalf("sandbox helper not ready: %q, %v, stderr=%s", line, err, stderr.String())
	}
	// Exchange two prepared symlinks atomically so the reader never observes
	// an intermediate create/remove window.
	for i := 0; i < 5000; i++ {
		if err := unix.Renameat2(unix.AT_FDCWD, other, unix.AT_FDCWD, race, unix.RENAME_EXCHANGE); err != nil {
			info, statErr := os.Lstat(race)
			target, targetErr := os.Readlink(race)
			t.Fatalf(
				"atomic symlink exchange failed at %d: %v; race mode=%v (%v), target=%q (%v)",
				i,
				err,
				infoMode(info),
				statErr,
				target,
				targetErr,
			)
		}
	}
	if err := cmd.Wait(); err != nil {
		info, statErr := os.Lstat(race)
		target, targetErr := os.Readlink(race)
		t.Fatalf(
			"symlink race helper failed: %v; race mode=%v (%v), target=%q (%v)\n%s",
			err,
			infoMode(info),
			statErr,
			target,
			targetErr,
			stderr.String(),
		)
	}
}

func TestSandboxLoaderHelper(t *testing.T) {
	if os.Getenv("ANVIL_SANDBOX_LOADER_HELPER") != "1" {
		return
	}
	workspace := os.Getenv("ANVIL_SANDBOX_WORKSPACE")
	if err := ApplyTool(workspace, "/usr/bin/rg", RuntimePaths()); err != nil {
		os.Exit(2)
	}
	if err := Exec("/lib64/ld-linux-x86-64.so.2", []string{"ld.so", "/bin/true"}, []string{"PATH=/usr/bin:/bin"}); err != nil {
		os.Exit(3)
	}
	os.Exit(4)
}

func TestSandboxDeniesLoaderExec(t *testing.T) {
	if _, err := LandlockABI(); err != nil {
		t.Skipf("Landlock unavailable: %v", err)
	}
	for _, path := range []string{"/usr/bin/rg", "/lib64/ld-linux-x86-64.so.2", "/bin/true"} {
		if _, err := os.Stat(path); err != nil {
			t.Skipf("%s unavailable", path)
		}
	}
	cmd := exec.Command(os.Args[0], "-test.run=TestSandboxLoaderHelper")
	cmd.Env = append(os.Environ(),
		"ANVIL_SANDBOX_LOADER_HELPER=1",
		"ANVIL_SANDBOX_WORKSPACE="+t.TempDir(),
	)
	out, err := cmd.CombinedOutput()
	if err == nil {
		t.Fatalf("ld.so executed a non-allowlisted binary: %s", out)
	}
	var exitErr *exec.ExitError
	if errors.As(err, &exitErr) && exitErr.ExitCode() == 0 {
		t.Fatalf("ld.so escaped the execute policy: %s", out)
	}
}

func TestRuntimePathsAreAbsolute(t *testing.T) {
	for _, path := range RuntimePaths() {
		if path == "" || path[0] != '/' {
			t.Fatalf("runtime path is not absolute: %q", path)
		}
	}
}
