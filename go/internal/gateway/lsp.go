package gateway

import (
	"context"
	"encoding/json"
	"log"
	"os"
	"path/filepath"
	"strings"

	"github.com/GBLMX/Anvil/go/internal/execpolicy"
	"github.com/GBLMX/Anvil/go/internal/lsp"
)

type lspStartParams struct {
	Tool string   `json:"tool"`
	Args []string `json:"args"`
}

type lspSendParams struct {
	Data json.RawMessage `json:"data"`
}

func (s *Server) lspStart(ctx context.Context, req request, state *connectionState) (interface{}, *rpcError) {
	if !s.cfg.NoSandboxLSP {
		if rpcErr := s.permissionError(state, "sys.exec"); rpcErr != nil {
			return nil, rpcErr
		}
	}
	if !s.cfg.NoSandboxLSP && s.cfg.ExecRunner == "" {
		return nil, &rpcError{Code: -32008, Message: "exec runner unavailable"}
	}
	var params lspStartParams
	if err := decodeParams(req.Params, &params); err != nil || params.Tool == "" {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	if s.cfg.NoSandboxLSP {
		home, _ := os.UserHomeDir()
		path := os.Getenv("PATH")
		if path == "" {
			path = "/usr/bin:/bin"
		}
		env := []string{
			"HOME=" + home,
			"RUSTUP_HOME=" + filepath.Join(home, ".rustup"),
			"CARGO_HOME=" + filepath.Join(home, ".cargo"),
			"PATH=" + path,
			"RUSTC_WRAPPER=",
		}
		manager, err := lsp.Start(ctx, lsp.Config{
			Command: params.Tool,
			Args:    params.Args,
			Dir:     s.cfg.Workspace,
			Env:     env,
		})
		if err != nil {
			return nil, &rpcError{Code: -32008, Message: "lsp start unavailable"}
		}
		state.lspMu.Lock()
		old := state.lsp
		state.lsp = manager
		state.lspMu.Unlock()
		if old != nil {
			_ = old.Close()
		}
		go s.forwardLSP(ctx, state, manager)
		go s.forwardLSPStderr(ctx, state, manager)
		go s.watchLSP(ctx, state, manager)
		return map[string]bool{"started": true}, nil
	}
	if err := execpolicy.Validate(params.Tool, params.Args); err != nil {
		return nil, &rpcError{Code: -32002, Message: "policy denied"}
	}
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
	manager, err := lsp.Start(ctx, lsp.Config{
		Command: s.cfg.ExecRunner,
		Args:    args,
		Dir:     s.cfg.Workspace,
		Env:     env,
	})
	if err != nil {
		return nil, &rpcError{Code: -32008, Message: "lsp start unavailable"}
	}
	state.lspMu.Lock()
	old := state.lsp
	state.lsp = manager
	state.lspMu.Unlock()
	if old != nil {
		_ = old.Close()
	}
	go s.forwardLSP(ctx, state, manager)
	go s.forwardLSPStderr(ctx, state, manager)
	go s.watchLSP(ctx, state, manager)
	return map[string]bool{"started": true}, nil
}

func (s *Server) lspSend(req request, state *connectionState) (interface{}, *rpcError) {
	state.lspMu.Lock()
	manager := state.lsp
	state.lspMu.Unlock()
	if manager == nil {
		return nil, &rpcError{Code: -32008, Message: "lsp not running"}
	}
	var params lspSendParams
	if err := decodeParams(req.Params, &params); err != nil || len(params.Data) == 0 {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	log.Printf("LSP In: %s", string(params.Data))
	if err := manager.Send(params.Data); err != nil {
		return nil, &rpcError{Code: -32008, Message: "lsp send failed"}
	}
	return map[string]bool{"sent": true}, nil
}

func (s *Server) forwardLSP(ctx context.Context, state *connectionState, manager *lsp.Manager) {
	for {
		body, err := manager.Recv()
		if err != nil {
			if ctx.Err() == nil {
				s.notifyLSP(state, "lsp.crashed", map[string]string{"message": err.Error()})
			}
			return
		}
		log.Printf("LSP Out: %s", string(body))
		s.notifyLSP(state, "lsp.message", map[string]json.RawMessage{"data": body})
	}
}

func (s *Server) watchLSP(ctx context.Context, state *connectionState, manager *lsp.Manager) {
	_ = manager.Wait()
	if ctx.Err() == nil {
		s.notifyLSP(state, "lsp.crashed", map[string]string{"message": "language server exited"})
	}
}

func (s *Server) forwardLSPStderr(ctx context.Context, state *connectionState, manager *lsp.Manager) {
	for {
		line, err := manager.RecvStderr()
		if err != nil {
			return
		}
		if ctx.Err() != nil {
			return
		}
		s.notifyLSP(state, "lsp.stderr", map[string]string{"message": line})
	}
}

func (s *Server) notifyLSP(state *connectionState, method string, params interface{}) {
	if state.conn == nil || state.writeMu == nil {
		return
	}
	payload, err := json.Marshal(map[string]interface{}{
		"jsonrpc": "2.0",
		"method":  method,
		"params":  params,
	})
	if err != nil {
		return
	}
	state.writeMu.Lock()
	defer state.writeMu.Unlock()
	_ = writeFrame(state.conn, payload)
}
