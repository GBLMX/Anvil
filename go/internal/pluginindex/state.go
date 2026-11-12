package pluginindex

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
)

const (
	indexFile      = "index.json"
	indexSigFile   = "index.sig"
	revocationFile = "revocations.json"
	revocationSig  = "revocations.sig"
	stateFile      = "state.json"
	cacheDirMode   = 0o700
	cacheFileMode  = 0o600
	stateVersion   = 1
)

// ErrCacheInvalid reports a missing or inconsistent cache.
var ErrCacheInvalid = errors.New("invalid signed index cache")

// DocumentState records the anti-rollback metadata for one document.
type DocumentState struct {
	Sequence uint64 `json:"sequence"`
	Digest   string `json:"digest"`
	IssuedAt string `json:"issued_at,omitempty"`
}

// RevokedPackage is one cached package revocation.
type RevokedPackage struct {
	PluginID string `json:"plugin_id"`
	Digest   string `json:"digest"`
}

// State is the anti-rollback and revocation cache state.
type State struct {
	Version           int              `json:"version"`
	IndexURL          string           `json:"index_url"`
	Index             DocumentState    `json:"index"`
	Revocations       DocumentState    `json:"revocations"`
	RevokedPackages   []RevokedPackage `json:"revoked_packages"`
	RevokedPublishers []string         `json:"revoked_publishers"`
}

// Cache stores verified documents and anti-rollback state.
type Cache struct {
	Dir string
}

// Load returns the cached documents and state. Documents whose digest does not
// match state.json are rejected.
func (cache Cache) Load() (State, []byte, []byte, error) {
	var state State
	data, err := os.ReadFile(filepath.Join(cache.Dir, stateFile))
	if err != nil {
		return State{}, nil, nil, fmt.Errorf("%w: state: %v", ErrCacheInvalid, err)
	}
	if err := decodeStrict(data, &state); err != nil {
		return State{}, nil, nil, fmt.Errorf("%w: state: %v", ErrCacheInvalid, err)
	}
	if state.Version != stateVersion {
		return State{}, nil, nil, fmt.Errorf("%w: unsupported state version", ErrCacheInvalid)
	}
	indexPayload, err := os.ReadFile(filepath.Join(cache.Dir, indexFile))
	if err != nil {
		return State{}, nil, nil, fmt.Errorf("%w: %s: %v", ErrCacheInvalid, indexFile, err)
	}
	revocationPayload, err := os.ReadFile(filepath.Join(cache.Dir, revocationFile))
	if err != nil {
		return State{}, nil, nil, fmt.Errorf("%w: %s: %v", ErrCacheInvalid, revocationFile, err)
	}
	if digest(indexPayload) != state.Index.Digest {
		return State{}, nil, nil, fmt.Errorf("%w: index digest mismatch", ErrCacheInvalid)
	}
	if digest(revocationPayload) != state.Revocations.Digest {
		return State{}, nil, nil, fmt.Errorf("%w: revocation digest mismatch", ErrCacheInvalid)
	}

	return state, indexPayload, revocationPayload, nil
}

// AcceptIndex verifies anti-rollback rules and stores a verified index.
func (cache Cache) AcceptIndex(document *Index, payload, signature []byte, roots RootKeys, indexURL string) error {
	if document == nil {
		return fmt.Errorf("%w: nil index", ErrInvalidDocument)
	}
	if err := VerifyDocument(payload, signature, roots); err != nil {
		return err
	}
	var parsed Index
	if err := decodeStrict(payload, &parsed); err != nil {
		return fmt.Errorf("%w: index JSON: %v", ErrInvalidDocument, err)
	}
	if parsed.Sequence != document.Sequence || parsed.IssuedAt != document.IssuedAt {
		return fmt.Errorf("%w: index metadata does not match payload", ErrInvalidDocument)
	}
	state, err := cache.loadStateForUpdate()
	if err != nil {
		return err
	}
	next := DocumentState{
		Sequence: document.Sequence,
		Digest:   digest(payload),
		IssuedAt: document.IssuedAt,
	}
	if err := acceptSequence(state.Index, next, "index"); err != nil {
		return err
	}
	state.Version = stateVersion
	state.IndexURL = indexURL
	state.Index = next
	if err := cache.write(state, indexFile, indexSigFile, payload, signature); err != nil {
		return err
	}
	return nil
}

// AcceptRevocations verifies anti-rollback rules, merges the append-only union,
// and stores a verified revocation document.
func (cache Cache) AcceptRevocations(document *Revocations, payload, signature []byte, roots RootKeys) error {
	if document == nil {
		return fmt.Errorf("%w: nil revocations", ErrInvalidDocument)
	}
	if err := VerifyDocument(payload, signature, roots); err != nil {
		return err
	}
	var parsed Revocations
	if err := decodeStrict(payload, &parsed); err != nil {
		return fmt.Errorf("%w: revocation JSON: %v", ErrInvalidDocument, err)
	}
	if parsed.Sequence != document.Sequence || parsed.IssuedAt != document.IssuedAt {
		return fmt.Errorf("%w: revocation metadata does not match payload", ErrInvalidDocument)
	}
	state, err := cache.loadStateForUpdate()
	if err != nil {
		return err
	}
	next := DocumentState{
		Sequence: document.Sequence,
		Digest:   digest(payload),
		IssuedAt: document.IssuedAt,
	}
	if err := acceptSequence(state.Revocations, next, "revocations"); err != nil {
		return err
	}
	set := NewRevocationSet()
	for _, entry := range state.RevokedPackages {
		set.Packages[entry.Digest] = entry.PluginID
	}
	for _, keyID := range state.RevokedPublishers {
		set.Publishers[keyID] = struct{}{}
	}
	if err := set.Merge(document); err != nil {
		return err
	}
	state.Revocations = next
	state.RevokedPackages = make([]RevokedPackage, 0, len(set.Packages))
	for digest, pluginID := range set.Packages {
		state.RevokedPackages = append(state.RevokedPackages, RevokedPackage{
			PluginID: pluginID,
			Digest:   digest,
		})
	}
	state.RevokedPublishers = make([]string, 0, len(set.Publishers))
	for keyID := range set.Publishers {
		state.RevokedPublishers = append(state.RevokedPublishers, keyID)
	}
	if err := cache.write(state, revocationFile, revocationSig, payload, signature); err != nil {
		return err
	}
	return nil
}

func (cache Cache) loadStateForUpdate() (State, error) {
	data, err := os.ReadFile(filepath.Join(cache.Dir, stateFile))
	if errors.Is(err, os.ErrNotExist) {
		return State{Version: stateVersion}, nil
	}
	if err != nil {
		return State{}, fmt.Errorf("%w: state: %v", ErrCacheInvalid, err)
	}
	var state State
	if err := decodeStrict(data, &state); err != nil {
		return State{}, fmt.Errorf("%w: state: %v", ErrCacheInvalid, err)
	}
	if state.Version != stateVersion {
		return State{}, fmt.Errorf("%w: unsupported state version", ErrCacheInvalid)
	}
	return state, nil
}

func acceptSequence(previous, next DocumentState, name string) error {
	if next.Sequence < previous.Sequence {
		return fmt.Errorf("%w: %s rollback", ErrInvalidDocument, name)
	}
	if next.Sequence == previous.Sequence && previous.Digest != "" && next.Digest != previous.Digest {
		return fmt.Errorf("%w: %s sequence equivocation", ErrInvalidDocument, name)
	}
	return nil
}

func (cache Cache) write(state State, payloadName, signatureName string, payload, signature []byte) error {
	if err := os.MkdirAll(cache.Dir, cacheDirMode); err != nil {
		return fmt.Errorf("create index cache: %w", err)
	}
	if err := os.Chmod(cache.Dir, cacheDirMode); err != nil {
		return fmt.Errorf("chmod index cache: %w", err)
	}
	if err := writeAtomic(cache.Dir, payloadName, payload); err != nil {
		return err
	}
	if err := writeAtomic(cache.Dir, signatureName, signature); err != nil {
		return err
	}
	encoded, err := json.Marshal(state)
	if err != nil {
		return fmt.Errorf("encode index state: %w", err)
	}
	return writeAtomic(cache.Dir, stateFile, encoded)
}

func writeAtomic(dir, name string, data []byte) error {
	temp, err := os.CreateTemp(dir, ".state-*")
	if err != nil {
		return fmt.Errorf("create %s: %w", name, err)
	}
	tempName := temp.Name()
	defer os.Remove(tempName)
	if err := temp.Chmod(cacheFileMode); err != nil {
		temp.Close()
		return fmt.Errorf("chmod %s: %w", name, err)
	}
	if _, err := temp.Write(data); err != nil {
		temp.Close()
		return fmt.Errorf("write %s: %w", name, err)
	}
	if err := temp.Sync(); err != nil {
		temp.Close()
		return fmt.Errorf("sync %s: %w", name, err)
	}
	if err := temp.Close(); err != nil {
		return fmt.Errorf("close %s: %w", name, err)
	}
	if err := os.Rename(tempName, filepath.Join(dir, name)); err != nil {
		return fmt.Errorf("rename %s: %w", name, err)
	}
	directory, err := os.Open(dir)
	if err != nil {
		return fmt.Errorf("open cache directory: %w", err)
	}
	defer directory.Close()
	if err := directory.Sync(); err != nil {
		return fmt.Errorf("sync cache directory: %w", err)
	}
	return nil
}

func digest(payload []byte) string {
	sum := sha256.Sum256(payload)
	return hex.EncodeToString(sum[:])
}

// ValidKeyID reports whether value is a lowercase sha256 key id.
func ValidKeyID(value string) bool {
	return validKeyID(value)
}
