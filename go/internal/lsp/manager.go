package lsp

import (
	"bufio"
	"context"
	"fmt"
	"io"
	"os/exec"
	"sync"
)

// Config describes one language-server process. It must already name a
// sandboxed launcher (for example anvil-exec-runner), never an ambient shell.
type Config struct {
	Command string
	Args    []string
	Dir     string
	Env     []string
}

// Manager owns a persistent, bidirectional LSP child.
type Manager struct {
	cmd    *exec.Cmd
	stdin  io.WriteCloser
	stdout *bufio.Reader
	stderr *bufio.Reader
	once   sync.Once
}

// Start launches the configured process with stdin/stdout pipes.
func Start(ctx context.Context, cfg Config) (*Manager, error) {
	if cfg.Command == "" {
		return nil, fmt.Errorf("lsp: command is required")
	}
	cmd := exec.CommandContext(ctx, cfg.Command, cfg.Args...)
	cmd.Dir = cfg.Dir
	cmd.Env = cfg.Env
	stdin, err := cmd.StdinPipe()
	if err != nil {
		return nil, err
	}
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		_ = stdin.Close()
		return nil, err
	}
	stderr, err := cmd.StderrPipe()
	if err != nil {
		_ = stdin.Close()
		return nil, err
	}
	if err := cmd.Start(); err != nil {
		_ = stdin.Close()
		return nil, err
	}
	return &Manager{
		cmd:    cmd,
		stdin:  stdin,
		stdout: bufio.NewReader(stdout),
		stderr: bufio.NewReader(stderr),
	}, nil
}

// Send writes one framed LSP JSON message.
func (m *Manager) Send(body []byte) error {
	return WriteFrame(m.stdin, body)
}

// Recv reads one framed LSP JSON message.
func (m *Manager) Recv() ([]byte, error) {
	return ReadFrame(m.stdout)
}

// RecvStderr reads one line from the server's stderr.
func (m *Manager) RecvStderr() (string, error) {
	return m.stderr.ReadString('\n')
}

// Wait waits for the server process to exit.
func (m *Manager) Wait() error {
	return m.cmd.Wait()
}

// Close closes stdin and waits for the process.
func (m *Manager) Close() error {
	var err error
	m.once.Do(func() {
		_ = m.stdin.Close()
		err = m.cmd.Wait()
	})
	return err
}
