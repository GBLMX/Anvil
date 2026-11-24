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

// ProcessConfig describes one supervised tool process. A nil Env means an
// empty environment; callers must pass every variable the tool needs.
type ProcessConfig struct {
	Path      string
	Args      []string
	Env       []string
	Dir       string
	Stdin     []byte
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
	if cfg.Path[0] != '/' {
		return ProcessResult{}, errors.New("process path must be absolute")
	}
	maxOutput := cfg.MaxOutput
	if maxOutput <= 0 {
		maxOutput = DefaultMaxOutput
	}

	cmd := exec.Command(cfg.Path, cfg.Args...)
	if cfg.Env == nil {
		cmd.Env = []string{}
	} else {
		cmd.Env = cfg.Env
	}
	cmd.Dir = cfg.Dir
	if len(cfg.Stdin) > 0 {
		cmd.Stdin = bytes.NewReader(cfg.Stdin)
	}
	cmd.SysProcAttr = &syscall.SysProcAttr{Setpgid: true, Pdeathsig: syscall.SIGKILL}
	cmd.WaitDelay = 2 * time.Second

	var (
		killOnce  sync.Once
		pgid      atomic.Int64
		timedOut  atomic.Bool
		truncated atomic.Bool
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
			truncated.Store(true)
			killGroup()
		},
	}
	stdout := &limitedOutput{buffer: &bytes.Buffer{}, budget: budget}
	stderr := &limitedOutput{buffer: &bytes.Buffer{}, budget: budget}
	cmd.Stdout = stdout
	cmd.Stderr = stderr

	if err := cmd.Start(); err != nil {
		return ProcessResult{}, fmt.Errorf("start %s: %w", cfg.Path, err)
	}
	pgid.Store(int64(cmd.Process.Pid))
	if cfg.Timeout > 0 {
		var cancel context.CancelFunc
		ctx, cancel = context.WithTimeout(ctx, cfg.Timeout)
		defer cancel()
	}
	stop := context.AfterFunc(ctx, func() {
		timedOut.Store(true)
		killGroup()
	})
	defer stop()

	waitErr := cmd.Wait()
	stop()
	killGroup()
	exitCode := 0
	if cmd.ProcessState != nil {
		exitCode = cmd.ProcessState.ExitCode()
	}
	result := ProcessResult{
		Stdout:          append([]byte(nil), stdout.buffer.Bytes()...),
		Stderr:          append([]byte(nil), stderr.buffer.Bytes()...),
		ExitCode:        exitCode,
		TimedOut:        timedOut.Load(),
		OutputTruncated: truncated.Load(),
	}
	if waitErr != nil && !result.TimedOut && !result.OutputTruncated {
		if _, ok := errors.AsType[*exec.ExitError](waitErr); !ok {
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
