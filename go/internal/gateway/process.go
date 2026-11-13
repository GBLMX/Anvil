package gateway

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/GBLMX/Anvil/go/internal/execpolicy"
	"github.com/GBLMX/Anvil/go/internal/execrunner"
	"github.com/GBLMX/Anvil/go/internal/fsroot"
)

type execParams struct {
	Tool           string   `json:"tool"`
	Args           []string `json:"args"`
	Cwd            string   `json:"cwd"`
	Stdin          string   `json:"stdin"`
	TimeoutMS      uint64   `json:"timeout_ms"`
	MaxOutputBytes uint64   `json:"max_output_bytes"`
}

type fetchParams struct {
	URL      string            `json:"url"`
	MaxBytes int64             `json:"max_bytes"`
	Headers  map[string]string `json:"headers"`
}

type fetchResult struct {
	Status      int    `json:"status"`
	ContentType string `json:"content_type"`
	Body        string `json:"body"`
	Truncated   bool   `json:"truncated"`
}

type execResult struct {
	Stdout          string `json:"stdout"`
	Stderr          string `json:"stderr"`
	ExitCode        int    `json:"exit_code"`
	TimedOut        bool   `json:"timed_out"`
	OutputTruncated bool   `json:"output_truncated"`
}

func (s *Server) fetch(parent context.Context, req request, state *connectionState) (interface{}, *rpcError) {
	if rpcErr := s.permissionError(state, "sys.fetch"); rpcErr != nil {
		return nil, rpcErr
	}
	if s.cfg.Fetcher == nil {
		return nil, &rpcError{Code: -32008, Message: "fetch unavailable"}
	}
	var params fetchParams
	if err := decodeParams(req.Params, &params); err != nil || params.URL == "" {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	response, err := s.cfg.Fetcher.Fetch(parent, params.URL, params.MaxBytes, params.Headers)
	if err != nil {
		return nil, &rpcError{Code: -32002, Message: "fetch policy denied"}
	}
	return fetchResult{
		Status:      response.Status,
		ContentType: response.ContentType,
		Body:        string(response.Body),
		Truncated:   response.Truncated,
	}, nil
}

func workspacePath(path string) (string, error) {
	if path == "" || strings.ContainsRune(path, 0) || filepath.IsAbs(path) {
		return "", fsroot.ErrInvalidPath
	}
	clean := filepath.Clean(path)
	if clean == "." || clean == ".." || strings.HasPrefix(clean, "../") {
		return "", fsroot.ErrInvalidPath
	}
	return clean, nil
}

func confinedDir(workspace, relative string) (string, error) {
	clean, err := workspacePath(relative)
	if err != nil {
		return "", err
	}
	root, err := filepath.EvalSymlinks(workspace)
	if err != nil {
		return "", err
	}
	candidate, err := filepath.EvalSymlinks(filepath.Join(root, clean))
	if err != nil {
		return "", err
	}
	rel, err := filepath.Rel(root, candidate)
	if err != nil || rel == ".." || strings.HasPrefix(rel, ".."+string(filepath.Separator)) {
		return "", fsroot.ErrInvalidPath
	}
	info, err := os.Stat(candidate)
	if err != nil || !info.IsDir() {
		return "", fsroot.ErrInvalidPath
	}
	return candidate, nil
}

func (s *Server) exec(parent context.Context, req request, state *connectionState) (interface{}, *rpcError) {
	if rpcErr := s.permissionError(state, "sys.exec"); rpcErr != nil {
		return nil, rpcErr
	}
	if s.cfg.ExecRunner == "" {
		return nil, &rpcError{Code: -32008, Message: "exec runner unavailable"}
	}
	var params execParams
	if err := decodeParams(req.Params, &params); err != nil {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	if len(params.Args) > 1 && params.Args[1] == "apply" && state.pluginID != "" {
		return nil, &rpcError{Code: -32002, Message: "policy denied"}
	}
	if err := execpolicy.Validate(params.Tool, params.Args); err != nil {
		return nil, &rpcError{Code: -32002, Message: "policy denied"}
	}
	timeout := s.cfg.ExecTimeout
	if params.TimeoutMS > 0 {
		requested := time.Duration(params.TimeoutMS) * time.Millisecond
		if requested > 0 && requested < timeout {
			timeout = requested
		}
	}
	if len(params.Stdin) > 4<<20 {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	cwd := s.cfg.Workspace
	if params.Cwd != "" {
		resolved, err := confinedDir(s.cfg.Workspace, params.Cwd)
		if err != nil {
			return nil, &rpcError{Code: -32602, Message: "invalid params"}
		}
		cwd = resolved
	}
	maxOutput := s.cfg.MaxOutput
	if params.MaxOutputBytes > 0 && params.MaxOutputBytes < uint64(maxOutput) {
		maxOutput = int(params.MaxOutputBytes)
	}
	ctx, cancel := context.WithTimeout(parent, timeout)
	defer cancel()
	home, _ := os.UserHomeDir()
	var runtimePaths []string
	var execPaths []string
	var extraEnv []string
	if home != "" {
		runtimePaths = append(runtimePaths, filepath.Join(home, ".rustup"), filepath.Join(home, ".cargo"))
		for _, pattern := range []string{
			filepath.Join(home, ".rustup", "toolchains", "*", "bin", "rustc"),
			filepath.Join(home, ".rustup", "toolchains", "*", "bin", "cargo"),
			filepath.Join(home, ".rustup", "toolchains", "*", "bin", "rustdoc"),
			filepath.Join(home, ".rustup", "toolchains", "*", "libexec", "rust-analyzer-proc-macro-srv"),
		} {
			matches, _ := filepath.Glob(pattern)
			execPaths = append(execPaths, matches...)
		}
		tmp := filepath.Join(s.cfg.Workspace, ".anvil-tmp")
		_ = os.MkdirAll(tmp, 0o700)
		extraEnv = append(extraEnv,
			"HOME="+home,
			"RUSTUP_HOME="+filepath.Join(home, ".rustup"),
			"CARGO_HOME="+filepath.Join(home, ".cargo"),
			"TMPDIR="+tmp,
			"PATH="+filepath.Join(home, ".cargo", "bin")+":/usr/bin:/bin",
			"RUSTC_WRAPPER=",
			"GIT_CONFIG_GLOBAL=/dev/null",
			"GIT_CONFIG_SYSTEM=/dev/null",
			"GIT_DIR="+filepath.Join(s.cfg.Workspace, ".git"),
			"GIT_WORK_TREE="+s.cfg.Workspace,
		)
	}
	args := []string{
		"-workspace", s.cfg.Workspace,
		"-tool", params.Tool,
		"-runtime-paths", strings.Join(runtimePaths, ","),
		"-exec-paths", strings.Join(execPaths, ","),
		"-workspace-write",
		"-env", strings.Join(extraEnv, ","),
		"--",
	}
	args = append(args, params.Args...)
	env := []string{}
	if s.cgroup != "" {
		env = append(env, "ANVIL_CGROUP_PATH="+s.cgroup)
	}
	result, err := execrunner.Run(ctx, execrunner.ProcessConfig{
		Path:      s.cfg.ExecRunner,
		Args:      args,
		Env:       env,
		Dir:       cwd,
		Stdin:     []byte(params.Stdin),
		Timeout:   timeout,
		MaxOutput: maxOutput,
	})
	if err != nil {
		return nil, &rpcError{Code: -32008, Message: "exec unavailable"}
	}
	return execResult{
		Stdout:          string(result.Stdout),
		Stderr:          string(result.Stderr),
		ExitCode:        result.ExitCode,
		TimedOut:        result.TimedOut,
		OutputTruncated: result.OutputTruncated,
	}, nil
}
