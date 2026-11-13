package pluginindex

import (
	"crypto/ed25519"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"strings"
	"testing"
	"time"
)

type fixture struct {
	now        time.Time
	private    ed25519.PrivateKey
	public     ed25519.PublicKey
	roots      RootKeys
	index      Index
	payload    []byte
	signature  []byte
	hosts      map[string]bool
	revocation Revocations
}

func newFixture(t *testing.T) fixture {
	t.Helper()
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("generate key: %v", err)
	}
	now := time.Date(2026, 11, 12, 12, 0, 0, 0, time.UTC)
	keyID := KeyID(public)
	digest := strings.Repeat("ab", sha256.Size)
	index := Index{
		Schema:        "anvil.plugin-index",
		SchemaVersion: 1,
		Sequence:      3,
		IssuedAt:      now.Add(-time.Hour).Format(time.RFC3339),
		ExpiresAt:     now.Add(24 * time.Hour).Format(time.RFC3339),
		Publishers: []Publisher{{
			ID:        "anvil",
			Name:      "Anvil",
			KeyID:     keyID,
			PublicKey: hex.EncodeToString(public),
			Official:  true,
		}},
		Plugins: []Plugin{{
			ID:          "anvil.demo",
			Name:        "Demo",
			Summary:     "A demo plugin.",
			PublisherID: "anvil",
			Homepage:    "https://packages.example.invalid/demo",
			License:     "MIT",
			Versions: []Version{{
				Version:      "1.0.0",
				Digest:       digest,
				ManifestHash: strings.Repeat("cd", sha256.Size),
				PackageURL:   "https://packages.example.invalid/demo.anvil-plugin",
				SignatureURL: "https://packages.example.invalid/demo.anvil-plugin.sig",
				PackageSize:  1024,
				APIMin:       "1.0.0",
				APIMax:       "2.0.0",
				Platforms:    []string{"linux-x86_64"},
				Permissions:  []string{"workspace.read"},
				PublishedAt:  now.Add(-time.Hour).Format(time.RFC3339),
			}},
		}},
	}
	payload, err := json.Marshal(index)
	if err != nil {
		t.Fatalf("marshal index: %v", err)
	}
	revocation := Revocations{
		Schema:        "anvil.plugin-revocations",
		SchemaVersion: 1,
		Sequence:      2,
		IssuedAt:      now.Add(-time.Hour).Format(time.RFC3339),
		ExpiresAt:     now.Add(24 * time.Hour).Format(time.RFC3339),
		Entries: []RevocationEntry{{
			Kind:      "package",
			PluginID:  "anvil.demo",
			Digest:    digest,
			Reason:    "malware",
			RevokedAt: now.Add(-time.Hour).Format(time.RFC3339),
		}},
	}
	return fixture{
		now:        now,
		private:    private,
		public:     public,
		roots:      RootKeys{keyID: public},
		index:      index,
		payload:    payload,
		signature:  sign(t, payload, private),
		hosts:      map[string]bool{"packages.example.invalid": true},
		revocation: revocation,
	}
}

func sign(t *testing.T, payload []byte, private ed25519.PrivateKey) []byte {
	t.Helper()
	digest := sha256.Sum256(payload)
	signature := ed25519.Sign(private, digest[:])
	public := private.Public().(ed25519.PublicKey)
	envelope, err := json.Marshal(signatureEnvelope{
		SchemaVersion: 1,
		KeyID:         KeyID(public),
		Signature:     hex.EncodeToString(signature),
	})
	if err != nil {
		t.Fatalf("marshal signature: %v", err)
	}
	return envelope
}

func TestLoadIndex(t *testing.T) {
	fixture := newFixture(t)
	index, err := LoadIndex(fixture.payload, fixture.signature, fixture.roots, fixture.now, fixture.hosts)
	if err != nil {
		t.Fatalf("load index: %v", err)
	}
	if index.Sequence != 3 || len(index.Plugins) != 1 {
		t.Fatalf("unexpected index: %+v", index)
	}
	version := index.Plugins[0].Versions[0]
	if !version.Compatible("1.2.0", "linux-x86_64") {
		t.Fatal("version should be compatible")
	}
	if version.Compatible("2.0.0", "linux-x86_64") {
		t.Fatal("api_max is exclusive")
	}
	if version.Compatible("1.2.0", "windows-x86_64") {
		t.Fatal("unsupported platform must be incompatible")
	}
}

func TestLoadIndexRejectsTampering(t *testing.T) {
	fixture := newFixture(t)
	tampered := append([]byte(nil), fixture.payload...)
	tampered[len(tampered)-2] ^= 1
	if _, err := LoadIndex(tampered, fixture.signature, fixture.roots, fixture.now, fixture.hosts); err == nil {
		t.Fatal("tampered payload must fail")
	}
}

func TestLoadIndexRejectsUnknownRoot(t *testing.T) {
	fixture := newFixture(t)
	if _, err := LoadIndex(fixture.payload, fixture.signature, RootKeys{}, fixture.now, fixture.hosts); err == nil {
		t.Fatal("unknown root must fail")
	}
}

func TestLoadIndexRejectsTrailingData(t *testing.T) {
	fixture := newFixture(t)
	payload := append(append([]byte(nil), fixture.payload...), []byte("\n{}")...)
	signature := sign(t, payload, fixture.private)
	if _, err := LoadIndex(payload, signature, fixture.roots, fixture.now, fixture.hosts); err == nil {
		t.Fatal("trailing data must fail")
	}
}

func TestLoadIndexRejectsUnknownPublisher(t *testing.T) {
	fixture := newFixture(t)
	index := fixture.index
	index.Plugins = append([]Plugin(nil), index.Plugins...)
	index.Plugins[0].PublisherID = "missing"
	payload, err := json.Marshal(index)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	if _, err := LoadIndex(payload, sign(t, payload, fixture.private), fixture.roots, fixture.now, fixture.hosts); err == nil {
		t.Fatal("unknown publisher must fail")
	}
}

func TestLoadIndexRejectsDisallowedHost(t *testing.T) {
	fixture := newFixture(t)
	if _, err := LoadIndex(
		fixture.payload,
		fixture.signature,
		fixture.roots,
		fixture.now,
		map[string]bool{"other.example.invalid": true},
	); err == nil {
		t.Fatal("disallowed host must fail")
	}
}

func TestLoadRevocationsAndUnion(t *testing.T) {
	fixture := newFixture(t)
	payload, err := json.Marshal(fixture.revocation)
	if err != nil {
		t.Fatalf("marshal revocations: %v", err)
	}
	document, err := LoadRevocations(payload, sign(t, payload, fixture.private), fixture.roots, fixture.now)
	if err != nil {
		t.Fatalf("load revocations: %v", err)
	}
	set := NewRevocationSet()
	if err := set.Merge(document); err != nil {
		t.Fatalf("merge: %v", err)
	}
	digest := fixture.index.Plugins[0].Versions[0].Digest
	if !set.PackageRevoked(digest, "anvil.demo") {
		t.Fatal("package should be revoked")
	}
	if set.PackageRevoked(digest, "other.plugin") {
		t.Fatal("revocation owner must match")
	}
	if set.PublisherRevoked(KeyID(fixture.public)) {
		t.Fatal("publisher is not revoked")
	}
	next := &Revocations{Entries: []RevocationEntry{{
		Kind:           "publisher",
		PublisherKeyID: KeyID(fixture.public),
		Reason:         "key compromise",
		RevokedAt:      fixture.now.Format(time.RFC3339),
	}}}
	if err := set.Merge(next); err != nil {
		t.Fatalf("merge publisher: %v", err)
	}
	if !set.PublisherRevoked(KeyID(fixture.public)) {
		t.Fatal("publisher should be revoked")
	}
	if !set.PackageRevoked(digest, "anvil.demo") {
		t.Fatal("merge must not remove cached revocations")
	}
}

func TestLoadRevocationsRejectsConflictingOwner(t *testing.T) {
	fixture := newFixture(t)
	document := fixture.revocation
	document.Entries = append(document.Entries, RevocationEntry{
		Kind:      "package",
		PluginID:  "other.plugin",
		Digest:    document.Entries[0].Digest,
		Reason:    "conflict",
		RevokedAt: fixture.now.Format(time.RFC3339),
	})
	payload, err := json.Marshal(document)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	if _, err := LoadRevocations(payload, sign(t, payload, fixture.private), fixture.roots, fixture.now); err == nil {
		t.Fatal("conflicting digest owner must fail")
	}
}

func TestVerifyRejectsUppercaseSignature(t *testing.T) {
	fixture := newFixture(t)
	var envelope signatureEnvelope
	if err := json.Unmarshal(fixture.signature, &envelope); err != nil {
		t.Fatalf("decode signature: %v", err)
	}
	envelope.Signature = strings.ToUpper(envelope.Signature)
	encoded, err := json.Marshal(envelope)
	if err != nil {
		t.Fatalf("encode signature: %v", err)
	}
	if err := VerifyDocument(fixture.payload, encoded, fixture.roots); err == nil {
		t.Fatal("uppercase signature must fail")
	}
}

func TestRejectsExpiredIndex(t *testing.T) {
	f := newFixture(t)
	f.index.IssuedAt = f.now.Add(-2 * time.Hour).Format(time.RFC3339)
	f.index.ExpiresAt = f.now.Add(-time.Hour).Format(time.RFC3339)
	payload, err := json.Marshal(f.index)
	if err != nil {
		t.Fatalf("marshal index: %v", err)
	}
	index, err := LoadIndex(payload, sign(t, payload, f.private), f.roots, f.now, f.hosts)
	if !errors.Is(err, ErrDocumentExpired) || index == nil || index.Sequence != f.index.Sequence {
		t.Fatalf("expired index = %#v, error = %v", index, err)
	}
}

func TestRejectsFutureIndex(t *testing.T) {
	f := newFixture(t)
	f.index.IssuedAt = f.now.Add(MaxFutureSkew + time.Minute).Format(time.RFC3339)
	f.index.ExpiresAt = f.index.IssuedAt
	payload, err := json.Marshal(f.index)
	if err != nil {
		t.Fatalf("marshal index: %v", err)
	}
	if _, err := LoadIndex(payload, sign(t, payload, f.private), f.roots, f.now, f.hosts); err == nil || !strings.Contains(err.Error(), "future") {
		t.Fatalf("future index error = %v", err)
	}
}

func TestRejectsExpiredRevocations(t *testing.T) {
	f := newFixture(t)
	f.revocation.IssuedAt = f.now.Add(-2 * time.Hour).Format(time.RFC3339)
	f.revocation.ExpiresAt = f.now.Add(-time.Hour).Format(time.RFC3339)
	payload, err := json.Marshal(f.revocation)
	if err != nil {
		t.Fatalf("marshal revocations: %v", err)
	}
	revocations, err := LoadRevocations(payload, sign(t, payload, f.private), f.roots, f.now)
	if !errors.Is(err, ErrDocumentExpired) || revocations == nil || revocations.Sequence != f.revocation.Sequence {
		t.Fatalf("expired revocations = %#v, error = %v", revocations, err)
	}
}

func TestRejectsFutureRevocations(t *testing.T) {
	f := newFixture(t)
	f.revocation.IssuedAt = f.now.Add(MaxFutureSkew + time.Minute).Format(time.RFC3339)
	f.revocation.ExpiresAt = f.now.Add(MaxFutureSkew + time.Hour).Format(time.RFC3339)
	payload, err := json.Marshal(f.revocation)
	if err != nil {
		t.Fatalf("marshal revocations: %v", err)
	}
	revocations, err := LoadRevocations(payload, sign(t, payload, f.private), f.roots, f.now)
	if err == nil || revocations != nil {
		t.Fatalf("future revocations = %#v, error = %v", revocations, err)
	}
}
