package gateway

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"crypto/ed25519"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/GBLMX/Anvil/go/internal/pkg"
)

func testPackage(t *testing.T, dir string) (packagePath, signaturePath, publicKey, digest string) {
	t.Helper()
	var archive bytes.Buffer
	gzipWriter := gzip.NewWriter(&archive)
	tarWriter := tar.NewWriter(gzipWriter)
	body := []byte(`{"id":"demo","version":"1.0.0","entry":"main.luau","permissions":["fs.read:**"]}`)
	if err := tarWriter.WriteHeader(&tar.Header{
		Name: "plugin.json",
		Mode: 0o644,
		Size: int64(len(body)),
	}); err != nil {
		t.Fatalf("write header: %v", err)
	}
	if _, err := tarWriter.Write(body); err != nil {
		t.Fatalf("write body: %v", err)
	}
	if err := tarWriter.Close(); err != nil {
		t.Fatalf("close tar: %v", err)
	}
	if err := gzipWriter.Close(); err != nil {
		t.Fatalf("close gzip: %v", err)
	}
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("key: %v", err)
	}
	packagePath = filepath.Join(dir, "demo.anvil-plugin")
	if err := os.WriteFile(packagePath, archive.Bytes(), 0o600); err != nil {
		t.Fatalf("write package: %v", err)
	}
	fileDigest := sha256.Sum256(archive.Bytes())
	envelope, err := json.Marshal(map[string]string{
		"public_key": hex.EncodeToString(public),
		"signature":  hex.EncodeToString(ed25519.Sign(private, fileDigest[:])),
	})
	if err != nil {
		t.Fatalf("envelope: %v", err)
	}
	signaturePath = filepath.Join(dir, "demo.sig")
	if err := os.WriteFile(signaturePath, envelope, 0o600); err != nil {
		t.Fatalf("write signature: %v", err)
	}
	return packagePath, signaturePath, hex.EncodeToString(public), hex.EncodeToString(fileDigest[:])
}

func installRequest(t *testing.T, packagePath, signaturePath, publicKey, digest string) request {
	t.Helper()
	params, err := json.Marshal(pkgInstallParams{
		PackagePath:   packagePath,
		SignaturePath: signaturePath,
		PublicKey:     publicKey,
		Digest:        digest,
	})
	if err != nil {
		t.Fatalf("params: %v", err)
	}
	return request{Method: "pkg.install", Params: params}
}

func TestPackageInstallRequiresReviewedDigestAndTrust(t *testing.T) {
	root := t.TempDir()
	packagePath, signaturePath, publicKey, digest := testPackage(t, root)
	store, err := pkg.NewStore(filepath.Join(root, "store"))
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
	server := &Server{cfg: Config{Store: store, TrustStorePath: trustPath, Role: "installer"}}
	editor := &connectionState{handshaken: true, isEditor: true}

	if _, rpcErr := server.packageInstall(installRequest(t, packagePath, signaturePath, publicKey, "00"), editor); rpcErr == nil || rpcErr.Code != -32009 {
		t.Fatalf("wrong digest: got %+v", rpcErr)
	}
	if plugins, err := store.ListInstalled(); err != nil || len(plugins) != 0 {
		t.Fatalf("wrong digest state: %#v %v", plugins, err)
	}

	if _, rpcErr := server.packageInstall(installRequest(t, packagePath, signaturePath, publicKey, digest), editor); rpcErr == nil || rpcErr.Code != -32010 {
		t.Fatalf("untrusted key: got %+v", rpcErr)
	}

	decoded, err := pkg.DecodePublicKey(publicKey)
	if err != nil {
		t.Fatalf("decode key: %v", err)
	}
	if err := pkg.AddTrustedKey(trustPath, "demo", decoded); err != nil {
		t.Fatalf("trust key: %v", err)
	}
	result, rpcErr := server.packageInstall(installRequest(t, packagePath, signaturePath, publicKey, digest), editor)
	if rpcErr != nil {
		t.Fatalf("install: %+v", rpcErr)
	}
	response, ok := result.(map[string]string)
	if !ok || response["plugin_id"] != "demo" {
		t.Fatalf("install result: %#v", result)
	}
	if _, err := os.Stat(filepath.Join(response["path"], "plugin.json")); err != nil {
		t.Fatalf("installed plugin: %v", err)
	}
	plugins, err := store.ListInstalled()
	if err != nil || len(plugins) != 1 || plugins[0].Enabled {
		t.Fatalf("installed state: %#v %v", plugins, err)
	}
	server.cfg.Role = "editor"
	result, rpcErr = server.packageInstalled(request{}, editor)
	if rpcErr != nil {
		t.Fatalf("installed rpc: %+v", rpcErr)
	}
	listed, ok := result.(map[string]interface{})["plugins"].([]pkg.ManagedPlugin)
	if !ok || len(listed) != 1 || listed[0].ID != "demo" {
		t.Fatalf("installed rpc result: %#v", result)
	}
	params, err := json.Marshal(pkgSetEnabledParams{PluginID: "demo", Enabled: true})
	if err != nil {
		t.Fatalf("enable params: %v", err)
	}
	if _, rpcErr := server.packageSetEnabled(request{Params: params}, editor); rpcErr != nil {
		t.Fatalf("enable rpc: %+v", rpcErr)
	}
	plugins, err = store.ListInstalled()
	if err != nil || len(plugins) != 1 || !plugins[0].Enabled {
		t.Fatalf("enabled state: %#v %v", plugins, err)
	}
}

func TestPackageRPCsRequireEditorSession(t *testing.T) {
	server := &Server{}
	state := &connectionState{handshaken: true}
	if _, rpcErr := server.packageDownload(request{}, state); rpcErr == nil || rpcErr.Code != -32003 {
		t.Fatalf("download: got %+v", rpcErr)
	}
	if _, rpcErr := server.packageInstall(request{}, state); rpcErr == nil || rpcErr.Code != -32003 {
		t.Fatalf("install: got %+v", rpcErr)
	}
	if _, rpcErr := server.packageTrust(request{}, state); rpcErr == nil || rpcErr.Code != -32003 {
		t.Fatalf("trust: got %+v", rpcErr)
	}
}

func TestPackageRPCsRejectPluginSessions(t *testing.T) {
	socket := startGatewayAt(t, t.TempDir(), []string{"fs.read:**"})
	editor := dialGateway(t, socket)
	if response := sendRequest(t, editor, "editor-hello", "session.hello", helloRequestParams(testToken)); response.Error != nil {
		t.Fatalf("editor handshake: %+v", response.Error)
	}
	pluginToken := strings.Repeat("p", 32)
	hash := sha256.Sum256([]byte(pluginToken))
	if response := sendRequest(t, editor, "register", "plugin.register", map[string]interface{}{
		"plugin_id":    "demo",
		"token_sha256": hex.EncodeToString(hash[:]),
		"permissions":  []string{"fs.read:**"},
	}); response.Error != nil {
		t.Fatalf("register plugin: %+v", response.Error)
	}
	plugin := dialGateway(t, socket)
	if response := sendRequest(t, plugin, "plugin-hello", "session.hello", helloRequestParams(pluginToken)); response.Error != nil {
		t.Fatalf("plugin handshake: %+v", response.Error)
	}
	response := sendRequest(t, plugin, "trust", "pkg.trust", map[string]interface{}{
		"name":       "demo",
		"public_key": strings.Repeat("00", 32),
	})
	if response.Error == nil || response.Error.Code != -32003 {
		t.Fatalf("plugin package trust = %+v, want editor session error", response.Error)
	}
}

func TestPackageUninstallCleansStore(t *testing.T) {
	root := t.TempDir()
	store, err := pkg.NewStore(filepath.Join(root, "plugins"))
	if err != nil {
		t.Fatalf("new store: %v", err)
	}
	digest := strings.Repeat("ab", 32)
	entry, err := store.EntryPath(digest)
	if err != nil {
		t.Fatalf("entry: %v", err)
	}
	if err := os.MkdirAll(filepath.Join(entry, "root"), 0o700); err != nil {
		t.Fatalf("entry root: %v", err)
	}
	if err := store.AddInstalledVersion("demo", digest, "1.0.0", "", "local-file", nil); err != nil {
		t.Fatalf("add installed: %v", err)
	}
	server := &Server{cfg: Config{Store: store, Role: "editor"}}
	editor := &connectionState{handshaken: true, isEditor: true}
	params, err := json.Marshal(pkgPluginParams{PluginID: "demo"})
	if err != nil {
		t.Fatalf("params: %v", err)
	}
	if _, rpcErr := server.packageUninstall(request{Params: params}, editor); rpcErr != nil {
		t.Fatalf("uninstall: %+v", rpcErr)
	}
	if _, err := os.Stat(entry); !os.IsNotExist(err) {
		t.Fatalf("store entry still exists: %v", err)
	}
	state, err := store.ReadState()
	if err != nil || len(state.Plugins) != 0 {
		t.Fatalf("state after uninstall: %#v %v", state, err)
	}
}

func TestPackageRoleBoundaries(t *testing.T) {
	editor := &Server{cfg: Config{Role: "editor"}}
	editorState := &connectionState{handshaken: true, isEditor: true}
	if _, rpcErr := editor.packageInstall(request{}, editorState); rpcErr == nil || rpcErr.Code != -32003 {
		t.Fatalf("editor install = %+v, want role rejection", rpcErr)
	}
	if _, rpcErr := editor.packageDownload(request{}, editorState); rpcErr == nil || rpcErr.Code != -32003 {
		t.Fatalf("editor download = %+v, want role rejection", rpcErr)
	}
	installer := &Server{cfg: Config{Role: "installer"}}
	installerState := &connectionState{handshaken: true}
	if _, rpcErr := installer.packageInstalled(request{}, installerState); rpcErr == nil || rpcErr.Code != -32003 {
		t.Fatalf("installer state RPC = %+v, want role rejection", rpcErr)
	}
}
