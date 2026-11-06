package pkg

import (
	"crypto/ed25519"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"
)

// ErrUntrustedKey reports a valid signature whose key is not trusted.
var ErrUntrustedKey = errors.New("signing key is not trusted")

// TrustedKey is one explicitly trusted Ed25519 public key.
type TrustedKey struct {
	Name      string `json:"name"`
	PublicKey string `json:"public_key"`
	AddedAt   int64  `json:"added_at"`
}

// TrustStore is the local trust database.
type TrustStore struct {
	Version int          `json:"version"`
	Keys    []TrustedKey `json:"keys"`
}

// DefaultTrustStorePath resolves ~/.config/anvil/trust.json.
func DefaultTrustStorePath() string {
	if path := os.Getenv("ANVIL_TRUST_STORE"); path != "" {
		return path
	}
	config, err := os.UserConfigDir()
	if err == nil && config != "" {
		return filepath.Join(config, "anvil", "trust.json")
	}
	home, _ := os.UserHomeDir()
	return filepath.Join(home, ".config", "anvil", "trust.json")
}

// LoadTrustStore reads the trust store; a missing file is an empty store.
func LoadTrustStore(path string) (*TrustStore, error) {
	if path == "" {
		path = DefaultTrustStorePath()
	}
	data, err := os.ReadFile(path)
	if errors.Is(err, os.ErrNotExist) {
		return &TrustStore{Version: 1}, nil
	}
	if err != nil {
		return nil, fmt.Errorf("read trust store: %w", err)
	}
	var store TrustStore
	if err := json.Unmarshal(data, &store); err != nil {
		return nil, fmt.Errorf("parse trust store: %w", err)
	}
	if store.Version != 0 && store.Version != 1 {
		return nil, fmt.Errorf("unsupported trust store version %d", store.Version)
	}
	for _, key := range store.Keys {
		if _, err := decodeHexKey(key.PublicKey); err != nil {
			return nil, fmt.Errorf("trust key %q: %w", key.Name, err)
		}
	}
	store.Version = 1
	return &store, nil
}

func (s *TrustStore) Contains(publicKey ed25519.PublicKey) bool {
	wanted := hex.EncodeToString(publicKey)
	for _, key := range s.Keys {
		if strings.EqualFold(key.PublicKey, wanted) {
			return true
		}
	}
	return false
}

// Add inserts or refreshes one trusted key.
func (s *TrustStore) Add(name string, publicKey ed25519.PublicKey) error {
	if len(publicKey) != ed25519.PublicKeySize {
		return errors.New("public key must be 32 bytes")
	}
	encoded := hex.EncodeToString(publicKey)
	for index := range s.Keys {
		if strings.EqualFold(s.Keys[index].PublicKey, encoded) {
			if name != "" {
				s.Keys[index].Name = name
			}
			s.Keys[index].AddedAt = time.Now().Unix()
			return nil
		}
	}
	if name == "" {
		name = encoded[:16]
	}
	s.Keys = append(s.Keys, TrustedKey{
		Name:      name,
		PublicKey: encoded,
		AddedAt:   time.Now().Unix(),
	})
	return nil
}

// Save writes the trust store atomically with owner-only permissions.
func (s *TrustStore) Save(path string) error {
	if path == "" {
		path = DefaultTrustStorePath()
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return fmt.Errorf("create trust directory: %w", err)
	}
	data, err := json.MarshalIndent(s, "", "  ")
	if err != nil {
		return fmt.Errorf("encode trust store: %w", err)
	}
	tmp, err := os.CreateTemp(filepath.Dir(path), ".trust-*.tmp")
	if err != nil {
		return fmt.Errorf("create trust temp: %w", err)
	}
	tmpName := tmp.Name()
	defer os.Remove(tmpName)
	if err := tmp.Chmod(0o600); err != nil {
		tmp.Close()
		return fmt.Errorf("chmod trust temp: %w", err)
	}
	if _, err := tmp.Write(data); err != nil {
		tmp.Close()
		return fmt.Errorf("write trust temp: %w", err)
	}
	if err := tmp.Sync(); err != nil {
		tmp.Close()
		return fmt.Errorf("sync trust temp: %w", err)
	}
	if err := tmp.Close(); err != nil {
		return fmt.Errorf("close trust temp: %w", err)
	}
	if err := os.Rename(tmpName, path); err != nil {
		return fmt.Errorf("commit trust store: %w", err)
	}
	return nil
}

// AddTrustedKey loads, adds, and saves one key.
func AddTrustedKey(path, name string, publicKey ed25519.PublicKey) error {
	store, err := LoadTrustStore(path)
	if err != nil {
		return err
	}
	if err := store.Add(name, publicKey); err != nil {
		return err
	}
	return store.Save(path)
}

// InstallTrusted verifies the package and requires its key in the trust store.
func InstallTrusted(archivePath, signaturePath string, publicKey ed25519.PublicKey, destination, trustPath string) ([32]byte, error) {
	store, err := LoadTrustStore(trustPath)
	if err != nil {
		return [32]byte{}, err
	}
	if !store.Contains(publicKey) {
		return [32]byte{}, fmt.Errorf("%w: %s", ErrUntrustedKey, hex.EncodeToString(publicKey))
	}
	return Install(archivePath, signaturePath, publicKey, destination)
}

func decodeHexKey(value string) ([]byte, error) {
	decoded, err := hex.DecodeString(strings.TrimSpace(value))
	if err != nil || len(decoded) != ed25519.PublicKeySize {
		return nil, errors.New("public key must be 32 bytes in hex")
	}
	return decoded, nil
}
