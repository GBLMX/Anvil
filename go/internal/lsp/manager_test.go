package lsp

import (
	"context"
	"testing"
	"time"
)

func TestManagerRoundTripWithCat(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	manager, err := Start(ctx, Config{Command: "/bin/cat"})
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = manager.Close() }()
	body := []byte(`{"jsonrpc":"2.0","method":"initialized"}`)
	if err := manager.Send(body); err != nil {
		t.Fatal(err)
	}
	got, err := manager.Recv()
	if err != nil {
		t.Fatal(err)
	}
	if string(got) != string(body) {
		t.Fatalf("body = %q", got)
	}
}
