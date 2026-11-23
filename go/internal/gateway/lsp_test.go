package gateway

import (
	"context"
	"crypto/sha256"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

func TestLSPEnvironmentSortsAndValidates(t *testing.T) {
	entries, err := lspEnvironment(map[string]string{
		"ZED":   "z",
		"ALPHA": "a",
	})
	if err != nil {
		t.Fatalf("lspEnvironment: %v", err)
	}
	if len(entries) != 2 || entries[0] != "ALPHA=a" || entries[1] != "ZED=z" {
		t.Fatalf("entries = %v", entries)
	}
	for _, env := range []map[string]string{
		{"": "x"},
		{"A=B": "x"},
		{"A": "x,y"},
		{"A": "x\x00y"},
	} {
		if _, err := lspEnvironment(env); err == nil {
			t.Fatalf("lspEnvironment(%v) succeeded", env)
		}
	}
}

func TestLSPRootUsesWorkspaceAndConfinesSubdirectories(t *testing.T) {
	workspace := t.TempDir()
	sub := filepath.Join(workspace, "project")
	if err := os.Mkdir(sub, 0o700); err != nil {
		t.Fatal(err)
	}
	server := &Server{cfg: Config{Workspace: workspace}}

	root, err := server.lspRoot("")
	if err != nil || root != workspace {
		t.Fatalf("empty root = %q, %v", root, err)
	}
	root, err = server.lspRoot(".")
	if err != nil || root != workspace {
		t.Fatalf("dot root = %q, %v", root, err)
	}
	root, err = server.lspRoot("project")
	if err != nil || root != sub {
		t.Fatalf("project root = %q, %v", root, err)
	}
	if _, err := server.lspRoot("../outside"); err == nil {
		t.Fatal("outside root accepted")
	}
}

func TestLSPStartRequiresLSPOrEditorSession(t *testing.T) {
	server := &Server{cfg: Config{NoSandboxLSP: true}}
	cases := []struct {
		name  string
		state *connectionState
		code  int
	}{
		{"unhandshaken", &connectionState{}, -32003},
		{"plugin", &connectionState{handshaken: true, permissions: []string{"sys.exec"}}, -32002},
		{"editor", &connectionState{handshaken: true, isEditor: true}, -32602},
		{"lsp", &connectionState{handshaken: true, lspAuthorized: true}, -32602},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			_, rpcErr := server.lspStart(context.Background(), request{
				Params: json.RawMessage(`{}`),
			}, test.state)
			if rpcErr == nil || rpcErr.Code != test.code {
				t.Fatalf("lspStart error = %+v, want code %d", rpcErr, test.code)
			}
		})
	}
}

func TestLSPTokenAuthorizesNoSandboxLSP(t *testing.T) {
	token := "fedcba9876543210fedcba9876543210"
	hash := sha256.Sum256([]byte(token))
	server := &Server{cfg: Config{
		NoSandboxLSP:   true,
		LSPTokenSHA256: hash,
	}}
	state := &connectionState{}
	params, err := json.Marshal(helloRequestParams(token))
	if err != nil {
		t.Fatal(err)
	}
	if _, rpcErr := server.hello(request{Params: params}, state); rpcErr != nil {
		t.Fatalf("LSP handshake: %+v", rpcErr)
	}
	state.mu.Lock()
	authorized := state.lspAuthorized
	state.mu.Unlock()
	if !authorized {
		t.Fatal("LSP token did not authorize LSP start")
	}
	if _, rpcErr := server.lspStart(context.Background(), request{
		Params: json.RawMessage(`{}`),
	}, state); rpcErr == nil || rpcErr.Code != -32602 {
		t.Fatalf("authorized LSP start error = %+v, want invalid params", rpcErr)
	}
}

func TestLSPStartSandboxStillRequiresSysExec(t *testing.T) {
	server := &Server{cfg: Config{
		NoSandboxLSP: false,
		ExecRunner:   "/bin/true",
	}}
	state := &connectionState{handshaken: true, isEditor: true}
	_, rpcErr := server.lspStart(context.Background(), request{
		Params: json.RawMessage(`{}`),
	}, state)
	if rpcErr == nil || rpcErr.Code != -32002 {
		t.Fatalf("sandboxed LSP start error = %+v, want policy denied", rpcErr)
	}
}
