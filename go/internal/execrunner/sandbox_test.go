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
	if _, err := os.ReadFile("/etc/passwd"); err == nil {
		os.Exit(4)
	}
	if fd, err := unix.Socket(unix.AF_INET, unix.SOCK_STREAM, 0); err == nil {
		_ = unix.Close(fd)
		os.Exit(5)
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

func TestRuntimePathsAreAbsolute(t *testing.T) {
	for _, path := range RuntimePaths() {
		if path == "" || path[0] != '/' {
			t.Fatalf("runtime path is not absolute: %q", path)
		}
	}
}
