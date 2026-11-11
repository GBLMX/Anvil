package execrunner

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

func TestGitSandboxHelper(t *testing.T) {
	if os.Getenv("ANVIL_GIT_SANDBOX_HELPER") != "1" {
		return
	}
	workspace := os.Getenv("ANVIL_GIT_SANDBOX_WORKSPACE")
	if err := os.Chdir(workspace); err != nil {
		_, _ = fmt.Fprintf(os.Stderr, "chdir: %v\n", err)
		os.Exit(2)
	}
	if err := ApplyToolForLSP(workspace, "/usr/bin/git", RuntimePaths(), nil); err != nil {
		_, _ = fmt.Fprintf(os.Stderr, "apply: %v\n", err)
		os.Exit(2)
	}
	env := []string{
		"PATH=/usr/bin:/bin",
		"HOME=" + workspace,
		"GIT_CONFIG_GLOBAL=/dev/null",
		"GIT_CONFIG_SYSTEM=/dev/null",
		"GIT_DIR=" + filepath.Join(workspace, ".git"),
		"GIT_WORK_TREE=" + workspace,
	}
	args := []string{"--no-pager", "diff", "--no-ext-diff", "--no-textconv", "--no-color", "--unified=0", "HEAD", "--", "a.txt"}
	if err := Exec("/usr/bin/git", append([]string{"/usr/bin/git"}, args...), env); err != nil {
		_, _ = fmt.Fprintf(os.Stderr, "exec: %v\n", err)
		os.Exit(3)
	}
	os.Exit(0)
}

func TestGitDiffUnderSandbox(t *testing.T) {
	if _, err := os.Stat("/usr/bin/git"); err != nil {
		t.Skip("git unavailable")
	}
	workspace := t.TempDir()
	git := func(args ...string) {
		t.Helper()
		cmd := exec.Command("/usr/bin/git", args...)
		cmd.Dir = workspace
		cmd.Env = append(os.Environ(), "GIT_CONFIG_GLOBAL=/dev/null", "GIT_CONFIG_SYSTEM=/dev/null")
		if out, err := cmd.CombinedOutput(); err != nil {
			t.Fatalf("git %v: %v\n%s", args, err, out)
		}
	}
	git("init", "-q")
	if err := os.WriteFile(filepath.Join(workspace, "a.txt"), []byte("one\ntwo\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	git("-c", "user.name=Test", "-c", "user.email=t@example.com", "add", "a.txt")
	git("-c", "user.name=Test", "-c", "user.email=t@example.com", "commit", "-qm", "init")
	if err := os.WriteFile(filepath.Join(workspace, "a.txt"), []byte("one\nTWO\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	cmd := exec.Command(os.Args[0], "-test.run=TestGitSandboxHelper", "-test.v")
	cmd.Env = append(os.Environ(),
		"ANVIL_GIT_SANDBOX_HELPER=1",
		"ANVIL_GIT_SANDBOX_WORKSPACE="+workspace,
	)
	out, err := cmd.CombinedOutput()
	if err != nil {
		if exitErr, ok := err.(*exec.ExitError); ok && exitErr.ExitCode() == 2 {
			t.Skipf("kernel sandbox unavailable: %s", out)
		}
		t.Fatalf("helper failed: %v\n%s", err, out)
	}
	if !strings.Contains(string(out), "diff --git") {
		t.Fatalf("no diff output:\n%s", out)
	}
}

func TestGitApplySandboxHelper(t *testing.T) {
	if os.Getenv("ANVIL_GIT_APPLY_HELPER") != "1" {
		return
	}
	workspace := os.Getenv("ANVIL_GIT_SANDBOX_WORKSPACE")
	if err := os.Chdir(workspace); err != nil {
		_, _ = fmt.Fprintf(os.Stderr, "chdir: %v\n", err)
		os.Exit(2)
	}
	if err := ApplyToolForLSP(workspace, "/usr/bin/git", RuntimePaths(), nil); err != nil {
		_, _ = fmt.Fprintf(os.Stderr, "apply: %v\n", err)
		os.Exit(2)
	}
	env := []string{
		"PATH=/usr/bin:/bin",
		"HOME=" + workspace,
		"GIT_CONFIG_GLOBAL=/dev/null",
		"GIT_CONFIG_SYSTEM=/dev/null",
		"GIT_DIR=" + filepath.Join(workspace, ".git"),
		"GIT_WORK_TREE=" + workspace,
	}
	args := []string{"--no-pager", "apply", "--cached", "--whitespace=nowarn", "--", ".anvil-tmp/git-hunk.patch"}
	if err := Exec("/usr/bin/git", append([]string{"/usr/bin/git"}, args...), env); err != nil {
		_, _ = fmt.Fprintf(os.Stderr, "exec: %v\n", err)
		os.Exit(3)
	}
	os.Exit(0)
}

func TestGitApplyUnderSandbox(t *testing.T) {
	if _, err := os.Stat("/usr/bin/git"); err != nil {
		t.Skip("git unavailable")
	}
	workspace := t.TempDir()
	git := func(args ...string) string {
		t.Helper()
		cmd := exec.Command("/usr/bin/git", args...)
		cmd.Dir = workspace
		cmd.Env = append(os.Environ(), "GIT_CONFIG_GLOBAL=/dev/null", "GIT_CONFIG_SYSTEM=/dev/null")
		out, err := cmd.CombinedOutput()
		if err != nil {
			t.Fatalf("git %v: %v\n%s", args, err, out)
		}
		return string(out)
	}
	git("init", "-q")
	if err := os.WriteFile(filepath.Join(workspace, "a.txt"), []byte("one\ntwo\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	git("-c", "user.name=Test", "-c", "user.email=t@example.com", "add", "a.txt")
	git("-c", "user.name=Test", "-c", "user.email=t@example.com", "commit", "-qm", "init")
	if err := os.WriteFile(filepath.Join(workspace, "a.txt"), []byte("one\nTWO\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	patch := git("diff", "--", "a.txt")
	if err := os.MkdirAll(filepath.Join(workspace, ".anvil-tmp"), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(workspace, ".anvil-tmp/git-hunk.patch"), []byte(patch), 0o600); err != nil {
		t.Fatal(err)
	}
	cmd := exec.Command(os.Args[0], "-test.run=TestGitApplySandboxHelper", "-test.v")
	cmd.Env = append(os.Environ(),
		"ANVIL_GIT_APPLY_HELPER=1",
		"ANVIL_GIT_SANDBOX_WORKSPACE="+workspace,
	)
	out, err := cmd.CombinedOutput()
	if err != nil {
		if exitErr, ok := err.(*exec.ExitError); ok && exitErr.ExitCode() == 2 {
			t.Skipf("kernel sandbox unavailable: %s", out)
		}
		t.Fatalf("helper failed: %v\n%s", err, out)
	}
	cached := git("diff", "--cached", "--", "a.txt")
	if !strings.Contains(cached, "+TWO") || !strings.Contains(cached, "-two") {
		t.Fatalf("patch not staged:\n%s", cached)
	}
}
