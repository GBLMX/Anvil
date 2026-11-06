package gateway

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/GBLMX/Anvil/go/internal/pkg"
)

type pkgDownloadParams struct {
	URL          string `json:"url"`
	SignatureURL string `json:"signature_url"`
}

type pkgInstallParams struct {
	PackagePath   string `json:"package_path"`
	SignaturePath string `json:"signature_path"`
	PublicKey     string `json:"public_key"`
	Digest        string `json:"digest"`
}

type pkgTrustParams struct {
	Name      string `json:"name"`
	PublicKey string `json:"public_key"`
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

func (s *Server) packageDownload(req request, state *connectionState) (interface{}, *rpcError) {
	if rpcErr := editorRequired(state); rpcErr != nil {
		return nil, rpcErr
	}
	if s.downloads == nil {
		return nil, &rpcError{Code: -32008, Message: "package downloader unavailable"}
	}
	var params pkgDownloadParams
	if err := decodeParams(req.Params, &params); err != nil || params.URL == "" || params.SignatureURL == "" {
		return nil, &rpcError{Code: -32602, Message: "url and signature_url are required"}
	}
	jobID := randomID()
	notify := func(method string, value any) {
		state.notify(method, value)
	}
	if err := s.downloads.submit(downloadJob{
		id:           jobID,
		packageURL:   params.URL,
		signatureURL: params.SignatureURL,
		notify:       notify,
	}); err != nil {
		return nil, &rpcError{Code: -32008, Message: err.Error()}
	}
	return map[string]string{"job_id": jobID}, nil
}

func (s *Server) packageInstall(req request, state *connectionState) (interface{}, *rpcError) {
	if rpcErr := editorRequired(state); rpcErr != nil {
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
	manifest, err := pkg.InspectArchive(params.PackagePath)
	if err != nil {
		return nil, &rpcError{Code: -32602, Message: err.Error()}
	}
	if !validPluginID(manifest.ID) {
		return nil, &rpcError{Code: -32602, Message: "invalid plugin id"}
	}
	pluginDir := s.cfg.PluginDir
	if pluginDir == "" {
		return nil, &rpcError{Code: -32008, Message: "plugin directory is not configured"}
	}
	destination := filepath.Join(pluginDir, manifest.ID)
	trustPath := s.cfg.TrustStorePath
	if trustPath == "" {
		trustPath = pkg.DefaultTrustStorePath()
	}
	digest, err := pkg.InstallTrusted(
		params.PackagePath,
		params.SignaturePath,
		publicKey,
		destination,
		trustPath,
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
		"plugin_id": manifest.ID,
		"path":      destination,
		"digest":    fmt.Sprintf("%x", digest),
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

func validPluginID(id string) bool {
	if id == "" || len(id) > 128 || id == "." || id == ".." {
		return false
	}
	for _, character := range id {
		if !(character == '-' || character == '_' || character == '.' ||
			character >= 'a' && character <= 'z' ||
			character >= 'A' && character <= 'Z' ||
			character >= '0' && character <= '9') {
			return false
		}
	}
	return true
}
