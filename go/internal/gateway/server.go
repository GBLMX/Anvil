// Package gateway implements the per-session Unix-socket policy gateway.
package gateway

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/base64"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log"
	"net"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"sync"
	"time"

	"github.com/GBLMX/Anvil/go/internal/execpolicy"
	"github.com/GBLMX/Anvil/go/internal/execrunner"
	"github.com/GBLMX/Anvil/go/internal/fetch"
	"github.com/GBLMX/Anvil/go/internal/fsroot"
	"github.com/GBLMX/Anvil/go/internal/lsp"
	"golang.org/x/sys/unix"
)

const (
	// MaxFrameLen is the largest accepted JSON-RPC frame.
	MaxFrameLen = 8 << 20
	// maxReadChunk is the maximum JSON/base64 file chunk.
	// 5.5 MiB encodes to about 7.33 MiB of base64, leaving frame overhead room.
	maxReadChunk = 5*1024*1024 + 512*1024
	// maxBinaryReadChunk is the maximum raw chunk; one flag byte plus 7 MiB
	// still fits the 8 MiB frame.
	maxBinaryReadChunk = 7 << 20
	// maxWriteChunk is the maximum file bytes accepted by one sys.write_file.
	maxWriteChunk       = maxReadChunk
	maxBinaryWriteChunk = 7 << 20
	// Protocol is the v1 JSON-RPC protocol name.
	Protocol = "anvil-jsonrpc-1"
	// APIVersion is the public API version.
	APIVersion = "0.1.0"
)

// Config is the minimum per-session gateway configuration.
type Config struct {
	SocketPath     string
	Workspace      string
	PluginID       string
	TokenSHA256    [32]byte
	LSPTokenSHA256 [32]byte
	Permissions    []string
	ExecRunner     string
	NoSandboxLSP   bool
	ExecTimeout    time.Duration
	MaxOutput      int
	Fetcher        *fetch.Client
	CgroupRoot     string
}

// Server accepts framed JSON-RPC connections on a Unix socket.
type Server struct {
	cfg          Config
	listener     net.Listener
	root         *fsroot.Root
	tokenMu      sync.Mutex
	tokenUsed    bool
	lspTokenUsed bool
	cgroup       string
}

// Listen binds the gateway socket with mode 0600.
func Listen(cfg Config) (*Server, error) {
	if cfg.SocketPath == "" {
		return nil, errors.New("gateway socket path is required")
	}
	if cfg.PluginID == "" {
		return nil, errors.New("gateway plugin id is required")
	}
	if cfg.ExecTimeout <= 0 {
		cfg.ExecTimeout = 5 * time.Second
	}
	if cfg.MaxOutput <= 0 {
		cfg.MaxOutput = execrunner.DefaultMaxOutput
	}
	if cfg.Permissions == nil {
		cfg.Permissions = []string{}
	}
	root, err := fsroot.Open(cfg.Workspace)
	if err != nil {
		return nil, fmt.Errorf("open workspace: %w", err)
	}
	listener, err := net.Listen("unix", cfg.SocketPath)
	if err != nil {
		_ = root.Close()
		return nil, fmt.Errorf("listen gateway socket: %w", err)
	}
	if err := os.Chmod(cfg.SocketPath, 0o600); err != nil {
		_ = listener.Close()
		_ = root.Close()
		_ = os.Remove(cfg.SocketPath)
		return nil, fmt.Errorf("chmod gateway socket: %w", err)
	}
	var cgroupPath string
	if cfg.CgroupRoot != "" {
		_ = execrunner.CleanupStaleCgroups(cfg.CgroupRoot, time.Hour)
		path, err := os.MkdirTemp(cfg.CgroupRoot, "anvil-exec-")
		if err != nil {
			_ = listener.Close()
			_ = root.Close()
			_ = os.Remove(cfg.SocketPath)
			return nil, fmt.Errorf("create session cgroup: %w", err)
		}
		cgroupPath = path
	}
	return &Server{cfg: cfg, listener: listener, root: root, cgroup: cgroupPath}, nil
}

// Addr returns the bound Unix socket path.
func (s *Server) Addr() string {
	return s.cfg.SocketPath
}

func (s *Server) Close() error {
	err := s.listener.Close()
	_ = os.Remove(s.cfg.SocketPath)
	if s.root != nil {
		_ = s.root.Close()
	}
	if s.cgroup != "" {
		_ = os.Remove(s.cgroup)
	}
	return err
}

// Serve accepts connections until the context is cancelled or the listener is
// closed.
func (s *Server) Serve(ctx context.Context) error {
	if ctx == nil {
		ctx = context.Background()
	}
	done := make(chan struct{})
	defer close(done)
	go func() {
		select {
		case <-ctx.Done():
			_ = s.Close()
		case <-done:
		}
	}()
	for {
		conn, err := s.listener.Accept()
		if err != nil {
			if ctx.Err() != nil || errors.Is(err, net.ErrClosed) {
				return nil
			}
			return err
		}
		go s.handle(ctx, conn)
	}
}

type request struct {
	JSONRPC string          `json:"jsonrpc"`
	ID      *string         `json:"id"`
	Method  string          `json:"method"`
	Params  json.RawMessage `json:"params"`
	Meta    json.RawMessage `json:"meta"`
}

type response struct {
	JSONRPC string      `json:"jsonrpc"`
	ID      *string     `json:"id"`
	Result  interface{} `json:"result,omitempty"`
	Error   *rpcError   `json:"error,omitempty"`
}

type rpcError struct {
	Code    int         `json:"code"`
	Message string      `json:"message"`
	Data    interface{} `json:"data,omitempty"`
}

type helloParams struct {
	Token      string `json:"token"`
	APIVersion string `json:"api_version"`
	Protocol   string `json:"protocol"`
}

type execParams struct {
	Tool      string   `json:"tool"`
	Args      []string `json:"args"`
	TimeoutMS uint64   `json:"timeout_ms"`
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

type readParams struct {
	Path     string `json:"path"`
	Offset   int64  `json:"offset"`
	MaxBytes int64  `json:"max_bytes"`
	Binary   bool   `json:"binary"`
	Seq      uint64 `json:"seq"`
}

type binaryResult struct {
	payload []byte
}

type readResult struct {
	Data       string `json:"data"`
	NextOffset int64  `json:"next_offset"`
	EOF        bool   `json:"eof"`
}

type statParams struct {
	Path string `json:"path"`
}

type statResult struct {
	Size int64 `json:"size"`
}

type writeParams struct {
	Path    string `json:"path"`
	Offset  int64  `json:"offset"`
	Data    string `json:"data"`
	EOF     *bool  `json:"eof"`
	Binary  bool   `json:"binary"`
	Seq     uint64 `json:"seq"`
	DataLen int    `json:"data_len"`
}

type binaryWriteResult struct {
	payload []byte
}

type writeFileResult struct {
	NextOffset int64 `json:"next_offset"`
}

type pendingWrite struct {
	target *fsroot.WriteTarget
	offset int64
}

type connectionState struct {
	mu          sync.Mutex
	handshaken  bool
	sessionID   string
	permissions []string
	writesMu    sync.Mutex
	writes      map[string]*pendingWrite
	conn        net.Conn
	writeMu     *sync.Mutex
	lspMu       sync.Mutex
	lsp         *lsp.Manager
}

func (state *connectionState) stopLSP() {
	state.lspMu.Lock()
	manager := state.lsp
	state.lsp = nil
	state.lspMu.Unlock()
	if manager != nil {
		_ = manager.Close()
	}
}

func (state *connectionState) abortWrites() {
	state.writesMu.Lock()
	defer state.writesMu.Unlock()
	for path, pending := range state.writes {
		pending.target.Abort()
		delete(state.writes, path)
	}
}

func (s *Server) handle(serverCtx context.Context, conn net.Conn) {
	defer conn.Close()
	uid, err := peerUID(conn)
	if err != nil || uid != uint32(os.Getuid()) {
		return
	}
	ctx, cancel := context.WithCancel(serverCtx)
	defer cancel()
	var writeMu sync.Mutex
	state := &connectionState{
		permissions: append([]string{}, s.cfg.Permissions...),
		writes:      map[string]*pendingWrite{},
		conn:        conn,
		writeMu:     &writeMu,
	}
	defer state.abortWrites()
	defer state.stopLSP()
	var requests sync.WaitGroup
	for {
		body, err := readFrame(conn)
		if err != nil {
			cancel()
			break
		}
		var req request
		if err := json.Unmarshal(body, &req); err != nil {
			writeMu.Lock()
			_ = writeError(conn, nil, -32700, "parse error")
			writeMu.Unlock()
			cancel()
			break
		}
		if req.ID == nil {
			continue
		}
		if req.JSONRPC != "2.0" || req.Method == "" {
			writeMu.Lock()
			_ = writeError(conn, req.ID, -32600, "invalid request")
			writeMu.Unlock()
			continue
		}
		if req.Method == "sys.write_file" {
			var params writeParams
			if json.Unmarshal(req.Params, &params) == nil && params.Binary {
				data, err := readFrame(conn)
				if err != nil {
					cancel()
					break
				}
				requests.Add(1)
				go func(req request, data []byte) {
					defer requests.Done()
					result, rpcErr := s.writeFileBinary(req, state, data)
					writeMu.Lock()
					defer writeMu.Unlock()
					if rpcErr != nil {
						_ = writeError(conn, req.ID, rpcErr.Code, rpcErr.Message)
						return
					}
					if binary, ok := result.(binaryWriteResult); ok {
						_ = writeFrame(conn, binary.payload)
						return
					}
				}(req, data)
				continue
			}
		}
		requests.Add(1)
		go func(req request) {
			defer requests.Done()
			result, rpcErr := s.dispatch(ctx, req, state)
			writeMu.Lock()
			defer writeMu.Unlock()
			if rpcErr != nil {
				_ = writeError(conn, req.ID, rpcErr.Code, rpcErr.Message)
				return
			}
			if binary, ok := result.(binaryResult); ok {
				_ = writeFrame(conn, binary.payload)
				return
			}
			_ = writeResult(conn, req.ID, result)
		}(req)
	}
	cancel()
	requests.Wait()
}

func (s *Server) dispatch(ctx context.Context, req request, state *connectionState) (interface{}, *rpcError) {
	switch req.Method {
	case "sys.read_file":
		return s.readFile(req, state)
	case "sys.stat":
		return s.statFile(req, state)
	case "sys.write_file":
		return s.writeFile(req, state)
	case "session.hello":
		return s.hello(req, state)
	case "sys.exec":
		return s.exec(ctx, req, state)
	case "sys.fetch":
		return s.fetch(ctx, req, state)
	case "lsp.start":
		return s.lspStart(ctx, req, state)
	case "lsp.send":
		return s.lspSend(req, state)
	case "lsp.stop":
		state.stopLSP()
		return map[string]bool{"stopped": true}, nil
	default:
		return nil, &rpcError{Code: -32601, Message: "method not found"}
	}
}

func (s *Server) hello(req request, state *connectionState) (interface{}, *rpcError) {
	state.mu.Lock()
	defer state.mu.Unlock()
	if state.handshaken {
		return nil, &rpcError{Code: -32600, Message: "session.hello already completed"}
	}
	var params helloParams
	if err := decodeParams(req.Params, &params); err != nil {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	if params.Protocol != Protocol || (params.APIVersion != APIVersion && params.APIVersion != "0.1") {
		return nil, &rpcError{Code: -32600, Message: "unsupported protocol"}
	}
	rawToken, err := base64.RawURLEncoding.DecodeString(strings.TrimRight(params.Token, "="))
	if err != nil || len(rawToken) != 32 {
		return nil, &rpcError{Code: -32001, Message: "unauthorized"}
	}
	hash := sha256.Sum256(rawToken)
	s.tokenMu.Lock()
	if subtle.ConstantTimeCompare(hash[:], s.cfg.TokenSHA256[:]) == 1 {
		if s.tokenUsed {
			s.tokenMu.Unlock()
			return nil, &rpcError{Code: -32001, Message: "unauthorized"}
		}
		s.tokenUsed = true
	} else if s.cfg.LSPTokenSHA256 != [32]byte{} && subtle.ConstantTimeCompare(hash[:], s.cfg.LSPTokenSHA256[:]) == 1 {
		if s.lspTokenUsed {
			s.tokenMu.Unlock()
			return nil, &rpcError{Code: -32001, Message: "unauthorized"}
		}
		s.lspTokenUsed = true
	} else {
		s.tokenMu.Unlock()
		return nil, &rpcError{Code: -32001, Message: "unauthorized"}
	}
	s.tokenMu.Unlock()
	state.handshaken = true
	state.sessionID = randomID()
	return map[string]interface{}{
		"session_id":  state.sessionID,
		"plugin_id":   s.cfg.PluginID,
		"api_version": APIVersion,
		"permissions": state.permissions,
	}, nil
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

func (s *Server) statFile(req request, state *connectionState) (interface{}, *rpcError) {
	var params statParams
	if err := decodeParams(req.Params, &params); err != nil {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	path, err := workspacePath(params.Path)
	if err != nil {
		return nil, &rpcError{Code: -32002, Message: "policy denied"}
	}
	if rpcErr := s.pathPermissionError(state, "fs.read", path); rpcErr != nil {
		return nil, rpcErr
	}
	if s.root == nil {
		return nil, &rpcError{Code: -32008, Message: "file service unavailable"}
	}
	size, err := s.root.Size(path)
	if err != nil {
		code := -32002
		if errors.Is(err, os.ErrNotExist) {
			code = -32007
		}
		return nil, &rpcError{Code: code, Message: "stat unavailable"}
	}
	return statResult{Size: size}, nil
}

func (s *Server) readFile(req request, state *connectionState) (interface{}, *rpcError) {
	var params readParams
	if err := decodeParams(req.Params, &params); err != nil {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	if params.Offset < 0 {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	path, err := workspacePath(params.Path)
	if err != nil {
		return nil, &rpcError{Code: -32002, Message: "policy denied"}
	}
	limit := int64(maxReadChunk)
	if params.Binary {
		limit = maxBinaryReadChunk
	}
	max := params.MaxBytes
	if max == 0 {
		max = limit
	}
	if max < 0 || max > limit {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	if rpcErr := s.pathPermissionError(state, "fs.read", path); rpcErr != nil {
		return nil, rpcErr
	}
	if s.root == nil {
		return nil, &rpcError{Code: -32008, Message: "file service unavailable"}
	}
	data, eof, err := s.root.ReadFileAt(path, params.Offset, max)
	if err != nil {
		code := -32002
		if errors.Is(err, fsroot.ErrOffsetOutOfRange) {
			code = -32602
		} else if errors.Is(err, os.ErrNotExist) {
			code = -32007
		}
		return nil, &rpcError{Code: code, Message: "read unavailable"}
	}
	next := params.Offset + int64(len(data))
	if params.Binary {
		payload := make([]byte, 17+len(data))
		if eof {
			payload[0] = 1
		}
		binary.BigEndian.PutUint64(payload[1:9], params.Seq)
		binary.BigEndian.PutUint64(payload[9:17], uint64(params.Offset))
		copy(payload[17:], data)
		return binaryResult{payload: payload}, nil
	}
	return readResult{
		Data:       base64.StdEncoding.EncodeToString(data),
		NextOffset: next,
		EOF:        eof,
	}, nil
}

func (s *Server) writeFileBinary(req request, state *connectionState, data []byte) (interface{}, *rpcError) {
	var params writeParams
	if err := decodeParams(req.Params, &params); err != nil {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	if params.Offset < 0 || params.EOF == nil || !params.Binary || len(data) > maxBinaryWriteChunk || len(data) != params.DataLen {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	if len(data) == 0 && !*params.EOF {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	path, err := workspacePath(params.Path)
	if err != nil {
		return nil, &rpcError{Code: -32002, Message: "policy denied"}
	}
	if rpcErr := s.pathPermissionError(state, "fs.write", path); rpcErr != nil {
		return nil, rpcErr
	}
	if s.root == nil {
		return nil, &rpcError{Code: -32008, Message: "file service unavailable"}
	}
	state.writesMu.Lock()
	defer state.writesMu.Unlock()
	pending := state.writes[path]
	if pending == nil {
		if params.Offset != 0 {
			return nil, &rpcError{Code: -32009, Message: "write offset conflict"}
		}
		target, err := s.root.CreateWrite(path)
		if err != nil {
			code := -32008
			if errors.Is(err, fsroot.ErrInvalidPath) {
				code = -32002
			} else if errors.Is(err, os.ErrNotExist) {
				code = -32007
			}
			return nil, &rpcError{Code: code, Message: "write unavailable"}
		}
		pending = &pendingWrite{target: target}
		state.writes[path] = pending
	}
	if params.Offset != pending.offset {
		return nil, &rpcError{Code: -32009, Message: "write offset conflict"}
	}
	if _, err := pending.target.Write(data); err != nil {
		pending.target.Abort()
		delete(state.writes, path)
		return nil, &rpcError{Code: -32008, Message: "write unavailable"}
	}
	pending.offset += int64(len(data))
	if *params.EOF {
		if err := pending.target.Commit(); err != nil {
			delete(state.writes, path)
			return nil, &rpcError{Code: -32008, Message: "write unavailable"}
		}
		delete(state.writes, path)
	}
	payload := make([]byte, 17)
	if *params.EOF {
		payload[0] = 1
	}
	binary.BigEndian.PutUint64(payload[1:9], params.Seq)
	binary.BigEndian.PutUint64(payload[9:17], uint64(pending.offset))
	return binaryWriteResult{payload: payload}, nil
}

func (s *Server) writeFile(req request, state *connectionState) (interface{}, *rpcError) {
	var params writeParams
	if err := decodeParams(req.Params, &params); err != nil {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	if params.Offset < 0 || params.EOF == nil {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	path, err := workspacePath(params.Path)
	if err != nil {
		return nil, &rpcError{Code: -32002, Message: "policy denied"}
	}
	data, err := base64.StdEncoding.DecodeString(params.Data)
	if err != nil || len(data) > maxWriteChunk {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	if len(data) == 0 && !*params.EOF {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	if rpcErr := s.pathPermissionError(state, "fs.write", path); rpcErr != nil {
		return nil, rpcErr
	}
	if s.root == nil {
		return nil, &rpcError{Code: -32008, Message: "file service unavailable"}
	}

	state.writesMu.Lock()
	defer state.writesMu.Unlock()
	pending := state.writes[path]
	if pending == nil {
		if params.Offset != 0 {
			return nil, &rpcError{Code: -32009, Message: "write offset conflict"}
		}
		target, err := s.root.CreateWrite(path)
		if err != nil {
			code := -32008
			if errors.Is(err, fsroot.ErrInvalidPath) {
				code = -32002
			} else if errors.Is(err, os.ErrNotExist) {
				code = -32007
			}
			return nil, &rpcError{Code: code, Message: "write unavailable"}
		}
		pending = &pendingWrite{target: target}
		state.writes[path] = pending
	}
	if params.Offset != pending.offset {
		return nil, &rpcError{Code: -32009, Message: "write offset conflict"}
	}
	if _, err := pending.target.Write(data); err != nil {
		pending.target.Abort()
		delete(state.writes, path)
		return nil, &rpcError{Code: -32008, Message: "write unavailable"}
	}
	pending.offset += int64(len(data))
	if *params.EOF {
		if err := pending.target.Commit(); err != nil {
			delete(state.writes, path)
			return nil, &rpcError{Code: -32008, Message: "write unavailable"}
		}
		delete(state.writes, path)
	}
	return writeFileResult{NextOffset: pending.offset}, nil
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
		Dir:       s.cfg.Workspace,
		Timeout:   timeout,
		MaxOutput: s.cfg.MaxOutput,
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
		env := []string{
			"HOME=" + home,
			"RUSTUP_HOME=" + filepath.Join(home, ".rustup"),
			"CARGO_HOME=" + filepath.Join(home, ".cargo"),
			"PATH=" + filepath.Join(home, ".cargo", "bin") + ":/usr/bin:/bin",
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

func (s *Server) permissionError(state *connectionState, wanted string) *rpcError {
	state.mu.Lock()
	defer state.mu.Unlock()
	if !state.handshaken {
		return &rpcError{Code: -32003, Message: "session required"}
	}
	for _, permission := range state.permissions {
		if permission == wanted {
			return nil
		}
	}
	return &rpcError{Code: -32002, Message: "policy denied"}
}

func (s *Server) pathPermissionError(state *connectionState, base, path string) *rpcError {
	state.mu.Lock()
	defer state.mu.Unlock()
	if !state.handshaken {
		return &rpcError{Code: -32003, Message: "session required"}
	}
	for _, permission := range state.permissions {
		if permission == base {
			return nil
		}
		pattern, ok := strings.CutPrefix(permission, base+":")
		if ok && matchPath(pattern, path) {
			return nil
		}
	}
	return &rpcError{Code: -32002, Message: "policy denied"}
}

func matchPath(pattern, path string) bool {
	if pattern == "*" || pattern == "**" {
		return true
	}
	var expression strings.Builder
	expression.WriteByte('^')
	for index := 0; index < len(pattern); index++ {
		switch pattern[index] {
		case '*':
			if index+1 < len(pattern) && pattern[index+1] == '*' {
				expression.WriteString(".*")
				index++
			} else {
				expression.WriteString("[^/]*")
			}
		case '?':
			expression.WriteString("[^/]")
		default:
			expression.WriteString(regexp.QuoteMeta(string(pattern[index])))
		}
	}
	expression.WriteByte('$')
	compiled, err := regexp.Compile(expression.String())
	return err == nil && compiled.MatchString(path)
}

func decodeParams(raw json.RawMessage, target interface{}) error {
	if len(raw) == 0 || string(raw) == "null" {
		return errors.New("params required")
	}
	return json.Unmarshal(raw, target)
}

func readFrame(reader io.Reader) ([]byte, error) {
	var header [4]byte
	if _, err := io.ReadFull(reader, header[:]); err != nil {
		return nil, err
	}
	length := int(header[0])<<24 | int(header[1])<<16 | int(header[2])<<8 | int(header[3])
	if length == 0 || length > MaxFrameLen {
		return nil, errors.New("invalid frame length")
	}
	body := make([]byte, length)
	if _, err := io.ReadFull(reader, body); err != nil {
		return nil, err
	}
	return body, nil
}

func writeFrame(writer io.Writer, body []byte) error {
	if len(body) == 0 || len(body) > MaxFrameLen {
		return errors.New("invalid frame length")
	}
	header := []byte{byte(len(body) >> 24), byte(len(body) >> 16), byte(len(body) >> 8), byte(len(body))}
	if _, err := writer.Write(header); err != nil {
		return err
	}
	_, err := writer.Write(body)
	return err
}

func writeResult(writer io.Writer, id *string, result interface{}) error {
	body, err := json.Marshal(response{JSONRPC: "2.0", ID: id, Result: result})
	if err != nil {
		return err
	}
	return writeFrame(writer, body)
}

func writeError(writer io.Writer, id *string, code int, message string) error {
	body, err := json.Marshal(response{JSONRPC: "2.0", ID: id, Error: &rpcError{Code: code, Message: message}})
	if err != nil {
		return err
	}
	return writeFrame(writer, body)
}

func peerUID(conn net.Conn) (uint32, error) {
	unixConn, ok := conn.(*net.UnixConn)
	if !ok {
		return 0, errors.New("not a unix connection")
	}
	raw, err := unixConn.SyscallConn()
	if err != nil {
		return 0, err
	}
	var credentials *unix.Ucred
	var credentialErr error
	if err := raw.Control(func(fd uintptr) {
		credentials, credentialErr = unix.GetsockoptUcred(int(fd), unix.SOL_SOCKET, unix.SO_PEERCRED)
	}); err != nil {
		return 0, err
	}
	if credentialErr != nil {
		return 0, credentialErr
	}
	return credentials.Uid, nil
}

func randomID() string {
	var buffer [16]byte
	if _, err := rand.Read(buffer[:]); err != nil {
		return "session"
	}
	return hex.EncodeToString(buffer[:])
}
