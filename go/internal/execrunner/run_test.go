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

func TestRunCapturesExitCode(t *testing.T) {
	result, err := Run(context.Background(), ProcessConfig{
		Path: "/bin/sh",
		Args: []string{"-c", "exit 7"},
	})
	if err != nil {
		t.Fatal(err)
	}
	if result.ExitCode != 7 {
		t.Fatalf("exit code = %d, want 7", result.ExitCode)
	}
}
