// Package gateway implements the per-session Unix-socket policy gateway.
package gateway

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"sync"
	"time"

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
	APIVersion = "1.0.0"
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

type pluginGrant struct {
	ID          string
	TokenSHA256 [32]byte
	Permissions []string
	Handshaken  bool
}

// Server accepts framed JSON-RPC connections on a Unix socket.
type Server struct {
	cfg       Config
	listener  net.Listener
	root      *fsroot.Root
	tokenMu   sync.Mutex
	tokenUsed bool
	pluginMu  sync.Mutex
	plugins   map[string]pluginGrant
	cgroup    string
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
	return &Server{
		cfg:      cfg,
		listener: listener,
		root:     root,
		plugins:  map[string]pluginGrant{},
		cgroup:   cgroupPath,
	}, nil
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

type connectionState struct {
	mu          sync.Mutex
	handshaken  bool
	isEditor    bool
	pluginID    string
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

func (s *Server) effectivePermissions(state *connectionState) []string {
	if state.pluginID == "" {
		return state.permissions
	}
	s.pluginMu.Lock()
	defer s.pluginMu.Unlock()
	grant, ok := s.plugins[state.pluginID]
	if !ok {
		return nil
	}
	return grant.Permissions
}

func (s *Server) permissionError(state *connectionState, wanted string) *rpcError {
	state.mu.Lock()
	defer state.mu.Unlock()
	if !state.handshaken {
		return &rpcError{Code: -32003, Message: "session required"}
	}
	permissions := s.effectivePermissions(state)
	for _, permission := range permissions {
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
	canonical, err := s.canonicalPermissionPath(path)
	if err != nil {
		return &rpcError{Code: -32002, Message: "policy denied"}
	}
	permissions := s.effectivePermissions(state)
	for _, permission := range permissions {
		if permission == base {
			return nil
		}
		pattern, ok := strings.CutPrefix(permission, base+":")
		// Require both the requested path and the resolved symlink target to
		// match. Otherwise a link inside a granted subtree could expose a file
		// outside that subtree.
		if ok && matchPath(pattern, path) && matchPath(pattern, canonical) {
			return nil
		}
	}
	return &rpcError{Code: -32002, Message: "policy denied"}
}

func (s *Server) canonicalPermissionPath(path string) (string, error) {
	workspace, err := filepath.EvalSymlinks(s.cfg.Workspace)
	if err != nil {
		return "", err
	}
	full := filepath.Join(workspace, filepath.FromSlash(path))
	resolved, err := filepath.EvalSymlinks(full)
	if err != nil {
		if !errors.Is(err, os.ErrNotExist) {
			return "", err
		}
		parent, parentErr := filepath.EvalSymlinks(filepath.Dir(full))
		if parentErr != nil {
			return "", parentErr
		}
		resolved = filepath.Join(parent, filepath.Base(full))
	}
	relative, err := filepath.Rel(workspace, resolved)
	if err != nil || relative == ".." || strings.HasPrefix(relative, ".."+string(os.PathSeparator)) {
		return "", fsroot.ErrInvalidPath
	}
	return filepath.ToSlash(relative), nil
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
