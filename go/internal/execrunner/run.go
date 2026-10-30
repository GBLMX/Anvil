package execrunner

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"os/exec"
	"sync"
	"sync/atomic"
	"syscall"
	"time"
)

const DefaultMaxOutput = 1 << 20

var errOutputLimit = errors.New("output limit exceeded")

// ProcessConfig describes one supervised tool process.
type ProcessConfig struct {
	Path      string
	Args      []string
	Env       []string
	Dir       string
	Timeout   time.Duration
	MaxOutput int
}

// ProcessResult is available even when the process timed out or was killed.
type ProcessResult struct {
	Stdout          []byte
	Stderr          []byte
	ExitCode        int
	TimedOut        bool
	OutputTruncated bool
}

// Run starts Path in its own process group, enforces Timeout and a combined
// stdout/stderr limit, and kills the entire group on failure.
func Run(ctx context.Context, cfg ProcessConfig) (ProcessResult, error) {
	if ctx == nil {
		ctx = context.Background()
	}
	if cfg.Path == "" {
		return ProcessResult{}, errors.New("process path is required")
	}
	maxOutput := cfg.MaxOutput
	if maxOutput <= 0 {
		maxOutput = DefaultMaxOutput
	}

	cmd := exec.Command(cfg.Path, cfg.Args...)
	cmd.Env = cfg.Env
	cmd.Dir = cfg.Dir
	cmd.SysProcAttr = &syscall.SysProcAttr{Setpgid: true, Pdeathsig: syscall.SIGKILL}
	cmd.WaitDelay = 2 * time.Second

	var (
		mu       sync.Mutex
		result   ProcessResult
		killOnce sync.Once
		pgid     atomic.Int64
	)
	killGroup := func() {
		group := int(pgid.Load())
		if group <= 0 {
			return
		}
		killOnce.Do(func() {
			_ = syscall.Kill(-group, syscall.SIGKILL)
		})
	}
	budget := &outputBudget{
		remaining: maxOutput,
		onExceed: func() {
			mu.Lock()
			result.OutputTruncated = true
			mu.Unlock()
			killGroup()
		},
	}
	cmd.Stdout = &limitedOutput{buffer: &bytes.Buffer{}, budget: budget}
	cmd.Stderr = &limitedOutput{buffer: &bytes.Buffer{}, budget: budget}

	if err := cmd.Start(); err != nil {
		return ProcessResult{}, fmt.Errorf("start %s: %w", cfg.Path, err)
	}
	pgid.Store(int64(cmd.Process.Pid))

	var timer *time.Timer
	if cfg.Timeout > 0 {
		timer = time.AfterFunc(cfg.Timeout, func() {
			mu.Lock()
			result.TimedOut = true
			mu.Unlock()
			killGroup()
		})
	}
	done := make(chan struct{})
	go func() {
		select {
		case <-ctx.Done():
			mu.Lock()
			result.TimedOut = true
			mu.Unlock()
			killGroup()
		case <-done:
		}
	}()

	waitErr := cmd.Wait()
	close(done)
	if timer != nil {
		timer.Stop()
	}
	killGroup()
	if cmd.ProcessState != nil {
		result.ExitCode = cmd.ProcessState.ExitCode()
	}

	mu.Lock()
	stdout := append([]byte(nil), cmd.Stdout.(*limitedOutput).buffer.Bytes()...)
	stderr := append([]byte(nil), cmd.Stderr.(*limitedOutput).buffer.Bytes()...)
	timedOut := result.TimedOut
	truncated := result.OutputTruncated
	mu.Unlock()
	result.Stdout = stdout
	result.Stderr = stderr
	result.TimedOut = timedOut
	result.OutputTruncated = truncated

	if waitErr != nil && !timedOut && !truncated {
		var exitErr *exec.ExitError
		if !errors.As(waitErr, &exitErr) {
			return result, waitErr
		}
	}
	return result, nil
}

type outputBudget struct {
	mu        sync.Mutex
	remaining int
	onExceed  func()
}

func (b *outputBudget) take(size int) (int, bool) {
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.remaining <= 0 {
		return 0, true
	}
	if size > b.remaining {
		size = b.remaining
	}
	b.remaining -= size
	return size, b.remaining == 0
}

type limitedOutput struct {
	buffer *bytes.Buffer
	budget *outputBudget
}

func (w *limitedOutput) Write(p []byte) (int, error) {
	n, exhausted := w.budget.take(len(p))
	if n > 0 {
		_, _ = w.buffer.Write(p[:n])
	}
	if exhausted {
		w.budget.onExceed()
		return n, errOutputLimit
	}
	return n, nil
}
