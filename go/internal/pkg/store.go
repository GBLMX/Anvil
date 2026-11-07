package pkg

import (
	"crypto/ed25519"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"

	"golang.org/x/sys/unix"
)

const (
	// StoreVersion is the installed-state schema version.
	StoreVersion  = 1
	stateFile     = "state.json"
	stateLockFile = "state.lock"
	storeDir      = "store"
	storeLockFile = "store.lock"
)

// InstalledVersion is one retained package digest for a plugin.
type InstalledVersion struct {
	Digest       string `json:"digest"`
	Version      string `json:"version"`
	ManifestHash string `json:"manifest_hash"`
	InstalledAt  int64  `json:"installed_at"`
}

// InstalledPlugin is the persisted installed state for one plugin id.
type InstalledPlugin struct {
	ActiveDigest       string             `json:"active_digest"`
	Source             string             `json:"source"`
	Enabled            bool               `json:"enabled"`
	GrantedPermissions []string           `json:"granted_permissions"`
	Versions           []InstalledVersion `json:"versions"`
}

// StoreState is the persisted plugin store index.
type StoreState struct {
	Version int                        `json:"version"`
	Plugins map[string]InstalledPlugin `json:"plugins"`
}

// StoreMeta is the persisted metadata for one content-addressed entry.
type StoreMeta struct {
	Digest       string   `json:"digest"`
	PluginID     string   `json:"plugin_id"`
	Version      string   `json:"version"`
	ManifestHash string   `json:"manifest_hash"`
	Permissions  []string `json:"permissions"`
	InstalledAt  int64    `json:"installed_at"`
}

// ManagedPlugin describes one active managed plugin.
type ManagedPlugin struct {
	ID             string   `json:"id"`
	Root           string   `json:"root"`
	Digest         string   `json:"digest"`
	Version        string   `json:"version"`
	Source         string   `json:"source"`
	Enabled        bool     `json:"enabled"`
	PreviousDigest string   `json:"previous_digest,omitempty"`
	Permissions    []string `json:"permissions"`
}

// Store owns the content-addressed package store and installed state.
type Store struct {
	Root string
}

// DefaultStoreRoot resolves $XDG_DATA_HOME/anvil/plugins, falling back to
// ~/.local/share/anvil/plugins.
func DefaultStoreRoot() (string, error) {
	if data := strings.TrimSpace(os.Getenv("XDG_DATA_HOME")); data != "" {
		return filepath.Join(data, "anvil", "plugins"), nil
	}
	home, err := os.UserHomeDir()
	if err != nil || home == "" {
		return "", errors.New("resolve plugin data directory: no home directory")
	}
	return filepath.Join(home, ".local", "share", "anvil", "plugins"), nil
}

// NewStore creates or opens a package store at root. An empty root uses the
// default data directory.
func NewStore(root string) (*Store, error) {
	if root == "" {
		defaultRoot, err := DefaultStoreRoot()
		if err != nil {
			return nil, err
		}
		root = defaultRoot
	}
	if err := os.MkdirAll(root, 0o700); err != nil {
		return nil, fmt.Errorf("create plugin data directory: %w", err)
	}
	if err := os.Chmod(root, 0o700); err != nil {
		return nil, fmt.Errorf("secure plugin data directory: %w", err)
	}
	if err := os.MkdirAll(filepath.Join(root, storeDir), 0o700); err != nil {
		return nil, fmt.Errorf("create plugin store: %w", err)
	}
	return &Store{Root: root}, nil
}

// StatePath returns the installed-state path.
func (s *Store) StatePath() string {
	return filepath.Join(s.Root, stateFile)
}

// EntryPath returns the immutable store directory for a digest.
func (s *Store) EntryPath(digest string) (string, error) {
	if err := ValidateDigest(digest); err != nil {
		return "", err
	}
	return filepath.Join(s.Root, storeDir, digest), nil
}

// ValidateDigest checks a lowercase or uppercase SHA-256 hex digest.
func ValidateDigest(digest string) error {
	decoded, err := hex.DecodeString(strings.TrimSpace(digest))
	if err != nil || len(decoded) != 32 {
		return errors.New("digest must be a 32-byte SHA-256 hex string")
	}
	return nil
}

func emptyState() *StoreState {
	return &StoreState{Version: StoreVersion, Plugins: map[string]InstalledPlugin{}}
}

// ReadState returns a snapshot of installed state. A missing file is empty.
func (s *Store) ReadState() (*StoreState, error) {
	return loadState(s.StatePath())
}

func loadState(path string) (*StoreState, error) {
	data, err := os.ReadFile(path)
	if errors.Is(err, os.ErrNotExist) {
		return emptyState(), nil
	}
	if err != nil {
		return nil, fmt.Errorf("read plugin state: %w", err)
	}
	var state StoreState
	if err := json.Unmarshal(data, &state); err != nil {
		return nil, fmt.Errorf("parse plugin state: %w", err)
	}
	if state.Version != StoreVersion {
		return nil, fmt.Errorf("unsupported plugin state version %d", state.Version)
	}
	if state.Plugins == nil {
		state.Plugins = map[string]InstalledPlugin{}
	}
	if err := validateState(&state); err != nil {
		return nil, err
	}
	return &state, nil
}

func validateState(state *StoreState) error {
	for pluginID, plugin := range state.Plugins {
		if pluginID == "" {
			return errors.New("plugin state has an empty plugin id")
		}
		if plugin.ActiveDigest == "" {
			return fmt.Errorf("plugin %q has no active digest", pluginID)
		}
		if err := ValidateDigest(plugin.ActiveDigest); err != nil {
			return fmt.Errorf("plugin %q active digest: %w", pluginID, err)
		}
		seen := map[string]struct{}{}
		active := false
		for _, version := range plugin.Versions {
			if err := ValidateDigest(version.Digest); err != nil {
				return fmt.Errorf("plugin %q version digest: %w", pluginID, err)
			}
			if version.Version == "" {
				return fmt.Errorf("plugin %q has an empty version", pluginID)
			}
			if _, duplicate := seen[version.Digest]; duplicate {
				return fmt.Errorf("plugin %q repeats digest %s", pluginID, version.Digest)
			}
			seen[version.Digest] = struct{}{}
			if strings.EqualFold(version.Digest, plugin.ActiveDigest) {
				active = true
			}
		}
		if !active {
			return fmt.Errorf("plugin %q active digest is not in versions", pluginID)
		}
	}
	return nil
}

// UpdateState serializes a read-modify-write transaction under state.lock.
func (s *Store) UpdateState(update func(*StoreState) error) error {
	guard, err := s.lockState()
	if err != nil {
		return err
	}
	defer guard.Close()
	state, err := loadState(s.StatePath())
	if err != nil {
		return err
	}
	if err := update(state); err != nil {
		return err
	}
	if state.Version != StoreVersion {
		return fmt.Errorf("unsupported plugin state version %d", state.Version)
	}
	if err := validateState(state); err != nil {
		return err
	}
	return s.saveState(state)
}

type stateGuard struct {
	file *os.File
}

func (s *Store) lockState() (*stateGuard, error) {
	return s.lockFile(stateLockFile)
}

func (s *Store) lockStore() (*stateGuard, error) {
	return s.lockFile(storeLockFile)
}

func (s *Store) lockFile(name string) (*stateGuard, error) {
	path := filepath.Join(s.Root, name)
	file, err := os.OpenFile(path, os.O_CREATE|os.O_RDWR, 0o600)
	if err != nil {
		return nil, fmt.Errorf("open plugin lock %s: %w", name, err)
	}
	if err := file.Chmod(0o600); err != nil {
		_ = file.Close()
		return nil, fmt.Errorf("secure plugin lock %s: %w", name, err)
	}
	if err := unix.Flock(int(file.Fd()), unix.LOCK_EX); err != nil {
		_ = file.Close()
		return nil, fmt.Errorf("lock plugin %s: %w", name, err)
	}
	return &stateGuard{file: file}, nil
}

func (g *stateGuard) Close() {
	if g.file == nil {
		return
	}
	_ = unix.Flock(int(g.file.Fd()), unix.LOCK_UN)
	_ = g.file.Close()
	g.file = nil
}

func (s *Store) saveState(state *StoreState) error {
	data, err := json.MarshalIndent(state, "", "  ")
	if err != nil {
		return fmt.Errorf("encode plugin state: %w", err)
	}
	temp, err := os.CreateTemp(s.Root, ".state-*.tmp")
	if err != nil {
		return fmt.Errorf("create plugin state temp: %w", err)
	}
	tempPath := temp.Name()
	defer func() { _ = os.Remove(tempPath) }()
	if err := temp.Chmod(0o600); err != nil {
		_ = temp.Close()
		return fmt.Errorf("secure plugin state temp: %w", err)
	}
	if _, err := temp.Write(data); err != nil {
		_ = temp.Close()
		return fmt.Errorf("write plugin state temp: %w", err)
	}
	if err := temp.Sync(); err != nil {
		_ = temp.Close()
		return fmt.Errorf("sync plugin state temp: %w", err)
	}
	if err := temp.Close(); err != nil {
		return fmt.Errorf("close plugin state temp: %w", err)
	}
	if err := os.Rename(tempPath, s.StatePath()); err != nil {
		return fmt.Errorf("commit plugin state: %w", err)
	}
	return syncDir(s.Root)
}

func syncDir(path string) error {
	file, err := os.Open(path)
	if err != nil {
		return fmt.Errorf("open directory %s: %w", path, err)
	}
	defer file.Close()
	if err := file.Sync(); err != nil {
		return fmt.Errorf("sync directory %s: %w", path, err)
	}
	return nil
}

// AddInstalledVersion records an installed digest without enabling it.
func (s *Store) AddInstalledVersion(pluginID, digest, version, manifestHash, source string, permissions []string) error {
	if pluginID == "" || version == "" {
		return errors.New("plugin id and version are required")
	}
	if err := ValidateDigest(digest); err != nil {
		return err
	}
	if entry, err := s.EntryPath(digest); err != nil {
		return err
	} else if info, statErr := os.Stat(entry); statErr != nil || !info.IsDir() {
		return fmt.Errorf("plugin digest %s is not in the store", digest)
	}
	return s.UpdateState(func(state *StoreState) error {
		plugin, exists := state.Plugins[pluginID]
		if !exists {
			plugin = InstalledPlugin{
				ActiveDigest:       digest,
				Source:             source,
				Enabled:            false,
				GrantedPermissions: append([]string(nil), permissions...),
			}
		}
		if exists && !samePermissions(plugin.GrantedPermissions, permissions) {
			plugin.Enabled = false
			plugin.GrantedPermissions = []string{}
		}
		for _, installed := range plugin.Versions {
			if strings.EqualFold(installed.Digest, digest) {
				plugin.ActiveDigest = digest
				state.Plugins[pluginID] = plugin
				return nil
			}
		}
		plugin.Versions = append(plugin.Versions, InstalledVersion{
			Digest:       digest,
			Version:      version,
			ManifestHash: manifestHash,
			InstalledAt:  time.Now().Unix(),
		})
		plugin.ActiveDigest = digest
		state.Plugins[pluginID] = plugin
		return nil
	})
}

// SetEnabled changes the persisted enabled flag without activating a host.
func (s *Store) SetEnabled(pluginID string, enabled bool) error {
	if pluginID == "" {
		return errors.New("plugin id is required")
	}
	return s.UpdateState(func(state *StoreState) error {
		plugin, exists := state.Plugins[pluginID]
		if !exists {
			return fmt.Errorf("plugin %q is not installed", pluginID)
		}
		plugin.Enabled = enabled
		state.Plugins[pluginID] = plugin
		return nil
	})
}

func samePermissions(left, right []string) bool {
	if len(left) != len(right) {
		return false
	}
	leftSet := make(map[string]struct{}, len(left))
	for _, permission := range left {
		leftSet[permission] = struct{}{}
	}
	for _, permission := range right {
		if _, exists := leftSet[permission]; !exists {
			return false
		}
	}
	return true
}

// InstallPackage verifies a package and commits it to the content-addressed
// store without enabling it.
func (s *Store) InstallPackage(
	archivePath, signaturePath string,
	publicKey ed25519.PublicKey,
	expectedDigest, trustPath, source string,
) (ManagedPlugin, error) {
	guard, lockErr := s.lockStore()
	if lockErr != nil {
		return ManagedPlugin{}, lockErr
	}
	defer guard.Close()
	if source == "" {
		source = "local-file"
	}
	digest, err := VerifyFile(archivePath, signaturePath, publicKey)
	if err != nil {
		return ManagedPlugin{}, err
	}
	digestHex := hex.EncodeToString(digest[:])
	if expectedDigest != "" && !strings.EqualFold(expectedDigest, digestHex) {
		return ManagedPlugin{}, errors.New("package digest does not match the reviewed archive")
	}
	trust, err := LoadTrustStore(trustPath)
	if err != nil {
		return ManagedPlugin{}, err
	}
	if !trust.Contains(publicKey) {
		return ManagedPlugin{}, ErrUntrustedKey
	}
	manifest, err := InspectArchive(archivePath)
	if err != nil {
		return ManagedPlugin{}, err
	}
	if !ValidPluginID(manifest.ID) {
		return ManagedPlugin{}, errors.New("invalid plugin id")
	}
	manifestJSON, err := json.Marshal(manifest)
	if err != nil {
		return ManagedPlugin{}, fmt.Errorf("encode manifest: %w", err)
	}
	manifestHash := sha256.Sum256(manifestJSON)
	manifestHashHex := hex.EncodeToString(manifestHash[:])
	entry, err := s.EntryPath(digestHex)
	if err != nil {
		return ManagedPlugin{}, err
	}
	if info, statErr := os.Stat(entry); statErr == nil {
		if !info.IsDir() {
			return ManagedPlugin{}, fmt.Errorf("store entry %s is not a directory", entry)
		}
	} else if !errors.Is(statErr, os.ErrNotExist) {
		return ManagedPlugin{}, fmt.Errorf("stat store entry: %w", statErr)
	} else {
		stage, err := os.MkdirTemp(s.Root, ".store-*")
		if err != nil {
			return ManagedPlugin{}, fmt.Errorf("create store stage: %w", err)
		}
		keepStage := false
		defer func() {
			if !keepStage {
				_ = os.RemoveAll(stage)
			}
		}()
		root := filepath.Join(stage, "root")
		if _, err := InstallTrusted(archivePath, signaturePath, publicKey, root, trustPath); err != nil {
			return ManagedPlugin{}, err
		}
		archive, err := os.ReadFile(archivePath)
		if err != nil {
			return ManagedPlugin{}, fmt.Errorf("read package: %w", err)
		}
		if err := os.WriteFile(filepath.Join(stage, "package.anvil-plugin"), archive, 0o600); err != nil {
			return ManagedPlugin{}, fmt.Errorf("store package archive: %w", err)
		}
		meta := StoreMeta{
			Digest:       digestHex,
			PluginID:     manifest.ID,
			Version:      manifest.Version,
			ManifestHash: manifestHashHex,
			Permissions:  append([]string(nil), manifest.Permissions...),
			InstalledAt:  time.Now().Unix(),
		}
		metaJSON, err := json.MarshalIndent(meta, "", "  ")
		if err != nil {
			return ManagedPlugin{}, fmt.Errorf("encode store metadata: %w", err)
		}
		if err := os.WriteFile(filepath.Join(stage, "meta.json"), metaJSON, 0o600); err != nil {
			return ManagedPlugin{}, fmt.Errorf("write store metadata: %w", err)
		}
		if err := makeTreeReadOnly(root); err != nil {
			return ManagedPlugin{}, err
		}
		if err := os.Rename(stage, entry); err != nil {
			if _, statErr := os.Stat(entry); statErr != nil {
				return ManagedPlugin{}, fmt.Errorf("commit store entry: %w", err)
			}
		} else {
			keepStage = true
		}
		if err := os.Chmod(entry, 0o500); err != nil {
			return ManagedPlugin{}, fmt.Errorf("secure store entry: %w", err)
		}
		if err := syncDir(filepath.Join(s.Root, storeDir)); err != nil {
			return ManagedPlugin{}, err
		}
	}
	if err := s.AddInstalledVersion(
		manifest.ID,
		digestHex,
		manifest.Version,
		manifestHashHex,
		source,
		manifest.Permissions,
	); err != nil {
		return ManagedPlugin{}, err
	}
	return ManagedPlugin{
		ID:          manifest.ID,
		Root:        filepath.Join(entry, "root"),
		Digest:      digestHex,
		Version:     manifest.Version,
		Source:      source,
		Enabled:     false,
		Permissions: append([]string(nil), manifest.Permissions...),
	}, nil
}

// ListInstalled returns active managed plugins whose store roots exist.
func (s *Store) ListInstalled() ([]ManagedPlugin, error) {
	state, err := s.ReadState()
	if err != nil {
		return nil, err
	}
	plugins := make([]ManagedPlugin, 0, len(state.Plugins))
	for id, installed := range state.Plugins {
		entry, err := s.EntryPath(installed.ActiveDigest)
		if err != nil {
			return nil, fmt.Errorf("plugin %q: %w", id, err)
		}
		if info, err := os.Stat(filepath.Join(entry, "root")); err != nil || !info.IsDir() {
			return nil, fmt.Errorf("plugin %q store root is missing", id)
		}
		version := ""
		previous := ""
		for index, candidate := range installed.Versions {
			if strings.EqualFold(candidate.Digest, installed.ActiveDigest) {
				version = candidate.Version
				if index > 0 {
					previous = installed.Versions[index-1].Digest
				}
				break
			}
		}
		plugins = append(plugins, ManagedPlugin{
			ID:             id,
			Root:           filepath.Join(entry, "root"),
			Digest:         installed.ActiveDigest,
			Version:        version,
			Source:         installed.Source,
			Enabled:        installed.Enabled,
			PreviousDigest: previous,

			Permissions: append([]string(nil), installed.GrantedPermissions...),
		})
	}
	return plugins, nil
}

// Rollback switches one plugin to the version before its active digest.
func (s *Store) Rollback(pluginID string) (string, error) {
	var next string
	err := s.UpdateState(func(state *StoreState) error {
		plugin, exists := state.Plugins[pluginID]
		if !exists {
			return fmt.Errorf("plugin %q is not installed", pluginID)
		}
		for index, version := range plugin.Versions {
			if !strings.EqualFold(version.Digest, plugin.ActiveDigest) {
				continue
			}
			if index == 0 {
				return errors.New("no previous version to roll back to")
			}
			next = plugin.Versions[index-1].Digest
			plugin.ActiveDigest = next
			state.Plugins[pluginID] = plugin
			return nil
		}
		return errors.New("active digest is not in versions")
	})
	return next, err
}

// Uninstall removes installed state and unreferenced store entries.
func (s *Store) Uninstall(pluginID string) error {
	guard, lockErr := s.lockStore()
	if lockErr != nil {
		return lockErr
	}
	defer guard.Close()
	var digests []string
	err := s.UpdateState(func(state *StoreState) error {
		plugin, exists := state.Plugins[pluginID]
		if !exists {
			return fmt.Errorf("plugin %q is not installed", pluginID)
		}
		for _, version := range plugin.Versions {
			digests = append(digests, version.Digest)
		}
		delete(state.Plugins, pluginID)
		return nil
	})
	if err != nil {
		return err
	}
	state, err := s.ReadState()
	if err != nil {
		return err
	}
	referenced := map[string]struct{}{}
	for _, plugin := range state.Plugins {
		for _, version := range plugin.Versions {
			referenced[strings.ToLower(version.Digest)] = struct{}{}
		}
	}
	for _, digest := range digests {
		if _, used := referenced[strings.ToLower(digest)]; used {
			continue
		}
		entry, err := s.EntryPath(digest)
		if err != nil {
			return err
		}
		if err := removeStoreEntry(entry); err != nil {
			return err
		}
	}
	return nil
}

// StoreSize returns the total bytes held under store/.
func (s *Store) StoreSize() (int64, error) {
	var size int64
	root := filepath.Join(s.Root, storeDir)
	err := filepath.WalkDir(root, func(_ string, entry os.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		if entry.Type().IsRegular() {
			info, err := entry.Info()
			if err != nil {
				return err
			}
			size += info.Size()
		}
		return nil
	})
	return size, err
}

func removeStoreEntry(path string) error {
	if err := filepath.WalkDir(path, func(entryPath string, entry os.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		return os.Chmod(entryPath, 0o700)
	}); err != nil && !errors.Is(err, os.ErrNotExist) {
		return fmt.Errorf("unlock store entry: %w", err)
	}
	if err := os.RemoveAll(path); err != nil {
		return fmt.Errorf("remove store entry: %w", err)
	}
	return nil
}

// ValidPluginID accepts the on-disk plugin id character set.
func ValidPluginID(id string) bool {
	if id == "" || len(id) > 128 || id == "." || id == ".." {
		return false
	}
	for _, character := range id {
		valid := character == '-' || character == '_' || character == '.' ||
			character >= 'a' && character <= 'z' ||
			character >= 'A' && character <= 'Z' ||
			character >= '0' && character <= '9'
		if !valid {
			return false
		}
	}
	return true
}

func makeTreeReadOnly(root string) error {
	return filepath.WalkDir(root, func(path string, entry os.DirEntry, err error) error {
		if err != nil {
			return err
		}
		info, err := entry.Info()
		if err != nil {
			return err
		}
		return os.Chmod(path, info.Mode().Perm()&^0o222)
	})
}
