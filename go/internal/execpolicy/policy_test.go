package execpolicy

import (
	"errors"
	"testing"
)

func TestValidateAllowedArgs(t *testing.T) {
	if err := Validate("/usr/bin/rg", []string{"pattern", "src", "-n", "--json", "--glob", "*.go"}); err != nil {
		t.Fatalf("rg: %v", err)
	}
	if err := Validate("/usr/bin/fd", []string{"pattern", "src", "--type", "f", "--max-depth", "3"}); err != nil {
		t.Fatalf("fd: %v", err)
	}
}

func TestRejectsExecAndConfigFlags(t *testing.T) {
	for _, args := range [][]string{
		{"--pre", "/bin/sh"},
		{"--pre-glob", "*.md"},
		{"--config-path", "/tmp/config"},
		{"--unknown"},
	} {
		if err := Validate("/usr/bin/rg", args); !errors.Is(err, ErrInvalidArgs) {
			t.Fatalf("rg %v: got %v", args, err)
		}
	}
	for _, args := range [][]string{
		{"--exec", "id"},
		{"--exec-batch", "id"},
		{"--unknown"},
	} {
		if err := Validate("/usr/bin/fd", args); !errors.Is(err, ErrInvalidArgs) {
			t.Fatalf("fd %v: got %v", args, err)
		}
	}
}

func TestValidatesGitDiff(t *testing.T) {
	args := []string{"--no-pager", "diff", "--no-ext-diff", "--no-textconv", "--no-color", "--unified=0", "HEAD", "--", "src/main.go"}
	if err := Validate("/usr/bin/git", args); err != nil {
		t.Fatalf("git: %v", err)
	}
	args[5] = "--unified=3"
	if err := Validate("/usr/bin/git", args); err != nil {
		t.Fatalf("git full diff: %v", err)
	}
	staged := []string{"--no-pager", "diff", "--cached", "--no-ext-diff", "--no-textconv", "--no-color", "--unified=3", "HEAD", "--", "src/main.go"}
	if err := Validate("/usr/bin/git", staged); err != nil {
		t.Fatalf("git staged diff: %v", err)
	}
	if staged[2] != "--cached" {
		t.Fatalf("validator mutated staged args: %v", staged)
	}
	for _, bad := range [][]string{
		{"diff"},
		{"--no-pager", "show", "HEAD"},
		{"--no-pager", "diff", "HEAD", "--", "../secret"},
		{"--no-pager", "diff", "--no-ext-diff", "--no-textconv", "--no-color", "--unified=1", "HEAD", "--", "src/main.go"},
	} {
		if err := Validate("/usr/bin/git", bad); !errors.Is(err, ErrInvalidArgs) {
			t.Fatalf("git %v: got %v", bad, err)
		}
	}
}

func TestValidatesGitApply(t *testing.T) {
	for _, args := range [][]string{
		{"--no-pager", "apply", "--cached", "--whitespace=nowarn", "--", ".anvil-tmp/git-hunk.patch"},
		{"--no-pager", "apply", "--cached", "--whitespace=nowarn", "-R", "--", ".anvil-tmp/git-hunk.patch"},
	} {
		if err := Validate("/usr/bin/git", args); err != nil {
			t.Fatalf("git apply %v: %v", args, err)
		}
	}
	for _, args := range [][]string{
		{"--no-pager", "apply", "--cached", "--whitespace=nowarn", "--", "outside.patch"},
		{"--no-pager", "apply", "--cached", "--whitespace=nowarn", "--", "../git-hunk.patch"},
		{"--no-pager", "apply", "--cached", "--whitespace=nowarn", "-R", ".anvil-tmp/git-hunk.patch"},
	} {
		if err := Validate("/usr/bin/git", args); !errors.Is(err, ErrInvalidArgs) {
			t.Fatalf("git apply %v: got %v", args, err)
		}
	}
}

func TestRejectsUnknownTool(t *testing.T) {
	if err := Validate("/bin/sh", nil); !errors.Is(err, ErrUnknownTool) {
		t.Fatalf("got %v", err)
	}
}
