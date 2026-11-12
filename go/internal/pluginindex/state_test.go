package pluginindex

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

func TestCacheAcceptsAndLoads(t *testing.T) {
	fixture := newFixture(t)
	cache := Cache{Dir: t.TempDir()}
	if err := cache.AcceptIndex(&fixture.index, fixture.payload, fixture.signature, fixture.roots, "https://packages.example.invalid/index.json"); err != nil {
		t.Fatalf("accept index: %v", err)
	}
	revocationPayload, err := json.Marshal(fixture.revocation)
	if err != nil {
		t.Fatalf("marshal revocations: %v", err)
	}
	if err := cache.AcceptRevocations(&fixture.revocation, revocationPayload, sign(t, revocationPayload, fixture.private), fixture.roots); err != nil {
		t.Fatalf("accept revocations: %v", err)
	}
	state, indexPayload, storedRevocations, err := cache.Load()
	if err != nil {
		t.Fatalf("load: %v", err)
	}
	if state.IndexURL != "https://packages.example.invalid/index.json" {
		t.Fatalf("unexpected index URL: %q", state.IndexURL)
	}
	if string(indexPayload) != string(fixture.payload) {
		t.Fatal("index payload changed")
	}
	if string(storedRevocations) != string(revocationPayload) {
		t.Fatal("revocations payload changed")
	}
	if len(state.RevokedPackages) != 1 || state.RevokedPackages[0].PluginID != "anvil.demo" {
		t.Fatalf("unexpected revocations: %+v", state.RevokedPackages)
	}
}

func TestCacheRejectsRollback(t *testing.T) {
	fixture := newFixture(t)
	cache := Cache{Dir: t.TempDir()}
	if err := cache.AcceptIndex(&fixture.index, fixture.payload, fixture.signature, fixture.roots, ""); err != nil {
		t.Fatalf("accept index: %v", err)
	}
	older := fixture.index
	older.Sequence = 2
	payload, err := json.Marshal(older)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	if err := cache.AcceptIndex(&older, payload, sign(t, payload, fixture.private), fixture.roots, ""); err == nil {
		t.Fatal("lower sequence must be rejected")
	}
}

func TestCacheRejectsEquivocation(t *testing.T) {
	fixture := newFixture(t)
	cache := Cache{Dir: t.TempDir()}
	if err := cache.AcceptIndex(&fixture.index, fixture.payload, fixture.signature, fixture.roots, ""); err != nil {
		t.Fatalf("accept index: %v", err)
	}
	changed := fixture.index
	changed.Plugins[0].Summary = "changed"
	payload, err := json.Marshal(changed)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	if err := cache.AcceptIndex(&changed, payload, sign(t, payload, fixture.private), fixture.roots, ""); err == nil {
		t.Fatal("same sequence with different bytes must be rejected")
	}
}

func TestCacheRejectsDigestMismatch(t *testing.T) {
	fixture := newFixture(t)
	cache := Cache{Dir: t.TempDir()}
	if err := cache.AcceptIndex(&fixture.index, fixture.payload, fixture.signature, fixture.roots, ""); err != nil {
		t.Fatalf("accept index: %v", err)
	}
	path := filepath.Join(cache.Dir, indexFile)
	if err := os.WriteFile(path, []byte("{}"), cacheFileMode); err != nil {
		t.Fatalf("tamper cache: %v", err)
	}
	if _, _, _, err := cache.Load(); err == nil {
		t.Fatal("cache with mismatched digest must fail")
	}
}
