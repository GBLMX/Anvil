package gateway

import (
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
