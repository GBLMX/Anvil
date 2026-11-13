package pkg

import (
	"crypto/ed25519"
	"crypto/sha256"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
)

func testDigest(seed byte) string {
	value := make([]byte, 32)
	for index := range value {
		value[index] = seed
	}
	return fmt.Sprintf("%x", value)
}

func TestDefaultStoreRootFallback(t *testing.T) {
	root := t.TempDir()
	t.Setenv("XDG_DATA_HOME", "")
	t.Setenv("HOME", root)
	want := filepath.Join(root, ".local", "share", "anvil", "plugins")
	got, err := DefaultStoreRoot()
	if err != nil {
		t.Fatalf("default root: %v", err)
	}
	if got != want {
		t.Fatalf("DefaultStoreRoot() = %q, want %q", got, want)
	}
}

func TestStoreStatePermissionsAndAtomicUpdate(t *testing.T) {
	root := filepath.Join(t.TempDir(), "plugins")
	store, err := NewStore(root)
	if err != nil {
		t.Fatalf("new store: %v", err)
	}
	if info, err := os.Stat(root); err != nil || info.Mode().Perm() != 0o700 {
		t.Fatalf("data root mode: %v %v", info, err)
	}
	if err := store.UpdateState(func(state *StoreState) error {
		state.Plugins["demo"] = InstalledPlugin{
			ActiveDigest: testDigest(1),
			Source:       "local-file",
			Versions: []InstalledVersion{{
				Digest:  testDigest(1),
				Version: "1.0.0",
			}},
		}
		return nil
	}); err != nil {
		t.Fatalf("update state: %v", err)
	}
	for _, path := range []string{store.StatePath(), filepath.Join(root, "state.lock")} {
		info, err := os.Stat(path)
		if err != nil {
			t.Fatalf("stat %s: %v", path, err)
		}
		if info.Mode().Perm() != 0o600 {
			t.Fatalf("%s mode = %o, want 600", path, info.Mode().Perm())
		}
	}
	state, err := store.ReadState()
	if err != nil {
		t.Fatalf("read state: %v", err)
	}
	if state.Plugins["demo"].ActiveDigest != testDigest(1) {
		t.Fatalf("active digest: %#v", state.Plugins["demo"])
	}
}

func TestStoreConcurrentUpdatesDoNotLoseEntries(t *testing.T) {
	store, err := NewStore(filepath.Join(t.TempDir(), "plugins"))
	if err != nil {
		t.Fatalf("new store: %v", err)
	}
	const workers = 16
	var group sync.WaitGroup
	for index := 0; index < workers; index++ {
		group.Add(1)
		go func(index int) {
			defer group.Done()
			id := fmt.Sprintf("plugin-%d", index)
			err := store.UpdateState(func(state *StoreState) error {
				state.Plugins[id] = InstalledPlugin{
					ActiveDigest: testDigest(byte(index)),
					Source:       "local-file",
					Versions: []InstalledVersion{{
						Digest:  testDigest(byte(index)),
						Version: "1.0.0",
					}},
				}
				return nil
			})
			if err != nil {
				t.Errorf("update %d: %v", index, err)
			}
		}(index)
	}
	group.Wait()
	state, err := store.ReadState()
	if err != nil {
		t.Fatalf("read state: %v", err)
	}
	if len(state.Plugins) != workers {
		t.Fatalf("state has %d plugins, want %d", len(state.Plugins), workers)
	}
}

func TestStoreRejectsCorruptState(t *testing.T) {
	store, err := NewStore(filepath.Join(t.TempDir(), "plugins"))
	if err != nil {
		t.Fatalf("new store: %v", err)
	}
	if err := os.WriteFile(store.StatePath(), []byte("{"), 0o600); err != nil {
		t.Fatalf("write corrupt state: %v", err)
	}
	if _, err := store.ReadState(); err == nil {
		t.Fatal("corrupt state was accepted")
	}
}

func TestStoreInstallPackageIsContentAddressedAndDisabled(t *testing.T) {
	root := t.TempDir()
	store, err := NewStore(filepath.Join(root, "plugins"))
	if err != nil {
		t.Fatalf("new store: %v", err)
	}
	t.Cleanup(func() {
		_ = filepath.WalkDir(store.Root, func(path string, entry os.DirEntry, walkErr error) error {
			if walkErr == nil {
				_ = os.Chmod(path, 0o700)
			}
			return nil
		})
		_ = os.RemoveAll(store.Root)
	})
	manifestJSON := []byte("{\n  \"id\": \"demo\",\n  \"version\": \"1.0.0\",\n  \"entry\": \"main.luau\"\n}")
	archive, signature, public := signedPackage(t, root, []entry{{
		name: "plugin.json",
		data: string(manifestJSON),
	}})
	digest, err := DigestFile(archive)
	if err != nil {
		t.Fatalf("digest: %v", err)
	}
	digestHex := fmt.Sprintf("%x", digest)
	trustPath := filepath.Join(root, "trust.json")
	if err := AddTrustedKey(trustPath, "demo", public); err != nil {
		t.Fatalf("trust key: %v", err)
	}
	installed, err := store.InstallPackage(
		archive,
		signature,
		public,
		digestHex,
		trustPath,
		"local-file",
	)
	if err != nil {
		t.Fatalf("install package: %v", err)
	}
	if installed.Enabled {
		t.Fatal("install enabled the plugin")
	}
	if _, err := os.Stat(filepath.Join(installed.Root, "plugin.json")); err != nil {
		t.Fatalf("installed root: %v", err)
	}
	if _, err := os.Stat(filepath.Join(store.Root, "store", digestHex, "package.anvil-plugin")); err != nil {
		t.Fatalf("stored archive: %v", err)
	}
	again, err := store.InstallPackage(
		archive,
		signature,
		public,
		digestHex,
		trustPath,
		"local-file",
	)
	if err != nil {
		t.Fatalf("repeat install: %v", err)
	}
	if again.Digest != installed.Digest {
		t.Fatalf("repeat digest: %s != %s", again.Digest, installed.Digest)
	}
	state, err := store.ReadState()
	if err != nil {
		t.Fatalf("read state: %v", err)
	}
	if len(state.Plugins["demo"].Versions) != 1 {
		t.Fatalf("repeat install added a version: %#v", state.Plugins["demo"])
	}
	wantManifestHash := fmt.Sprintf("%x", sha256.Sum256(manifestJSON))
	if got := state.Plugins["demo"].Versions[0].ManifestHash; got != wantManifestHash {
		t.Fatalf("manifest hash = %s, want %s", got, wantManifestHash)
	}
}

func TestStoreAddInstalledVersionRequiresStoreEntry(t *testing.T) {
	store, err := NewStore(filepath.Join(t.TempDir(), "plugins"))
	if err != nil {
		t.Fatalf("new store: %v", err)
	}
	if err := store.AddInstalledVersion("demo", testDigest(1), "1.0.0", "", "local-file", nil); err == nil {
		t.Fatal("installed version without store entry was accepted")
	}
	digest := testDigest(2)
	entry, err := store.EntryPath(digest)
	if err != nil {
		t.Fatalf("entry path: %v", err)
	}
	if err := os.MkdirAll(entry, 0o700); err != nil {
		t.Fatalf("store entry: %v", err)
	}
	if err := store.AddInstalledVersion("demo", digest, "1.0.0", "", "local-file", []string{"fs.read:**"}); err != nil {
		t.Fatalf("add installed version: %v", err)
	}
	state, err := store.ReadState()
	if err != nil {
		t.Fatalf("read state: %v", err)
	}
	plugin := state.Plugins["demo"]
	if plugin.Enabled {
		t.Fatal("install enabled the plugin")
	}
	if plugin.ActiveDigest != digest || len(plugin.Versions) != 1 {
		t.Fatalf("plugin state: %#v", plugin)
	}
}

func TestStoreConcurrentInstallsPreserveState(t *testing.T) {
	root := t.TempDir()
	store, err := NewStore(filepath.Join(root, "plugins"))
	if err != nil {
		t.Fatalf("new store: %v", err)
	}
	t.Cleanup(func() {
		_ = filepath.WalkDir(store.Root, func(path string, entry os.DirEntry, walkErr error) error {
			if walkErr == nil {
				_ = os.Chmod(path, 0o700)
			}
			return nil
		})
	})
	trustPath := filepath.Join(root, "trust.json")
	type fixture struct {
		archive, signature string
		public             ed25519.PublicKey
		digest             string
	}
	fixtures := make([]fixture, 4)
	for index := range fixtures {
		dir := filepath.Join(root, fmt.Sprintf("src-%d", index))
		if err := os.MkdirAll(dir, 0o700); err != nil {
			t.Fatalf("source dir: %v", err)
		}
		archive, signature, public := signedPackage(t, dir, []entry{{
			name: "plugin.json",
			data: fmt.Sprintf(`{"id":"demo-%d","version":"1.0.0","entry":"main.luau"}`, index),
		}})
		digest, err := DigestFile(archive)
		if err != nil {
			t.Fatalf("digest: %v", err)
		}
		if err := AddTrustedKey(trustPath, fmt.Sprintf("demo-%d", index), public); err != nil {
			t.Fatalf("trust: %v", err)
		}
		fixtures[index] = fixture{archive, signature, public, fmt.Sprintf("%x", digest)}
	}
	var group sync.WaitGroup
	for _, item := range fixtures {
		group.Add(1)
		go func(item fixture) {
			defer group.Done()
			if _, err := store.InstallPackage(item.archive, item.signature, item.public, item.digest, trustPath, "local-file"); err != nil {
				t.Errorf("install: %v", err)
			}
		}(item)
	}
	group.Wait()
	plugins, err := store.ListInstalled()
	if err != nil {
		t.Fatalf("list: %v", err)
	}
	if len(plugins) != len(fixtures) {
		t.Fatalf("installed %d plugins, want %d", len(plugins), len(fixtures))
	}
}

func TestPackageUninstallCleansStore(t *testing.T) {
	store, err := NewStore(filepath.Join(t.TempDir(), "plugins"))
	if err != nil {
		t.Fatalf("new store: %v", err)
	}
	digest := testDigest(9)
	entry, err := store.EntryPath(digest)
	if err != nil {
		t.Fatalf("entry: %v", err)
	}
	if err := os.MkdirAll(filepath.Join(entry, "root"), 0o700); err != nil {
		t.Fatalf("entry root: %v", err)
	}
	if err := os.WriteFile(filepath.Join(entry, "package.anvil-plugin"), []byte("package"), 0o600); err != nil {
		t.Fatalf("entry file: %v", err)
	}
	if err := store.AddInstalledVersion("demo", digest, "1.0.0", "", "local-file", nil); err != nil {
		t.Fatalf("add installed: %v", err)
	}
	if err := store.Uninstall("demo"); err != nil {
		t.Fatalf("uninstall: %v", err)
	}
	if _, err := os.Stat(entry); !os.IsNotExist(err) {
		t.Fatalf("store entry still exists: %v", err)
	}
	state, err := store.ReadState()
	if err != nil {
		t.Fatalf("state: %v", err)
	}
	if len(state.Plugins) != 0 {
		t.Fatalf("state after uninstall: %#v", state.Plugins)
	}
}

func TestStoreRollbackSwitchesToPreviousVersion(t *testing.T) {
	store, err := NewStore(filepath.Join(t.TempDir(), "plugins"))
	if err != nil {
		t.Fatalf("new store: %v", err)
	}
	first, second := testDigest(10), testDigest(11)
	for _, digest := range []string{first, second} {
		entry, err := store.EntryPath(digest)
		if err != nil {
			t.Fatalf("entry: %v", err)
		}
		if err := os.MkdirAll(filepath.Join(entry, "root"), 0o700); err != nil {
			t.Fatalf("entry root: %v", err)
		}
		if err := store.AddInstalledVersion("demo", digest, digest[:4], "", "local-file", nil); err != nil {
			t.Fatalf("add installed: %v", err)
		}
	}
	digest, err := store.Rollback("demo")
	if err != nil {
		t.Fatalf("rollback: %v", err)
	}
	if digest != first {
		t.Fatalf("rollback digest = %s, want %s", digest, first)
	}
	state, err := store.ReadState()
	if err != nil {
		t.Fatalf("state: %v", err)
	}
	if state.Plugins["demo"].ActiveDigest != first {
		t.Fatalf("active digest: %#v", state.Plugins["demo"])
	}
}

func TestPackageLifecycleInstallEnableDisableRollbackUninstall(t *testing.T) {
	root := t.TempDir()
	store, err := NewStore(filepath.Join(root, "plugins"))
	if err != nil {
		t.Fatalf("new store: %v", err)
	}
	t.Cleanup(func() {
		_ = filepath.WalkDir(store.Root, func(path string, entry os.DirEntry, walkErr error) error {
			if walkErr == nil {
				_ = os.Chmod(path, 0o700)
			}
			return nil
		})
	})
	trustPath := filepath.Join(root, "trust.json")
	type installedVersion struct {
		archive, signature string
		public             ed25519.PublicKey
		digest             string
	}
	var versions []installedVersion
	for index, version := range []string{"1.0.0", "2.0.0"} {
		dir := filepath.Join(root, fmt.Sprintf("version-%d", index))
		if err := os.MkdirAll(dir, 0o700); err != nil {
			t.Fatalf("version dir: %v", err)
		}
		archive, signature, public := signedPackage(t, dir, []entry{{
			name: "plugin.json",
			data: fmt.Sprintf(`{"id":"demo","version":"%s","entry":"main.luau"}`, version),
		}})
		digest, err := DigestFile(archive)
		if err != nil {
			t.Fatalf("digest: %v", err)
		}
		if err := AddTrustedKey(trustPath, "demo", public); err != nil {
			t.Fatalf("trust: %v", err)
		}
		versions = append(versions, installedVersion{
			archive: archive, signature: signature, public: public,
			digest: fmt.Sprintf("%x", digest),
		})
	}
	for _, version := range versions {
		if _, err := store.InstallPackage(version.archive, version.signature, version.public, version.digest, trustPath, "local-file"); err != nil {
			t.Fatalf("install: %v", err)
		}
	}
	if err := store.SetEnabled("demo", true); err != nil {
		t.Fatalf("enable: %v", err)
	}
	if err := store.SetEnabled("demo", false); err != nil {
		t.Fatalf("disable: %v", err)
	}
	rolledBack, err := store.Rollback("demo")
	if err != nil {
		t.Fatalf("rollback: %v", err)
	}
	if rolledBack != versions[0].digest {
		t.Fatalf("rollback digest = %s, want %s", rolledBack, versions[0].digest)
	}
	if err := store.Uninstall("demo"); err != nil {
		t.Fatalf("uninstall: %v", err)
	}
	state, err := store.ReadState()
	if err != nil || len(state.Plugins) != 0 {
		t.Fatalf("state after lifecycle: %#v %v", state, err)
	}
}

func TestStorePermissionChangeDisablesUpdatedPlugin(t *testing.T) {
	store, err := NewStore(filepath.Join(t.TempDir(), "plugins"))
	if err != nil {
		t.Fatalf("new store: %v", err)
	}
	first, second := testDigest(12), testDigest(13)
	for _, digest := range []string{first, second} {
		entry, err := store.EntryPath(digest)
		if err != nil {
			t.Fatalf("entry: %v", err)
		}
		if err := os.MkdirAll(filepath.Join(entry, "root"), 0o700); err != nil {
			t.Fatalf("entry root: %v", err)
		}
	}
	if err := store.AddInstalledVersion("demo", first, "1.0.0", "", "local-file", []string{"fs.read:**"}); err != nil {
		t.Fatalf("first version: %v", err)
	}
	if err := store.SetEnabled("demo", true); err != nil {
		t.Fatalf("enable: %v", err)
	}
	if err := store.AddInstalledVersion("demo", second, "2.0.0", "", "local-file", []string{"fs.read:**", "sys.exec"}); err != nil {
		t.Fatalf("second version: %v", err)
	}
	state, err := store.ReadState()
	if err != nil {
		t.Fatalf("state: %v", err)
	}
	plugin := state.Plugins["demo"]
	if plugin.Enabled || len(plugin.GrantedPermissions) != 0 {
		t.Fatalf("permission change retained consent: %#v", plugin)
	}
}

func TestStorePermissionStableKeepsEnabledPreference(t *testing.T) {
	store, err := NewStore(filepath.Join(t.TempDir(), "plugins"))
	if err != nil {
		t.Fatalf("new store: %v", err)
	}
	first, second := testDigest(14), testDigest(15)
	for _, digest := range []string{first, second} {
		entry, err := store.EntryPath(digest)
		if err != nil {
			t.Fatalf("entry: %v", err)
		}
		if err := os.MkdirAll(filepath.Join(entry, "root"), 0o700); err != nil {
			t.Fatalf("entry root: %v", err)
		}
	}
	permissions := []string{"fs.read:**"}
	if err := store.AddInstalledVersion("demo", first, "1.0.0", "", "local-file", permissions); err != nil {
		t.Fatalf("first version: %v", err)
	}
	if err := store.SetEnabled("demo", true); err != nil {
		t.Fatalf("enable: %v", err)
	}
	if err := store.AddInstalledVersion("demo", second, "2.0.0", "", "local-file", permissions); err != nil {
		t.Fatalf("second version: %v", err)
	}
	state, err := store.ReadState()
	if err != nil {
		t.Fatalf("state: %v", err)
	}
	if !state.Plugins["demo"].Enabled {
		t.Fatalf("stable permissions lost enabled preference: %#v", state.Plugins["demo"])
	}
}

type revocationCheck map[string]bool

func (check revocationCheck) PackageRevoked(digest, pluginID string) bool {
	return check[digest+"\x00"+pluginID]
}

func (check revocationCheck) PackageDigests() map[string]string {
	digests := make(map[string]string, len(check))
	for key := range check {
		digest, pluginID, ok := strings.Cut(key, "\x00")
		if ok {
			digests[digest] = pluginID
		}
	}
	return digests
}

func TestStoreDisablesRevokedPackages(t *testing.T) {
	store, err := NewStore(t.TempDir())
	if err != nil {
		t.Fatalf("new store: %v", err)
	}
	revoked := testDigest(1)
	allowed := testDigest(2)
	if err := store.UpdateState(func(state *StoreState) error {
		for id, digest := range map[string]string{"revoked": revoked, "allowed": allowed} {
			state.Plugins[id] = InstalledPlugin{
				ActiveDigest: digest,
				Enabled:      true,
				Versions:     []InstalledVersion{{Digest: digest, Version: "1.0.0"}},
			}
		}
		return nil
	}); err != nil {
		t.Fatalf("update state: %v", err)
	}
	disabled, err := store.DisableRevokedPackages(revocationCheck{revoked + "\x00revoked": true})
	if err != nil {
		t.Fatalf("disable revoked: %v", err)
	}
	if len(disabled) != 1 || disabled[0] != "revoked" {
		t.Fatalf("unexpected disabled plugins: %v", disabled)
	}
	state, err := store.ReadState()
	if err != nil {
		t.Fatalf("read state: %v", err)
	}
	if state.Plugins["revoked"].Enabled {
		t.Fatal("revoked plugin must be disabled")
	}
	if !state.Plugins["allowed"].Enabled {
		t.Fatal("unrelated plugin must stay enabled")
	}
}

func TestStoreRejectsEnablingRevokedPackage(t *testing.T) {
	store, err := NewStore(t.TempDir())
	if err != nil {
		t.Fatalf("new store: %v", err)
	}
	digest := testDigest(3)
	if err := store.UpdateState(func(state *StoreState) error {
		state.Plugins["demo"] = InstalledPlugin{
			ActiveDigest: digest,
			Enabled:      false,
			Versions:     []InstalledVersion{{Digest: digest, Version: "1.0.0"}},
		}
		state.RevokedPackages = map[string]string{digest: "demo"}
		return nil
	}); err != nil {
		t.Fatalf("update state: %v", err)
	}
	if err := store.SetEnabled("demo", true); err == nil {
		t.Fatal("revoked package must not be enableable")
	}
	state, err := store.ReadState()
	if err != nil {
		t.Fatalf("read state: %v", err)
	}
	if state.Plugins["demo"].Enabled {
		t.Fatal("revoked package was enabled")
	}
}

func TestStorePersistsRevokedUnion(t *testing.T) {
	store, err := NewStore(t.TempDir())
	if err != nil {
		t.Fatalf("new store: %v", err)
	}
	disabledDigest := testDigest(4)
	enabledDigest := testDigest(5)
	if err := store.UpdateState(func(state *StoreState) error {
		state.Plugins["disabled"] = InstalledPlugin{
			ActiveDigest: disabledDigest,
			Enabled:      false,
			Versions:     []InstalledVersion{{Digest: disabledDigest, Version: "1.0.0"}},
		}
		state.Plugins["enabled"] = InstalledPlugin{
			ActiveDigest: enabledDigest,
			Enabled:      true,
			Versions:     []InstalledVersion{{Digest: enabledDigest, Version: "1.0.0"}},
		}
		return nil
	}); err != nil {
		t.Fatalf("update state: %v", err)
	}
	disabled, err := store.DisableRevokedPackages(revocationCheck{
		disabledDigest + "\x00disabled": true,
		enabledDigest + "\x00enabled":   true,
	})
	if err != nil {
		t.Fatalf("disable revoked: %v", err)
	}
	if len(disabled) != 1 || disabled[0] != "enabled" {
		t.Fatalf("unexpected disabled plugins: %v", disabled)
	}
	reloaded, err := NewStore(store.Root)
	if err != nil {
		t.Fatalf("reopen store: %v", err)
	}
	state, err := reloaded.ReadState()
	if err != nil {
		t.Fatalf("read state: %v", err)
	}
	if _, ok := state.RevokedPackages[disabledDigest]; !ok {
		t.Fatal("disabled revoked package was not persisted")
	}
	if _, ok := state.RevokedPackages[enabledDigest]; !ok {
		t.Fatal("enabled revoked package was not persisted")
	}
}

func TestStorePersistsRevocationsForUninstalledPackages(t *testing.T) {
	store, err := NewStore(t.TempDir())
	if err != nil {
		t.Fatalf("new store: %v", err)
	}
	digest := testDigest(6)
	if _, err := store.DisableRevokedPackages(revocationCheck{digest + "\x00future": true}); err != nil {
		t.Fatalf("persist revocations: %v", err)
	}
	state, err := store.ReadState()
	if err != nil {
		t.Fatalf("read state: %v", err)
	}
	if owner, ok := state.RevokedPackages[digest]; !ok || owner != "future" {
		t.Fatalf("revocation was not persisted: %+v", state.RevokedPackages)
	}
	entry, err := store.EntryPath(digest)
	if err != nil {
		t.Fatalf("entry path: %v", err)
	}
	if err := os.MkdirAll(entry, 0o700); err != nil {
		t.Fatalf("create store entry: %v", err)
	}
	if err := store.AddInstalledVersion("future", digest, "1.0.0", "", "local-file", nil); err == nil {
		t.Fatal("revoked package must not be installable")
	}
}

func TestStoreSweepsPersistedRevocations(t *testing.T) {
	store, err := NewStore(t.TempDir())
	if err != nil {
		t.Fatalf("new store: %v", err)
	}
	digest := testDigest(7)
	if err := store.UpdateState(func(state *StoreState) error {
		state.Plugins["demo"] = InstalledPlugin{
			ActiveDigest: digest,
			Enabled:      true,
			Versions:     []InstalledVersion{{Digest: digest, Version: "1.0.0"}},
		}
		state.RevokedPackages = map[string]string{digest: "demo"}
		return nil
	}); err != nil {
		t.Fatalf("update state: %v", err)
	}
	disabled, err := store.DisablePersistedRevoked()
	if err != nil {
		t.Fatalf("sweep revocations: %v", err)
	}
	if len(disabled) != 1 || disabled[0] != "demo" {
		t.Fatalf("unexpected disabled plugins: %v", disabled)
	}
	state, err := store.ReadState()
	if err != nil {
		t.Fatalf("read state: %v", err)
	}
	if state.Plugins["demo"].Enabled {
		t.Fatal("persisted revoked plugin must be disabled")
	}
}

func (check revocationCheck) PublisherRevoked(string) bool { return false }

func (check revocationCheck) PublisherKeyIDs() []string { return nil }

type publisherCheck struct {
	packages   map[string]string
	publishers map[string]bool
}

func (check publisherCheck) PackageRevoked(digest, pluginID string) bool {
	return check.packages[digest] == pluginID
}

func (check publisherCheck) PackageDigests() map[string]string { return check.packages }

func (check publisherCheck) PublisherRevoked(keyID string) bool { return check.publishers[keyID] }

func (check publisherCheck) PublisherKeyIDs() []string {
	keys := make([]string, 0, len(check.publishers))
	for keyID := range check.publishers {
		keys = append(keys, keyID)
	}
	return keys
}

func testPublisherKeyID(seed byte) string {
	value := make([]byte, 32)
	for index := range value {
		value[index] = seed
	}
	return "sha256:" + fmt.Sprintf("%x", value)
}

func TestStoreRejectsRevokedPublisher(t *testing.T) {
	store, err := NewStore(t.TempDir())
	if err != nil {
		t.Fatalf("new store: %v", err)
	}
	digest := testDigest(8)
	publisher := testPublisherKeyID(1)
	if err := store.UpdateState(func(state *StoreState) error {
		state.Plugins["demo"] = InstalledPlugin{
			ActiveDigest: digest,
			Enabled:      true,
			Versions: []InstalledVersion{{
				Digest:         digest,
				Version:        "1.0.0",
				PublisherKeyID: publisher,
			}},
		}
		state.RevokedPublishers = []string{publisher}
		return nil
	}); err != nil {
		t.Fatalf("update state: %v", err)
	}
	disabled, err := store.DisablePersistedRevoked()
	if err != nil {
		t.Fatalf("sweep publisher revocations: %v", err)
	}
	if len(disabled) != 1 || disabled[0] != "demo" {
		t.Fatalf("unexpected disabled plugins: %v", disabled)
	}
	if err := store.SetEnabled("demo", true); err == nil {
		t.Fatal("revoked publisher must not be enableable")
	}
}

func TestStorePersistsPublisherRevocations(t *testing.T) {
	store, err := NewStore(t.TempDir())
	if err != nil {
		t.Fatalf("new store: %v", err)
	}
	publisher := testPublisherKeyID(2)
	if _, err := store.DisableRevokedPackages(publisherCheck{
		packages:   map[string]string{},
		publishers: map[string]bool{publisher: true},
	}); err != nil {
		t.Fatalf("persist publisher revocations: %v", err)
	}
	state, err := store.ReadState()
	if err != nil {
		t.Fatalf("read state: %v", err)
	}
	if len(state.RevokedPublishers) != 1 || state.RevokedPublishers[0] != publisher {
		t.Fatalf("publisher revocation was not persisted: %+v", state.RevokedPublishers)
	}
}
