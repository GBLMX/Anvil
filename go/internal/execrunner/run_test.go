package execrunner

import (
	"context"
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestRunTimeoutKillsProcessGroup(t *testing.T) {
	marker := filepath.Join(t.TempDir(), "alive")
	start := time.Now()
	result, err := Run(context.Background(), ProcessConfig{
		Path:    "/bin/sh",
		Args:    []string{"-c", "(sleep 0.6; : > \"$1\") & wait", "sh", marker},
		Env:     []string{"PATH=/usr/bin:/bin"},
		Timeout: 100 * time.Millisecond,
	})
	if err != nil {
		t.Fatal(err)
	}
	if !result.TimedOut {
		t.Fatal("expected timeout")
	}
	if elapsed := time.Since(start); elapsed > 2*time.Second {
		t.Fatalf("timeout took too long: %s", elapsed)
	}
	time.Sleep(800 * time.Millisecond)
	if _, err := os.Stat(marker); err == nil {
		t.Fatal("descendant survived process-group kill")
	}
}

func TestRunOutputCap(t *testing.T) {
	result, err := Run(context.Background(), ProcessConfig{
		Path:      "/bin/sh",
		Args:      []string{"-c", "yes x"},
		Env:       []string{"PATH=/usr/bin:/bin"},
		Timeout:   5 * time.Second,
		MaxOutput: 1024,
	})
	if err != nil {
		t.Fatal(err)
	}
	if !result.OutputTruncated {
		t.Fatal("expected output truncation")
	}
	if len(result.Stdout)+len(result.Stderr) > 1024 {
		t.Fatalf("output exceeded cap: %d", len(result.Stdout)+len(result.Stderr))
	}
}

func TestRunRequiresAbsolutePath(t *testing.T) {
	if _, err := Run(context.Background(), ProcessConfig{Path: "sh"}); err == nil {
		t.Fatal("expected relative path rejection")
	}
}

func TestRunUsesEmptyEnvironmentByDefault(t *testing.T) {
	result, err := Run(context.Background(), ProcessConfig{Path: "/usr/bin/env"})
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Stdout) != 0 {
		t.Fatalf("environment leaked: %q", result.Stdout)
	}
}

func TestRunCancellationKillsProcessGroup(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	time.AfterFunc(50*time.Millisecond, cancel)
	result, err := Run(ctx, ProcessConfig{
		Path: "/bin/sh",
		Args: []string{"-c", "sleep 5"},
		Env:  []string{"PATH=/usr/bin:/bin"},
	})
	if err != nil {
		t.Fatal(err)
	}
	if !result.TimedOut {
		t.Fatal("expected cancellation to mark process as timed out")
	}
}

func TestRunCapturesExitCode(t *testing.T) {
	result, err := Run(context.Background(), ProcessConfig{
		Path: "/bin/sh",
		Args: []string{"-c", "exit 7"},
		Env:  []string{},
	})
	if err != nil {
		t.Fatal(err)
	}
	if result.ExitCode != 7 {
		t.Fatalf("exit code = %d, want 7", result.ExitCode)
	}
}
