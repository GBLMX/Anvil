package gateway

import (
	"context"
	"crypto/sha256"
	"encoding/base64"
	"encoding/binary"
	"encoding/json"
	"net"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/GBLMX/Anvil/go/internal/fetch"
)

const testToken = "0123456789abcdef0123456789abcdef"

func startGateway(t *testing.T, permissions []string, runner string) string {
	t.Helper()
	socket := filepath.Join(t.TempDir(), "gateway.sock")
	hash := sha256.Sum256([]byte(testToken))
	server, err := Listen(Config{
		SocketPath:  socket,
		Workspace:   t.TempDir(),
		PluginID:    "test-plugin",
		TokenSHA256: hash,
		Permissions: permissions,
		ExecRunner:  runner,
		ExecTimeout: time.Second,
		MaxOutput:   4096,
	})
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan struct{})
	go func() {
		defer close(done)
		_ = server.Serve(ctx)
	}()
	t.Cleanup(func() {
		cancel()
		_ = server.Close()
		<-done
		_ = os.Remove(socket)
	})
	return socket
}

func startGatewayAt(t *testing.T, workspace string, permissions []string) string {
	t.Helper()
	socket := filepath.Join(t.TempDir(), "gateway.sock")
	hash := sha256.Sum256([]byte(testToken))
	server, err := Listen(Config{
		SocketPath:  socket,
		Workspace:   workspace,
		PluginID:    "test-plugin",
		TokenSHA256: hash,
		Permissions: permissions,
	})
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan struct{})
	go func() {
		defer close(done)
		_ = server.Serve(ctx)
	}()
	t.Cleanup(func() {
		cancel()
		_ = server.Close()
		<-done
		_ = os.Remove(socket)
	})
	return socket
}

func dialGateway(t *testing.T, socket string) net.Conn {
	t.Helper()
	conn, err := net.Dial("unix", socket)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = conn.Close() })
	return conn
}

func sendRequest(t *testing.T, conn net.Conn, id, method string, params interface{}) response {
	t.Helper()
	body, err := json.Marshal(map[string]interface{}{
		"jsonrpc": "2.0",
		"id":      id,
		"method":  method,
		"params":  params,
	})
	if err != nil {
		t.Fatal(err)
	}
	if err := writeFrame(conn, body); err != nil {
		t.Fatal(err)
	}
	return receiveResponse(t, conn)
}

func receiveResponse(t *testing.T, conn net.Conn) response {
	t.Helper()
	body, err := readFrame(conn)
	if err != nil {
		t.Fatal(err)
	}
	var response response
	if err := json.Unmarshal(body, &response); err != nil {
		t.Fatal(err)
	}
	return response
}

func helloRequestParams(token string) map[string]interface{} {
	return map[string]interface{}{
		"token":       base64.RawURLEncoding.EncodeToString([]byte(token)),
		"api_version": APIVersion,
		"protocol":    Protocol,
	}
}

func TestHandshake(t *testing.T) {
	socket := startGateway(t, []string{"sys.exec"}, "")
	conn := dialGateway(t, socket)
	response := sendRequest(t, conn, "hello-1", "session.hello", helloRequestParams(testToken))
	if response.Error != nil {
		t.Fatalf("handshake error: %+v", response.Error)
	}
	result, ok := response.Result.(map[string]interface{})
	if !ok {
		t.Fatalf("result = %#v", response.Result)
	}
	if result["plugin_id"] != "test-plugin" {
		t.Fatalf("plugin_id = %v", result["plugin_id"])
	}
	if result["api_version"] != APIVersion {
		t.Fatalf("api_version = %v", result["api_version"])
	}
}

func TestSessionCgroupLifecycle(t *testing.T) {
	root := t.TempDir()
	socket := filepath.Join(t.TempDir(), "gateway.sock")
	hash := sha256.Sum256([]byte(testToken))
	server, err := Listen(Config{
		SocketPath:  socket,
		Workspace:   t.TempDir(),
		PluginID:    "test-plugin",
		TokenSHA256: hash,
		CgroupRoot:  root,
	})
	if err != nil {
		t.Fatal(err)
	}
	entries, err := os.ReadDir(root)
	if err != nil {
		t.Fatal(err)
	}
	if len(entries) != 1 {
		t.Fatalf("cgroup entries = %d", len(entries))
	}
	if err := server.Close(); err != nil {
		t.Fatal(err)
	}
	entries, err = os.ReadDir(root)
	if err != nil {
		t.Fatal(err)
	}
	if len(entries) != 0 {
		t.Fatalf("cgroup cleanup left %d entries", len(entries))
	}
}

func TestSysFetch(t *testing.T) {
	httpServer := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, _ *http.Request) {
		_, _ = writer.Write([]byte("fetched"))
	}))
	defer httpServer.Close()
	parsed, err := url.Parse(httpServer.URL)
	if err != nil {
		t.Fatal(err)
	}
	fetcher, err := fetch.New(fetch.Config{
		AllowedHosts:  []string{"127.0.0.1"},
		MaxBody:       1024,
		AllowHTTP:     true,
		AllowLoopback: true,
		AllowAnyPort:  true,
	})
	if err != nil {
		t.Fatal(err)
	}
	socket := filepath.Join(t.TempDir(), "gateway.sock")
	hash := sha256.Sum256([]byte(testToken))
	server, err := Listen(Config{
		SocketPath:  socket,
		Workspace:   t.TempDir(),
		PluginID:    "test-plugin",
		TokenSHA256: hash,
		Permissions: []string{"sys.fetch"},
		Fetcher:     fetcher,
	})
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan struct{})
	go func() {
		defer close(done)
		_ = server.Serve(ctx)
	}()
	t.Cleanup(func() {
		cancel()
		_ = server.Close()
		<-done
	})
	conn := dialGateway(t, socket)
	if response := sendRequest(t, conn, "hello-1", "session.hello", helloRequestParams(testToken)); response.Error != nil {
		t.Fatalf("handshake error: %+v", response.Error)
	}
	response := sendRequest(t, conn, "fetch-1", "sys.fetch", map[string]interface{}{
		"url": httpServer.URL + "/",
	})
	if response.Error != nil {
		t.Fatalf("fetch error: %+v", response.Error)
	}
	result, ok := response.Result.(map[string]interface{})
	if !ok || result["body"] != "fetched" || result["status"] != float64(200) {
		t.Fatalf("result = %#v on port %s", response.Result, parsed.Port())
	}
}

func TestSysExecRequiresHandshake(t *testing.T) {
	socket := startGateway(t, []string{"sys.exec"}, "")
	conn := dialGateway(t, socket)
	response := sendRequest(t, conn, "exec-1", "sys.exec", map[string]interface{}{
		"tool": "/usr/bin/rg",
		"args": []string{"-n", "foo", "README.md"},
	})
	if response.Error == nil || response.Error.Code != -32003 {
		t.Fatalf("error = %+v", response.Error)
	}
}

func decodedData(t *testing.T, result map[string]interface{}) string {
	t.Helper()
	encoded, ok := result["data"].(string)
	if !ok {
		t.Fatalf("data = %#v", result["data"])
	}
	data, err := base64.StdEncoding.DecodeString(encoded)
	if err != nil {
		t.Fatal(err)
	}
	return string(data)
}

func TestSysReadFileChunks(t *testing.T) {
	workspace := t.TempDir()
	if err := os.WriteFile(filepath.Join(workspace, "file.txt"), []byte("0123456789"), 0o600); err != nil {
		t.Fatal(err)
	}
	socket := startGatewayAt(t, workspace, []string{"fs.read:**"})
	conn := dialGateway(t, socket)
	if response := sendRequest(t, conn, "hello-1", "session.hello", helloRequestParams(testToken)); response.Error != nil {
		t.Fatalf("handshake error: %+v", response.Error)
	}

	response := sendRequest(t, conn, "read-1", "sys.read_file", map[string]interface{}{
		"path":      "file.txt",
		"offset":    0,
		"max_bytes": 4,
	})
	if response.Error != nil {
		t.Fatalf("read error: %+v", response.Error)
	}
	first, ok := response.Result.(map[string]interface{})
	if !ok {
		t.Fatalf("first chunk = %#v", response.Result)
	}
	if decodedData(t, first) != "0123" || first["next_offset"] != float64(4) || first["eof"] != false {
		t.Fatalf("first chunk = %#v", response.Result)
	}

	response = sendRequest(t, conn, "read-2", "sys.read_file", map[string]interface{}{
		"path":      "file.txt",
		"offset":    8,
		"max_bytes": 4,
	})
	last, ok := response.Result.(map[string]interface{})
	if response.Error != nil || !ok {
		t.Fatalf("last chunk = %#v, error=%+v", response.Result, response.Error)
	}
	if decodedData(t, last) != "89" || last["eof"] != true {
		t.Fatalf("last chunk = %#v, error=%+v", response.Result, response.Error)
	}
}

func TestSysReadFileRequiresPermission(t *testing.T) {
	workspace := t.TempDir()
	if err := os.WriteFile(filepath.Join(workspace, "file.txt"), []byte("data"), 0o600); err != nil {
		t.Fatal(err)
	}
	socket := startGatewayAt(t, workspace, []string{"sys.exec"})
	conn := dialGateway(t, socket)
	if response := sendRequest(t, conn, "hello-1", "session.hello", helloRequestParams(testToken)); response.Error != nil {
		t.Fatalf("handshake error: %+v", response.Error)
	}
	response := sendRequest(t, conn, "read-1", "sys.read_file", map[string]interface{}{
		"path":   "file.txt",
		"offset": 0,
	})
	if response.Error == nil || response.Error.Code != -32002 {
		t.Fatalf("error = %+v", response.Error)
	}
}

func TestHandshakeRejectsBadToken(t *testing.T) {
	socket := startGateway(t, []string{"sys.exec"}, "")
	conn := dialGateway(t, socket)
	response := sendRequest(t, conn, "hello-1", "session.hello", helloRequestParams("00000000000000000000000000000000"))
	if response.Error == nil || response.Error.Code != -32001 {
		t.Fatalf("error = %+v", response.Error)
	}
}

func TestSysExecRoutesThroughRunner(t *testing.T) {
	runner := filepath.Join(t.TempDir(), "runner")
	if err := os.WriteFile(runner, []byte("#!/bin/sh\nprintf 'runner:%s\\n' \"$*\"\n"), 0o700); err != nil {
		t.Fatal(err)
	}
	socket := startGateway(t, []string{"sys.exec"}, runner)
	conn := dialGateway(t, socket)
	if response := sendRequest(t, conn, "hello-1", "session.hello", helloRequestParams(testToken)); response.Error != nil {
		t.Fatalf("handshake error: %+v", response.Error)
	}
	response := sendRequest(t, conn, "exec-1", "sys.exec", map[string]interface{}{
		"tool": "/usr/bin/rg",
		"args": []string{"-n", "foo", "README.md"},
	})
	if response.Error != nil {
		t.Fatalf("exec error: %+v", response.Error)
	}
	result, ok := response.Result.(map[string]interface{})
	if !ok {
		t.Fatalf("result = %#v", response.Result)
	}
	stdout, ok := result["stdout"].(string)
	if !ok || !strings.Contains(stdout, "runner:-workspace") || !strings.Contains(stdout, "-tool /usr/bin/rg") {
		t.Fatalf("stdout = %q", stdout)
	}
	if result["exit_code"] != float64(0) {
		t.Fatalf("exit_code = %v", result["exit_code"])
	}
}

func TestTokenIsOneShot(t *testing.T) {
	socket := startGateway(t, []string{"sys.exec"}, "")
	first := dialGateway(t, socket)
	if response := sendRequest(t, first, "hello-1", "session.hello", helloRequestParams(testToken)); response.Error != nil {
		t.Fatalf("first handshake: %+v", response.Error)
	}
	second := dialGateway(t, socket)
	response := sendRequest(t, second, "hello-2", "session.hello", helloRequestParams(testToken))
	if response.Error == nil || response.Error.Code != -32001 {
		t.Fatalf("second handshake error = %+v", response.Error)
	}
}

func TestConnectionCloseCancelsExec(t *testing.T) {
	marker := filepath.Join(t.TempDir(), "alive")
	runner := filepath.Join(t.TempDir(), "runner")
	script := "#!/bin/sh\nsleep 1\n: > " + marker + "\n"
	if err := os.WriteFile(runner, []byte(script), 0o700); err != nil {
		t.Fatal(err)
	}
	socket := startGateway(t, []string{"sys.exec"}, runner)
	conn := dialGateway(t, socket)
	if response := sendRequest(t, conn, "hello-1", "session.hello", helloRequestParams(testToken)); response.Error != nil {
		t.Fatalf("handshake error: %+v", response.Error)
	}
	body, err := json.Marshal(map[string]interface{}{
		"jsonrpc": "2.0",
		"id":      "exec-1",
		"method":  "sys.exec",
		"params": map[string]interface{}{
			"tool": "/usr/bin/rg",
			"args": []string{"-n", "foo", "README.md"},
		},
	})
	if err != nil {
		t.Fatal(err)
	}
	if err := writeFrame(conn, body); err != nil {
		t.Fatal(err)
	}
	_ = conn.Close()
	time.Sleep(1500 * time.Millisecond)
	if _, err := os.Stat(marker); err == nil {
		t.Fatal("exec process survived connection close")
	}
}

func TestSysWriteFileChunks(t *testing.T) {
	workspace := t.TempDir()
	socket := startGatewayAt(t, workspace, []string{"fs.write:**"})
	conn := dialGateway(t, socket)
	if response := sendRequest(t, conn, "hello-1", "session.hello", helloRequestParams(testToken)); response.Error != nil {
		t.Fatalf("handshake error: %+v", response.Error)
	}
	first := sendRequest(t, conn, "write-1", "sys.write_file", map[string]interface{}{
		"path":   "file.txt",
		"offset": 0,
		"data":   base64.StdEncoding.EncodeToString([]byte("hello ")),
		"eof":    false,
	})
	if first.Error != nil {
		t.Fatalf("first write error: %+v", first.Error)
	}
	result, ok := first.Result.(map[string]interface{})
	if !ok || result["next_offset"] != float64(6) {
		t.Fatalf("first result = %#v", first.Result)
	}
	second := sendRequest(t, conn, "write-2", "sys.write_file", map[string]interface{}{
		"path":   "file.txt",
		"offset": 6,
		"data":   base64.StdEncoding.EncodeToString([]byte("world")),
		"eof":    true,
	})
	if second.Error != nil {
		t.Fatalf("second write error: %+v", second.Error)
	}
	got, err := os.ReadFile(filepath.Join(workspace, "file.txt"))
	if err != nil {
		t.Fatal(err)
	}
	if string(got) != "hello world" {
		t.Fatalf("file = %q", got)
	}
	entries, err := os.ReadDir(workspace)
	if err != nil {
		t.Fatal(err)
	}
	for _, entry := range entries {
		if strings.HasPrefix(entry.Name(), ".anvil-write-") {
			t.Fatalf("temporary file left behind: %s", entry.Name())
		}
	}
}

func TestSysWriteFileRequiresPermission(t *testing.T) {
	socket := startGatewayAt(t, t.TempDir(), []string{"fs.read:**"})
	conn := dialGateway(t, socket)
	if response := sendRequest(t, conn, "hello-1", "session.hello", helloRequestParams(testToken)); response.Error != nil {
		t.Fatalf("handshake error: %+v", response.Error)
	}
	response := sendRequest(t, conn, "write-1", "sys.write_file", map[string]interface{}{
		"path":   "file.txt",
		"offset": 0,
		"data":   base64.StdEncoding.EncodeToString([]byte("data")),
		"eof":    true,
	})
	if response.Error == nil || response.Error.Code != -32002 {
		t.Fatalf("write error = %+v", response.Error)
	}
}

func TestSysWriteFileOffsetConflict(t *testing.T) {
	workspace := t.TempDir()
	socket := startGatewayAt(t, workspace, []string{"fs.write:**"})
	conn := dialGateway(t, socket)
	if response := sendRequest(t, conn, "hello-1", "session.hello", helloRequestParams(testToken)); response.Error != nil {
		t.Fatalf("handshake error: %+v", response.Error)
	}
	first := sendRequest(t, conn, "write-1", "sys.write_file", map[string]interface{}{
		"path":   "file.txt",
		"offset": 0,
		"data":   base64.StdEncoding.EncodeToString([]byte("abc")),
		"eof":    false,
	})
	if first.Error != nil {
		t.Fatalf("first write error: %+v", first.Error)
	}
	second := sendRequest(t, conn, "write-2", "sys.write_file", map[string]interface{}{
		"path":   "file.txt",
		"offset": 1,
		"data":   base64.StdEncoding.EncodeToString([]byte("x")),
		"eof":    true,
	})
	if second.Error == nil || second.Error.Code != -32009 {
		t.Fatalf("conflict error = %+v", second.Error)
	}
	if _, err := os.Stat(filepath.Join(workspace, "file.txt")); !os.IsNotExist(err) {
		t.Fatalf("conflicting write modified destination: %v", err)
	}
}

func TestReadPermissionUsesCanonicalPath(t *testing.T) {
	workspace := t.TempDir()
	if err := os.WriteFile(filepath.Join(workspace, "secret"), []byte("secret"), 0o600); err != nil {
		t.Fatal(err)
	}
	socket := startGatewayAt(t, workspace, []string{"fs.read:src/**"})
	conn := dialGateway(t, socket)
	if response := sendRequest(t, conn, "hello-1", "session.hello", helloRequestParams(testToken)); response.Error != nil {
		t.Fatalf("handshake error: %+v", response.Error)
	}
	response := sendRequest(t, conn, "read-1", "sys.read_file", map[string]interface{}{
		"path": "src/../secret",
	})
	if response.Error == nil || response.Error.Code != -32002 {
		t.Fatalf("read error = %+v", response.Error)
	}
}

func TestReadPermissionRejectsSymlinkEscalation(t *testing.T) {
	workspace := t.TempDir()
	if err := os.Mkdir(filepath.Join(workspace, "src"), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(workspace, "secret"), []byte("secret"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink("../secret", filepath.Join(workspace, "src", "link")); err != nil {
		t.Fatal(err)
	}
	socket := startGatewayAt(t, workspace, []string{"fs.read:src/**"})
	conn := dialGateway(t, socket)
	if response := sendRequest(t, conn, "hello-symlink", "session.hello", helloRequestParams(testToken)); response.Error != nil {
		t.Fatalf("handshake error: %+v", response.Error)
	}
	response := sendRequest(t, conn, "read-symlink", "sys.read_file", map[string]interface{}{
		"path": "src/link",
	})
	if response.Error == nil {
		t.Fatal("symlink outside the read grant was readable")
	}
}

func TestSysReadFileBinary(t *testing.T) {
	workspace := t.TempDir()
	data := []byte("binary-frame-data")
	if err := os.WriteFile(filepath.Join(workspace, "file.txt"), data, 0o600); err != nil {
		t.Fatal(err)
	}
	socket := startGatewayAt(t, workspace, []string{"fs.read:**"})
	conn := dialGateway(t, socket)
	if response := sendRequest(t, conn, "hello-1", "session.hello", helloRequestParams(testToken)); response.Error != nil {
		t.Fatalf("handshake error: %+v", response.Error)
	}
	body, err := json.Marshal(map[string]interface{}{
		"jsonrpc": "2.0",
		"id":      "read-binary",
		"method":  "sys.read_file",
		"params": map[string]interface{}{
			"path":   "file.txt",
			"offset": 0,
			"binary": true,
			"seq":    7,
		},
	})
	if err != nil {
		t.Fatal(err)
	}
	if err := writeFrame(conn, body); err != nil {
		t.Fatal(err)
	}
	frame, err := readFrame(conn)
	if err != nil {
		t.Fatal(err)
	}
	if len(frame) == 0 || frame[0] == '{' {
		t.Fatalf("expected binary frame, got %q", frame)
	}
	if len(frame) < 17 {
		t.Fatalf("binary frame too short: %d", len(frame))
	}
	flags := frame[0]
	if seq := binary.BigEndian.Uint64(frame[1:9]); seq != 7 {
		t.Fatalf("seq = %d", seq)
	}
	if offset := binary.BigEndian.Uint64(frame[9:17]); offset != 0 {
		t.Fatalf("offset = %d", offset)
	}
	got := frame[17:]
	if string(got) != string(data) {
		t.Fatalf("data = %q, want %q", got, data)
	}
	if flags&1 == 0 {
		t.Fatal("binary frame did not set eof")
	}
}

func TestSysWriteFileBinary(t *testing.T) {
	workspace := t.TempDir()
	socket := startGatewayAt(t, workspace, []string{"fs.write:**"})
	conn := dialGateway(t, socket)
	if response := sendRequest(t, conn, "hello-1", "session.hello", helloRequestParams(testToken)); response.Error != nil {
		t.Fatalf("handshake error: %+v", response.Error)
	}
	body, err := json.Marshal(map[string]interface{}{
		"jsonrpc": "2.0",
		"id":      "write-binary",
		"method":  "sys.write_file",
		"params": map[string]interface{}{
			"path":     "file.txt",
			"offset":   0,
			"eof":      true,
			"binary":   true,
			"seq":      3,
			"data_len": len("binary-write"),
		},
	})
	if err != nil {
		t.Fatal(err)
	}
	if err := writeFrame(conn, body); err != nil {
		t.Fatal(err)
	}
	if err := writeFrame(conn, []byte("binary-write")); err != nil {
		t.Fatal(err)
	}
	frame, err := readFrame(conn)
	if err != nil {
		t.Fatal(err)
	}
	if len(frame) != 17 {
		t.Fatalf("binary response length = %d", len(frame))
	}
	if binary.BigEndian.Uint64(frame[1:9]) != 3 {
		t.Fatalf("seq = %d", binary.BigEndian.Uint64(frame[1:9]))
	}
	if binary.BigEndian.Uint64(frame[9:17]) != uint64(len("binary-write")) {
		t.Fatalf("next = %d", binary.BigEndian.Uint64(frame[9:17]))
	}
	got, err := os.ReadFile(filepath.Join(workspace, "file.txt"))
	if err != nil {
		t.Fatal(err)
	}
	if string(got) != "binary-write" {
		t.Fatalf("file = %q", got)
	}
}

func TestSysReadDir(t *testing.T) {
	workspace := t.TempDir()
	if err := os.MkdirAll(filepath.Join(workspace, "src"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(workspace, "src", "main.rs"), []byte("fn main() {}"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(filepath.Join(workspace, ".git"), 0o755); err != nil {
		t.Fatal(err)
	}
	socket := startGatewayAt(t, workspace, []string{"fs.read:**"})
	conn := dialGateway(t, socket)
	if response := sendRequest(t, conn, "hello-1", "session.hello", helloRequestParams(testToken)); response.Error != nil {
		t.Fatalf("handshake error: %+v", response.Error)
	}
	response := sendRequest(t, conn, "read-dir-1", "sys.read_dir", map[string]interface{}{"path": "."})
	if response.Error != nil {
		t.Fatalf("read_dir error: %+v", response.Error)
	}
	result, ok := response.Result.(map[string]interface{})
	if !ok {
		t.Fatalf("result = %#v", response.Result)
	}
	entries, ok := result["entries"].([]interface{})
	if !ok || len(entries) != 1 {
		t.Fatalf("entries = %#v", result["entries"])
	}
	entry, ok := entries[0].(map[string]interface{})
	if !ok || entry["name"] != "src" || entry["is_dir"] != true {
		t.Fatalf("entry = %#v", entries[0])
	}
}

func TestSysCopyAndRemoveAll(t *testing.T) {
	workspace := t.TempDir()
	if err := os.WriteFile(filepath.Join(workspace, "source.txt"), []byte("copy"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(filepath.Join(workspace, "tree", "nested"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(workspace, "tree", "nested", "file.txt"), []byte("x"), 0o600); err != nil {
		t.Fatal(err)
	}
	socket := startGatewayAt(t, workspace, []string{"fs.read:**", "fs.write:**"})
	conn := dialGateway(t, socket)
	if response := sendRequest(t, conn, "hello-1", "session.hello", helloRequestParams(testToken)); response.Error != nil {
		t.Fatalf("handshake error: %+v", response.Error)
	}
	response := sendRequest(t, conn, "copy-1", "sys.copy", map[string]interface{}{
		"from": "source.txt",
		"to":   "copy.txt",
	})
	if response.Error != nil {
		t.Fatalf("copy error: %+v", response.Error)
	}
	got, err := os.ReadFile(filepath.Join(workspace, "copy.txt"))
	if err != nil || string(got) != "copy" {
		t.Fatalf("copy = %q, %v", got, err)
	}
	response = sendRequest(t, conn, "remove-1", "sys.remove_all", map[string]interface{}{
		"path": "tree",
	})
	if response.Error != nil {
		t.Fatalf("remove_all error: %+v", response.Error)
	}
	if _, err := os.Stat(filepath.Join(workspace, "tree")); !os.IsNotExist(err) {
		t.Fatalf("tree still exists: %v", err)
	}
}

func TestLSPTokenCanReconnect(t *testing.T) {
	workspace := t.TempDir()
	socket := filepath.Join(t.TempDir(), "gateway.sock")
	mainHash := sha256.Sum256([]byte(testToken))
	lspToken := "fedcba9876543210fedcba9876543210"
	lspHash := sha256.Sum256([]byte(lspToken))
	server, err := Listen(Config{
		SocketPath:     socket,
		Workspace:      workspace,
		PluginID:       "test-plugin",
		TokenSHA256:    mainHash,
		LSPTokenSHA256: lspHash,
		Permissions:    []string{"fs.read:**"},
	})
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan struct{})
	go func() {
		defer close(done)
		_ = server.Serve(ctx)
	}()
	t.Cleanup(func() {
		cancel()
		_ = server.Close()
		<-done
	})
	for index := 0; index < 2; index++ {
		conn := dialGateway(t, socket)
		if response := sendRequest(t, conn, "hello", "session.hello", helloRequestParams(lspToken)); response.Error != nil {
			t.Fatalf("lsp handshake %d: %+v", index, response.Error)
		}
		_ = conn.Close()
	}
}
