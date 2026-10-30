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

func TestRejectsUnknownTool(t *testing.T) {
	if err := Validate("/bin/sh", nil); !errors.Is(err, ErrUnknownTool) {
		t.Fatalf("got %v", err)
	}
}
