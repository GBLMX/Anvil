package gateway

import (
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/GBLMX/Anvil/go/internal/pkg"
)

type pkgDownloadParams struct {
	URL          string   `json:"url"`
	SignatureURL string   `json:"signature_url"`
	Digest       string   `json:"digest,omitempty"`
	PackageSize  int64    `json:"package_size,omitempty"`
	PublicKey    string   `json:"public_key,omitempty"`
	ManifestHash string   `json:"manifest_hash,omitempty"`
	Permissions  []string `json:"permissions,omitempty"`
}

type pkgInstallParams struct {
	PackagePath   string `json:"package_path"`
	SignaturePath string `json:"signature_path"`
	PublicKey     string `json:"public_key"`
	Digest        string `json:"digest"`
}

type pkgInspectParams struct {
	PackagePath   string `json:"package_path"`
	SignaturePath string `json:"signature_path"`
	PublicKey     string `json:"public_key"`
}

type pkgTrustParams struct {
	Name      string `json:"name"`
	PublicKey string `json:"public_key"`
}

type pkgSetEnabledParams struct {
	PluginID string `json:"plugin_id"`
	Enabled  bool   `json:"enabled"`
}

type pkgPluginParams struct {
	PluginID string `json:"plugin_id"`
}

func (s *Server) requireRole(state *connectionState, role string) *rpcError {
	state.mu.Lock()
	authorized := state.handshaken && s.cfg.Role == role
	state.mu.Unlock()
	if !authorized {
		return &rpcError{Code: -32003, Message: role + " session required"}
	}
	return nil
}

func (state *connectionState) notify(method string, params any) {
	body, err := json.Marshal(map[string]any{
		"jsonrpc": "2.0",
		"method":  method,
		"params":  params,
	})
	if err != nil {
		return
	}
	state.writeMu.Lock()
	defer state.writeMu.Unlock()
	_ = writeFrame(state.conn, body)
}
func editorRequired(state *connectionState) *rpcError {
	state.mu.Lock()
	editor := state.handshaken && state.isEditor
	state.mu.Unlock()
	if !editor {
		return &rpcError{Code: -32003, Message: "editor session required"}
	}
	return nil
}

func validDigest(value string) bool {
	raw, err := hex.DecodeString(value)
	return err == nil && len(raw) == 32 && value == strings.ToLower(value)
}

func (s *Server) packageDownload(req request, state *connectionState) (interface{}, *rpcError) {
	if rpcErr := s.requireRole(state, "installer"); rpcErr != nil {
		return nil, rpcErr
	}
	downloader, err := s.installer.packageDownloader()
	if err != nil {
		return nil, &rpcError{Code: -32008, Message: err.Error()}
	}
	var params pkgDownloadParams
	if err := decodeParams(req.Params, &params); err != nil || params.URL == "" || params.SignatureURL == "" {
		return nil, &rpcError{Code: -32602, Message: "url and signature_url are required"}
	}
	if params.Digest != "" && !validDigest(params.Digest) {
		return nil, &rpcError{Code: -32602, Message: "digest must be 32 lowercase hex bytes"}
	}
	if params.ManifestHash != "" && !validDigest(params.ManifestHash) {
		return nil, &rpcError{Code: -32602, Message: "manifest_hash must be 32 lowercase hex bytes"}
	}
	if params.PackageSize < 0 || params.PackageSize > pkg.MaxPackageBytes {
		return nil, &rpcError{Code: -32602, Message: "package_size is out of bounds"}
	}
	if params.PublicKey != "" {
		if _, err := pkg.DecodePublicKey(params.PublicKey); err != nil {
			return nil, &rpcError{Code: -32602, Message: err.Error()}
		}
	}
	indexed := params.Digest != "" || params.PackageSize != 0 || params.PublicKey != "" || params.ManifestHash != "" || params.Permissions != nil
	if indexed && (params.Digest == "" || params.PackageSize <= 0 || params.PublicKey == "") {
		return nil, &rpcError{Code: -32602, Message: "digest, package_size, and public_key are required for indexed downloads"}
	}
	jobID := randomID()
	notify := func(method string, value any) {
		state.notify(method, value)
	}
	if err := downloader.submit(downloadJob{
		id:             jobID,
		packageURL:     params.URL,
		signatureURL:   params.SignatureURL,
		expectedDigest: params.Digest,
		expectedSize:   params.PackageSize,
		publisherKey:   params.PublicKey,
		manifestHash:   params.ManifestHash,
		permissions:    params.Permissions,
		notify:         notify,
	}); err != nil {
		return nil, &rpcError{Code: -32008, Message: err.Error()}
	}
	return map[string]string{"job_id": jobID}, nil
}

func (s *Server) packageInstall(req request, state *connectionState) (interface{}, *rpcError) {
	if rpcErr := s.requireRole(state, "installer"); rpcErr != nil {
		return nil, rpcErr
	}
	var params pkgInstallParams
	if err := decodeParams(req.Params, &params); err != nil ||
		params.PackagePath == "" || params.SignaturePath == "" ||
		params.PublicKey == "" || params.Digest == "" {
		return nil, &rpcError{Code: -32602, Message: "package_path, signature_path, public_key, and digest are required"}
	}
	publicKey, err := pkg.DecodePublicKey(params.PublicKey)
	if err != nil {
		return nil, &rpcError{Code: -32602, Message: err.Error()}
	}
	actualDigest, err := pkg.DigestFile(params.PackagePath)
	if err != nil {
		return nil, &rpcError{Code: -32602, Message: err.Error()}
	}
	if !strings.EqualFold(fmt.Sprintf("%x", actualDigest), params.Digest) {
		return nil, &rpcError{Code: -32009, Message: "package digest does not match the reviewed archive"}
	}
	if s.cfg.Store == nil {
		return nil, &rpcError{Code: -32008, Message: "plugin store is not configured"}
	}
	trustPath := s.cfg.TrustStorePath
	if trustPath == "" {
		trustPath = pkg.DefaultTrustStorePath()
	}
	installed, err := s.cfg.Store.InstallPackage(
		params.PackagePath,
		params.SignaturePath,
		publicKey,
		params.Digest,
		trustPath,
		"local-file",
	)
	if err != nil {
		if errors.Is(err, pkg.ErrUntrustedKey) {
			return nil, &rpcError{Code: -32010, Message: err.Error()}
		}
		return nil, &rpcError{Code: -32009, Message: err.Error()}
	}
	if strings.HasPrefix(filepath.Base(filepath.Dir(params.PackagePath)), "anvil-pkg-download-") {
		_ = os.RemoveAll(filepath.Dir(params.PackagePath))
	}
	return map[string]string{
		"plugin_id": installed.ID,
		"path":      installed.Root,
		"digest":    installed.Digest,
	}, nil
}

func (s *Server) packageInspect(req request, state *connectionState) (interface{}, *rpcError) {
	if rpcErr := s.requireRole(state, "installer"); rpcErr != nil {
		return nil, rpcErr
	}
	var params pkgInspectParams
	if err := decodeParams(req.Params, &params); err != nil ||
		params.PackagePath == "" || params.SignaturePath == "" || params.PublicKey == "" {
		return nil, &rpcError{Code: -32602, Message: "package_path, signature_path, and public_key are required"}
	}
	publicKey, err := pkg.DecodePublicKey(params.PublicKey)
	if err != nil {
		return nil, &rpcError{Code: -32602, Message: err.Error()}
	}
	digest, err := pkg.VerifyFile(params.PackagePath, params.SignaturePath, publicKey)
	if err != nil {
		return nil, &rpcError{Code: -32009, Message: err.Error()}
	}
	manifest, err := pkg.InspectArchive(params.PackagePath)
	if err != nil {
		return nil, &rpcError{Code: -32009, Message: err.Error()}
	}
	if manifest.Permissions == nil {
		manifest.Permissions = []string{}
	}
	trustPath := s.cfg.TrustStorePath
	if trustPath == "" {
		trustPath = pkg.DefaultTrustStorePath()
	}
	trusted := false
	if store, loadErr := pkg.LoadTrustStore(trustPath); loadErr == nil {
		trusted = store.Contains(publicKey)
	}
	return map[string]interface{}{
		"package_path":   params.PackagePath,
		"signature_path": params.SignaturePath,
		"public_key":     params.PublicKey,
		"manifest":       manifest,
		"digest":         fmt.Sprintf("%x", digest),
		"trusted":        trusted,
	}, nil
}

func (s *Server) packageTrust(req request, state *connectionState) (interface{}, *rpcError) {
	if rpcErr := editorRequired(state); rpcErr != nil {
		return nil, rpcErr
	}
	var params pkgTrustParams
	if err := decodeParams(req.Params, &params); err != nil || params.PublicKey == "" {
		return nil, &rpcError{Code: -32602, Message: "public_key is required"}
	}
	publicKey, err := pkg.DecodePublicKey(params.PublicKey)
	if err != nil {
		return nil, &rpcError{Code: -32602, Message: err.Error()}
	}
	trustPath := s.cfg.TrustStorePath
	if trustPath == "" {
		trustPath = pkg.DefaultTrustStorePath()
	}
	if err := pkg.AddTrustedKey(trustPath, params.Name, publicKey); err != nil {
		return nil, &rpcError{Code: -32009, Message: err.Error()}
	}
	return map[string]bool{"trusted": true}, nil
}

func (s *Server) packageInstalled(_ request, state *connectionState) (interface{}, *rpcError) {
	if rpcErr := editorRequired(state); rpcErr != nil {
		return nil, rpcErr
	}
	if s.cfg.Store == nil {
		return nil, &rpcError{Code: -32008, Message: "plugin store is not configured"}
	}
	plugins, err := s.cfg.Store.ListInstalled()
	if err != nil {
		return nil, &rpcError{Code: -32009, Message: err.Error()}
	}
	size, err := s.cfg.Store.StoreSize()
	if err != nil {
		return nil, &rpcError{Code: -32009, Message: err.Error()}
	}
	return map[string]interface{}{"plugins": plugins, "store_size": size}, nil
}

func (s *Server) packageSetEnabled(req request, state *connectionState) (interface{}, *rpcError) {
	if rpcErr := editorRequired(state); rpcErr != nil {
		return nil, rpcErr
	}
	if s.cfg.Store == nil {
		return nil, &rpcError{Code: -32008, Message: "plugin store is not configured"}
	}
	var params pkgSetEnabledParams
	if err := decodeParams(req.Params, &params); err != nil || params.PluginID == "" {
		return nil, &rpcError{Code: -32602, Message: "plugin_id is required"}
	}
	if err := s.cfg.Store.SetEnabled(params.PluginID, params.Enabled); err != nil {
		return nil, &rpcError{Code: -32009, Message: err.Error()}
	}
	return map[string]bool{"enabled": params.Enabled}, nil
}

func (s *Server) packageUninstall(req request, state *connectionState) (interface{}, *rpcError) {
	if rpcErr := editorRequired(state); rpcErr != nil {
		return nil, rpcErr
	}
	if s.cfg.Store == nil {
		return nil, &rpcError{Code: -32008, Message: "plugin store is not configured"}
	}
	var params pkgPluginParams
	if err := decodeParams(req.Params, &params); err != nil || params.PluginID == "" {
		return nil, &rpcError{Code: -32602, Message: "plugin_id is required"}
	}
	if err := s.cfg.Store.Uninstall(params.PluginID); err != nil {
		return nil, &rpcError{Code: -32009, Message: err.Error()}
	}
	return map[string]bool{"uninstalled": true}, nil
}

func (s *Server) packageRollback(req request, state *connectionState) (interface{}, *rpcError) {
	if rpcErr := editorRequired(state); rpcErr != nil {
		return nil, rpcErr
	}
	if s.cfg.Store == nil {
		return nil, &rpcError{Code: -32008, Message: "plugin store is not configured"}
	}
	var params pkgPluginParams
	if err := decodeParams(req.Params, &params); err != nil || params.PluginID == "" {
		return nil, &rpcError{Code: -32602, Message: "plugin_id is required"}
	}
	digest, err := s.cfg.Store.Rollback(params.PluginID)
	if err != nil {
		return nil, &rpcError{Code: -32009, Message: err.Error()}
	}
	return map[string]string{"digest": digest}, nil
}
