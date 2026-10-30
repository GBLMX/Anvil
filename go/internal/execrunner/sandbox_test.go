package execrunner

import (
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"testing"

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
	if err := Apply(workspace, RuntimePaths()); err != nil {
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

func TestRuntimePathsAreAbsolute(t *testing.T) {
	for _, path := range RuntimePaths() {
		if path == "" || path[0] != '/' {
			t.Fatalf("runtime path is not absolute: %q", path)
		}
	}
}
