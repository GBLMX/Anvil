package gateway

import (
	"context"
	"encoding/json"
	"testing"
	"time"
)

func TestExecGitApplyDeniedForPlugin(t *testing.T) {
	server := &Server{cfg: Config{
		ExecRunner:  "/bin/true",
		ExecTimeout: time.Second,
		Permissions: []string{"sys.exec"},
	}}
	state := &connectionState{
		handshaken:  true,
		pluginID:    "demo.plugin",
		permissions: []string{"sys.exec"},
	}
	params, err := json.Marshal(execParams{
		Tool: "/usr/bin/git",
		Args: []string{"--no-pager", "apply", "--cached", "--whitespace=nowarn", "--", ".anvil-tmp/git-hunk.patch"},
	})
	if err != nil {
		t.Fatal(err)
	}
	_, rpcErr := server.exec(context.Background(), request{Params: params}, state)
	if rpcErr == nil || rpcErr.Code != -32002 {
		t.Fatalf("expected policy denial, got %#v", rpcErr)
	}
}

func TestExecGitApplyAllowedForEditorConnection(t *testing.T) {
	server := &Server{cfg: Config{
		ExecRunner:  "/nonexistent/anvil-exec-runner",
		ExecTimeout: time.Second,
		Permissions: []string{"sys.exec"},
	}}
	state := &connectionState{
		handshaken:  true,
		permissions: []string{"sys.exec"},
	}
	params, err := json.Marshal(execParams{
		Tool: "/usr/bin/git",
		Args: []string{"--no-pager", "apply", "--cached", "--whitespace=nowarn", "--", ".anvil-tmp/git-hunk.patch"},
	})
	if err != nil {
		t.Fatal(err)
	}
	_, rpcErr := server.exec(context.Background(), request{Params: params}, state)
	if rpcErr == nil || rpcErr.Code != -32008 {
		t.Fatalf("editor connection did not reach runner: %#v", rpcErr)
	}
}
