package pkg

import (
	"crypto/ed25519"
	"crypto/rand"
	"os"
	"path/filepath"
	"testing"
)

func TestTrustStoreRoundTrip(t *testing.T) {
	root := t.TempDir()
	path := filepath.Join(root, "anvil", "trust.json")
	public, _, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("key: %v", err)
	}
	store, err := LoadTrustStore(path)
	if err != nil {
		t.Fatalf("load empty: %v", err)
	}
	if store.Contains(public) {
		t.Fatal("empty store trusted a key")
	}
	if err := store.Add("demo", public); err != nil {
		t.Fatalf("add: %v", err)
	}
	if err := store.Save(path); err != nil {
		t.Fatalf("save: %v", err)
	}
	info, err := os.Stat(path)
	if err != nil {
		t.Fatalf("stat: %v", err)
	}
	if info.Mode().Perm() != 0o600 {
		t.Fatalf("trust mode = %o, want 600", info.Mode().Perm())
	}
	loaded, err := LoadTrustStore(path)
	if err != nil {
		t.Fatalf("reload: %v", err)
	}
	if !loaded.Contains(public) {
		t.Fatal("saved key was not trusted")
	}
}

func TestDefaultTrustStorePathUsesConfigDir(t *testing.T) {
	root := t.TempDir()
	t.Setenv("ANVIL_TRUST_STORE", "")
	t.Setenv("XDG_CONFIG_HOME", root)
	want := filepath.Join(root, "anvil", "trust.json")
	if got := DefaultTrustStorePath(); got != want {
		t.Fatalf("DefaultTrustStorePath() = %q, want %q", got, want)
	}
}
