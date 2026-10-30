package gateway

import (
	"context"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"net"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
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
