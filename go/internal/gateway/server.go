// Package gateway implements the per-session Unix-socket policy gateway.
package gateway

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"os"
	"strings"
	"sync"
	"time"

	"github.com/GBLMX/Anvil/go/internal/execpolicy"
	"github.com/GBLMX/Anvil/go/internal/execrunner"
	"golang.org/x/sys/unix"
)

const (
	// MaxFrameLen is the largest accepted JSON-RPC frame.
	MaxFrameLen = 8 << 20
	// Protocol is the v1 JSON-RPC protocol name.
	Protocol = "anvil-jsonrpc-1"
	// APIVersion is the public API version.
	APIVersion = "0.1.0"
)

// Config is the minimum per-session gateway configuration.
type Config struct {
	SocketPath  string
	Workspace   string
	PluginID    string
	TokenSHA256 [32]byte
	Permissions []string
	ExecRunner  string
	ExecTimeout time.Duration
	MaxOutput   int
}

// Server accepts framed JSON-RPC connections on a Unix socket.
type Server struct {
	cfg      Config
	listener net.Listener
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
	listener, err := net.Listen("unix", cfg.SocketPath)
	if err != nil {
		return nil, fmt.Errorf("listen gateway socket: %w", err)
	}
	if err := os.Chmod(cfg.SocketPath, 0o600); err != nil {
		_ = listener.Close()
		_ = os.Remove(cfg.SocketPath)
		return nil, fmt.Errorf("chmod gateway socket: %w", err)
	}
	return &Server{cfg: cfg, listener: listener}, nil
}

// Addr returns the bound Unix socket path.
func (s *Server) Addr() string {
	return s.cfg.SocketPath
}

// Close closes the listener.
func (s *Server) Close() error {
	err := s.listener.Close()
	_ = os.Remove(s.cfg.SocketPath)
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
		go s.handle(conn)
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

type execResult struct {
	Stdout          string `json:"stdout"`
	Stderr          string `json:"stderr"`
	ExitCode        int    `json:"exit_code"`
	TimedOut        bool   `json:"timed_out"`
	OutputTruncated bool   `json:"output_truncated"`
}

type connectionState struct {
	mu          sync.Mutex
	handshaken  bool
	sessionID   string
	permissions []string
}

func (s *Server) handle(conn net.Conn) {
	defer conn.Close()
	uid, err := peerUID(conn)
	if err != nil || uid != uint32(os.Getuid()) {
		return
	}
	state := &connectionState{
		permissions: append([]string{}, s.cfg.Permissions...),
	}
	for {
		body, err := readFrame(conn)
		if err != nil {
			return
		}
		var req request
		if err := json.Unmarshal(body, &req); err != nil {
			_ = writeError(conn, nil, -32700, "parse error")
			return
		}
		if req.ID == nil {
			continue
		}
		if req.JSONRPC != "2.0" || req.Method == "" {
			_ = writeError(conn, req.ID, -32600, "invalid request")
			continue
		}
		result, rpcErr := s.dispatch(req, state)
		if rpcErr != nil {
			if err := writeError(conn, req.ID, rpcErr.Code, rpcErr.Message); err != nil {
				return
			}
			continue
		}
		if err := writeResult(conn, req.ID, result); err != nil {
			return
		}
	}
}

func (s *Server) dispatch(req request, state *connectionState) (interface{}, *rpcError) {
	switch req.Method {
	case "session.hello":
		return s.hello(req, state)
	case "sys.exec":
		return s.exec(req, state)
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
	if subtle.ConstantTimeCompare(hash[:], s.cfg.TokenSHA256[:]) != 1 {
		return nil, &rpcError{Code: -32001, Message: "unauthorized"}
	}
	state.handshaken = true
	state.sessionID = randomID()
	return map[string]interface{}{
		"session_id":  state.sessionID,
		"plugin_id":   s.cfg.PluginID,
		"api_version": APIVersion,
		"permissions": state.permissions,
	}, nil
}

func (s *Server) exec(req request, state *connectionState) (interface{}, *rpcError) {
	state.mu.Lock()
	if !state.handshaken {
		state.mu.Unlock()
		return nil, &rpcError{Code: -32003, Message: "session required"}
	}
	allowed := false
	for _, permission := range state.permissions {
		if permission == "sys.exec" {
			allowed = true
			break
		}
	}
	state.mu.Unlock()
	if !allowed {
		return nil, &rpcError{Code: -32002, Message: "policy denied"}
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
	ctx, cancel := context.WithTimeout(context.Background(), timeout)
	defer cancel()
	args := []string{"-workspace", s.cfg.Workspace, "-tool", params.Tool, "--"}
	args = append(args, params.Args...)
	result, err := execrunner.Run(ctx, execrunner.ProcessConfig{
		Path:      s.cfg.ExecRunner,
		Args:      args,
		Env:       []string{},
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
