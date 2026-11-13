package gateway

import (
	"context"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"net"
	"os"
	"strings"
	"sync"
)

type helloParams struct {
	Token      string `json:"token"`
	APIVersion string `json:"api_version"`
	Protocol   string `json:"protocol"`
}

type pluginRegisterParams struct {
	PluginID    string   `json:"plugin_id"`
	TokenSHA256 string   `json:"token_sha256"`
	Permissions []string `json:"permissions"`
}

type pluginRevokeParams struct {
	PluginID string `json:"plugin_id"`
}

func (s *Server) handle(serverCtx context.Context, conn net.Conn) {
	defer conn.Close()
	uid, err := peerUID(conn)
	if err != nil || uid != uint32(os.Getuid()) {
		return
	}
	ctx, cancel := context.WithCancel(serverCtx)
	defer cancel()
	var writeMu sync.Mutex
	state := &connectionState{
		permissions: append([]string{}, s.cfg.Permissions...),
		writes:      map[string]*pendingWrite{},
		conn:        conn,
		writeMu:     &writeMu,
	}
	defer state.abortWrites()
	defer state.stopLSP()
	var requests sync.WaitGroup
	for {
		body, err := readFrame(conn)
		if err != nil {
			cancel()
			break
		}
		var req request
		if err := json.Unmarshal(body, &req); err != nil {
			writeMu.Lock()
			_ = writeError(conn, nil, -32700, "parse error")
			writeMu.Unlock()
			cancel()
			break
		}
		if req.ID == nil {
			continue
		}
		if req.JSONRPC != "2.0" || req.Method == "" {
			writeMu.Lock()
			_ = writeError(conn, req.ID, -32600, "invalid request")
			writeMu.Unlock()
			continue
		}
		if req.Method == "sys.write_file" {
			var params writeParams
			if json.Unmarshal(req.Params, &params) == nil && params.Binary {
				data, err := readFrame(conn)
				if err != nil {
					cancel()
					break
				}
				requests.Add(1)
				go func(req request, data []byte) {
					defer requests.Done()
					result, rpcErr := s.writeFileBinary(req, state, data)
					writeMu.Lock()
					defer writeMu.Unlock()
					if rpcErr != nil {
						_ = writeError(conn, req.ID, rpcErr.Code, rpcErr.Message)
						return
					}
					if binary, ok := result.(binaryWriteResult); ok {
						_ = writeFrame(conn, binary.payload)
						return
					}
				}(req, data)
				continue
			}
		}
		requests.Add(1)
		go func(req request) {
			defer requests.Done()
			result, rpcErr := s.dispatch(ctx, req, state)
			writeMu.Lock()
			defer writeMu.Unlock()
			if rpcErr != nil {
				_ = writeError(conn, req.ID, rpcErr.Code, rpcErr.Message)
				return
			}
			if binary, ok := result.(binaryResult); ok {
				_ = writeFrame(conn, binary.payload)
				return
			}
			_ = writeResult(conn, req.ID, result)
		}(req)
	}
	cancel()
	requests.Wait()
}

func (s *Server) dispatch(ctx context.Context, req request, state *connectionState) (interface{}, *rpcError) {
	switch req.Method {
	case "sys.read_file":
		return s.readFile(req, state)
	case "sys.stat":
		return s.statFile(req, state)
	case "sys.read_dir":
		return s.readDir(req, state)
	case "sys.read_dir_signature":
		return s.readDirSignature(req, state)
	case "sys.write_file":
		return s.writeFile(req, state)
	case "sys.create_dir":
		return s.createDir(req, state)
	case "sys.rename":
		return s.rename(req, state)
	case "sys.remove":
		return s.remove(req, state)
	case "sys.copy":
		return s.copy(req, state)
	case "sys.remove_all":
		return s.removeAll(req, state)
	case "plugin.register":
		return s.registerPlugin(req, state)
	case "plugin.revoke":
		return s.revokePlugin(req, state)
	case "session.hello":
		return s.hello(req, state)
	case "sys.exec":
		return s.exec(ctx, req, state)
	case "sys.exec.v2":
		return s.exec(ctx, req, state)
	case "sys.fetch":
		return s.fetch(ctx, req, state)
	case "pkg.download":
		return s.packageDownload(req, state)
	case "pkg.inspect":
		return s.packageInspect(req, state)
	case "pkg.install":
		return s.packageInstall(req, state)
	case "pkg.trust":
		return s.packageTrust(req, state)
	case "pkg.installed":
		return s.packageInstalled(req, state)
	case "pkg.set_enabled":
		return s.packageSetEnabled(req, state)
	case "pkg.uninstall":
		return s.packageUninstall(req, state)
	case "pkg.rollback":
		return s.packageRollback(req, state)
	case "lsp.start":
		return s.lspStart(ctx, req, state)
	case "lsp.send":
		return s.lspSend(req, state)
	case "lsp.stop":
		state.stopLSP()
		return map[string]bool{"stopped": true}, nil
	default:
		return nil, &rpcError{Code: -32601, Message: "method not found"}
	}
}

func (s *Server) hello(req request, state *connectionState) (interface{}, *rpcError) {
	state.mu.Lock()
	defer state.mu.Unlock()
	if state.handshaken {
		return nil, &rpcError{Code: -32600, Message: "session.hello already completed"}
	}
	var params helloParams
	if err := decodeParams(req.Params, &params); err != nil {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	if params.Protocol != Protocol || (params.APIVersion != APIVersion && params.APIVersion != "1.0") {
		return nil, &rpcError{Code: -32600, Message: "unsupported protocol"}
	}
	rawToken, err := base64.RawURLEncoding.DecodeString(strings.TrimRight(params.Token, "="))
	if err != nil || len(rawToken) != 32 {
		return nil, &rpcError{Code: -32001, Message: "unauthorized"}
	}
	hash := sha256.Sum256(rawToken)
	s.tokenMu.Lock()
	if subtle.ConstantTimeCompare(hash[:], s.cfg.TokenSHA256[:]) == 1 {
		if s.tokenUsed {
			s.tokenMu.Unlock()
			return nil, &rpcError{Code: -32001, Message: "unauthorized"}
		}
		s.tokenUsed = true
		state.isEditor = s.cfg.Role == "editor"
		s.tokenMu.Unlock()
	} else if s.cfg.LSPTokenSHA256 != [32]byte{} && subtle.ConstantTimeCompare(hash[:], s.cfg.LSPTokenSHA256[:]) == 1 {
		// The LSP token is session-bound and may be reused after a worker crash
		// so the editor can reconnect and restart rust-analyzer.
		s.tokenMu.Unlock()
	} else {
		s.tokenMu.Unlock()
		grant, ok := s.consumePluginGrant(hash)
		if !ok {
			return nil, &rpcError{Code: -32001, Message: "unauthorized"}
		}
		state.pluginID = grant.ID
		state.permissions = append([]string{}, grant.Permissions...)
	}
	state.handshaken = true
	state.sessionID = randomID()
	pluginID := s.cfg.PluginID
	if state.pluginID != "" {
		pluginID = state.pluginID
	}
	return map[string]interface{}{
		"session_id":  state.sessionID,
		"plugin_id":   pluginID,
		"api_version": APIVersion,
		"permissions": state.permissions,
	}, nil
}

func (s *Server) consumePluginGrant(hash [32]byte) (pluginGrant, bool) {
	s.pluginMu.Lock()
	defer s.pluginMu.Unlock()
	for id, grant := range s.plugins {
		if !grant.Handshaken && subtle.ConstantTimeCompare(hash[:], grant.TokenSHA256[:]) == 1 {
			grant.Handshaken = true
			s.plugins[id] = grant
			return grant, true
		}
	}
	return pluginGrant{}, false
}

func (s *Server) registerPlugin(req request, state *connectionState) (interface{}, *rpcError) {
	state.mu.Lock()
	editor := state.handshaken && state.isEditor
	state.mu.Unlock()
	if !editor {
		return nil, &rpcError{Code: -32003, Message: "editor session required"}
	}
	var params pluginRegisterParams
	if err := decodeParams(req.Params, &params); err != nil {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	pluginID := strings.TrimSpace(params.PluginID)
	if pluginID == "" || len(pluginID) > 128 {
		return nil, &rpcError{Code: -32602, Message: "invalid plugin id"}
	}
	decoded, err := hex.DecodeString(params.TokenSHA256)
	if err != nil || len(decoded) != 32 {
		return nil, &rpcError{Code: -32602, Message: "invalid token hash"}
	}
	var hash [32]byte
	copy(hash[:], decoded)
	if subtle.ConstantTimeCompare(hash[:], s.cfg.TokenSHA256[:]) == 1 ||
		(s.cfg.LSPTokenSHA256 != [32]byte{} &&
			subtle.ConstantTimeCompare(hash[:], s.cfg.LSPTokenSHA256[:]) == 1) {
		return nil, &rpcError{Code: -32002, Message: "reserved token hash"}
	}
	if len(params.Permissions) > 64 {
		return nil, &rpcError{Code: -32602, Message: "too many permissions"}
	}
	permissions := make([]string, 0, len(params.Permissions))
	for _, permission := range params.Permissions {
		if permission == "" || len(permission) > 256 {
			return nil, &rpcError{Code: -32602, Message: "invalid permission"}
		}
		permissions = append(permissions, permission)
	}
	grant := pluginGrant{
		ID:          pluginID,
		TokenSHA256: hash,
		Permissions: permissions,
	}
	s.pluginMu.Lock()
	defer s.pluginMu.Unlock()
	if _, exists := s.plugins[pluginID]; exists {
		return nil, &rpcError{Code: -32009, Message: "plugin already registered"}
	}
	for _, existing := range s.plugins {
		if subtle.ConstantTimeCompare(hash[:], existing.TokenSHA256[:]) == 1 {
			return nil, &rpcError{Code: -32009, Message: "token already registered"}
		}
	}
	s.plugins[pluginID] = grant
	return map[string]interface{}{
		"plugin_id":  pluginID,
		"registered": true,
	}, nil
}

func (s *Server) revokePlugin(req request, state *connectionState) (interface{}, *rpcError) {
	state.mu.Lock()
	editor := state.handshaken && state.isEditor
	state.mu.Unlock()
	if !editor {
		return nil, &rpcError{Code: -32003, Message: "editor session required"}
	}
	var params pluginRevokeParams
	if err := decodeParams(req.Params, &params); err != nil || params.PluginID == "" {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	s.pluginMu.Lock()
	delete(s.plugins, params.PluginID)
	s.pluginMu.Unlock()
	return map[string]interface{}{"revoked": true}, nil
}
