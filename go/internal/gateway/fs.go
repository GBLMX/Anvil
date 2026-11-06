package gateway

import (
	"encoding/base64"
	"encoding/binary"
	"errors"
	"os"

	"github.com/GBLMX/Anvil/go/internal/fsroot"
	"golang.org/x/sys/unix"
)

type readParams struct {
	Path     string `json:"path"`
	Offset   int64  `json:"offset"`
	MaxBytes int64  `json:"max_bytes"`
	Binary   bool   `json:"binary"`
	Seq      uint64 `json:"seq"`
}

type binaryResult struct {
	payload []byte
}

type readResult struct {
	Data       string `json:"data"`
	NextOffset int64  `json:"next_offset"`
	EOF        bool   `json:"eof"`
}

type statParams struct {
	Path string `json:"path"`
}

type statResult struct {
	Size     int64 `json:"size"`
	Modified int64 `json:"modified_ns"`
}

type readDirParams struct {
	Path string `json:"path"`
}

type readDirResult struct {
	Entries []fsroot.DirEntry `json:"entries"`
}

type dirSignatureResult struct {
	Modified int64 `json:"modified_ns"`
	Changed  int64 `json:"changed_ns"`
}

type createDirParams struct {
	Path string `json:"path"`
}

type renameParams struct {
	From string `json:"from"`
	To   string `json:"to"`
}

type removeParams struct {
	Path string `json:"path"`
}

type copyParams struct {
	From string `json:"from"`
	To   string `json:"to"`
}

type removeAllParams struct {
	Path string `json:"path"`
}

type writeParams struct {
	Path    string `json:"path"`
	Offset  int64  `json:"offset"`
	Data    string `json:"data"`
	EOF     *bool  `json:"eof"`
	Binary  bool   `json:"binary"`
	Seq     uint64 `json:"seq"`
	DataLen int    `json:"data_len"`
}

type binaryWriteResult struct {
	payload []byte
}

type writeFileResult struct {
	NextOffset int64 `json:"next_offset"`
}

type pendingWrite struct {
	target *fsroot.WriteTarget
	offset int64
}

func (s *Server) statFile(req request, state *connectionState) (interface{}, *rpcError) {
	var params statParams
	if err := decodeParams(req.Params, &params); err != nil {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	path, err := workspacePath(params.Path)
	if err != nil {
		return nil, &rpcError{Code: -32002, Message: "policy denied"}
	}
	if rpcErr := s.pathPermissionError(state, "fs.read", path); rpcErr != nil {
		return nil, rpcErr
	}
	if s.root == nil {
		return nil, &rpcError{Code: -32008, Message: "file service unavailable"}
	}
	metadata, err := s.root.Stat(path)
	if err != nil {
		code := -32002
		if errors.Is(err, os.ErrNotExist) {
			code = -32007
		}
		return nil, &rpcError{Code: code, Message: "stat unavailable"}
	}
	return statResult{Size: metadata.Size, Modified: metadata.ModifiedNS}, nil
}

func (s *Server) readDir(req request, state *connectionState) (interface{}, *rpcError) {
	var params readDirParams
	if err := decodeParams(req.Params, &params); err != nil {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	path := params.Path
	if path == "" {
		path = "."
	}
	if path != "." {
		cleaned, err := workspacePath(path)
		if err != nil {
			return nil, &rpcError{Code: -32002, Message: "policy denied"}
		}
		path = cleaned
	}
	if rpcErr := s.pathPermissionError(state, "fs.read", path); rpcErr != nil {
		return nil, rpcErr
	}
	if s.root == nil {
		return nil, &rpcError{Code: -32008, Message: "file service unavailable"}
	}
	entries, err := s.root.ReadDir(path)
	if err != nil {
		code := -32002
		if errors.Is(err, os.ErrNotExist) {
			code = -32007
		}
		return nil, &rpcError{Code: code, Message: "read_dir unavailable: " + err.Error()}
	}
	return readDirResult{Entries: entries}, nil
}

func (s *Server) readDirSignature(req request, state *connectionState) (interface{}, *rpcError) {
	var params statParams
	if err := decodeParams(req.Params, &params); err != nil {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	path := params.Path
	if path == "" {
		path = "."
	}
	if path != "." {
		cleaned, err := workspacePath(path)
		if err != nil {
			return nil, &rpcError{Code: -32002, Message: "policy denied"}
		}
		path = cleaned
	}
	if rpcErr := s.pathPermissionError(state, "fs.read", path); rpcErr != nil {
		return nil, rpcErr
	}
	if s.root == nil {
		return nil, &rpcError{Code: -32008, Message: "file service unavailable"}
	}
	signature, err := s.root.DirSignature(path)
	if err != nil {
		code := -32002
		if errors.Is(err, os.ErrNotExist) {
			code = -32007
		}
		return nil, &rpcError{Code: code, Message: "dir signature unavailable: " + err.Error()}
	}
	return dirSignatureResult{
		Modified: signature.ModifiedNS,
		Changed:  signature.ChangedNS,
	}, nil
}

func fileMutationError(err error) *rpcError {
	code := -32008
	switch {
	case errors.Is(err, fsroot.ErrInvalidPath):
		code = -32002
	case errors.Is(err, os.ErrNotExist):
		code = -32007
	case errors.Is(err, os.ErrExist), errors.Is(err, unix.EEXIST), errors.Is(err, unix.ENOTEMPTY):
		code = -32009
	}
	return &rpcError{Code: code, Message: "file operation unavailable"}
}

func (s *Server) createDir(req request, state *connectionState) (interface{}, *rpcError) {
	var params createDirParams
	if err := decodeParams(req.Params, &params); err != nil || params.Path == "" {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	path, err := workspacePath(params.Path)
	if err != nil {
		return nil, &rpcError{Code: -32002, Message: "policy denied"}
	}
	if rpcErr := s.pathPermissionError(state, "fs.write", path); rpcErr != nil {
		return nil, rpcErr
	}
	if s.root == nil {
		return nil, &rpcError{Code: -32008, Message: "file service unavailable"}
	}
	if err := s.root.CreateDir(path); err != nil {
		return nil, fileMutationError(err)
	}
	return map[string]bool{"ok": true}, nil
}

func (s *Server) rename(req request, state *connectionState) (interface{}, *rpcError) {
	var params renameParams
	if err := decodeParams(req.Params, &params); err != nil || params.From == "" || params.To == "" {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	from, err := workspacePath(params.From)
	if err != nil {
		return nil, &rpcError{Code: -32002, Message: "policy denied"}
	}
	to, err := workspacePath(params.To)
	if err != nil {
		return nil, &rpcError{Code: -32002, Message: "policy denied"}
	}
	if rpcErr := s.pathPermissionError(state, "fs.write", from); rpcErr != nil {
		return nil, rpcErr
	}
	if rpcErr := s.pathPermissionError(state, "fs.write", to); rpcErr != nil {
		return nil, rpcErr
	}
	if s.root == nil {
		return nil, &rpcError{Code: -32008, Message: "file service unavailable"}
	}
	if err := s.root.Rename(from, to); err != nil {
		return nil, fileMutationError(err)
	}
	return map[string]bool{"ok": true}, nil
}

func (s *Server) remove(req request, state *connectionState) (interface{}, *rpcError) {
	var params removeParams
	if err := decodeParams(req.Params, &params); err != nil || params.Path == "" {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	path, err := workspacePath(params.Path)
	if err != nil {
		return nil, &rpcError{Code: -32002, Message: "policy denied"}
	}
	if rpcErr := s.pathPermissionError(state, "fs.write", path); rpcErr != nil {
		return nil, rpcErr
	}
	if s.root == nil {
		return nil, &rpcError{Code: -32008, Message: "file service unavailable"}
	}
	if err := s.root.Remove(path); err != nil {
		return nil, fileMutationError(err)
	}
	return map[string]bool{"ok": true}, nil
}

func (s *Server) copy(req request, state *connectionState) (interface{}, *rpcError) {
	var params copyParams
	if err := decodeParams(req.Params, &params); err != nil || params.From == "" || params.To == "" {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	from, err := workspacePath(params.From)
	if err != nil {
		return nil, &rpcError{Code: -32002, Message: "policy denied"}
	}
	to, err := workspacePath(params.To)
	if err != nil {
		return nil, &rpcError{Code: -32002, Message: "policy denied"}
	}
	if rpcErr := s.pathPermissionError(state, "fs.read", from); rpcErr != nil {
		return nil, rpcErr
	}
	if rpcErr := s.pathPermissionError(state, "fs.write", to); rpcErr != nil {
		return nil, rpcErr
	}
	if s.root == nil {
		return nil, &rpcError{Code: -32008, Message: "file service unavailable"}
	}
	if err := s.root.Copy(from, to); err != nil {
		return nil, fileMutationError(err)
	}
	return map[string]bool{"ok": true}, nil
}

func (s *Server) removeAll(req request, state *connectionState) (interface{}, *rpcError) {
	var params removeAllParams
	if err := decodeParams(req.Params, &params); err != nil || params.Path == "" {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	path, err := workspacePath(params.Path)
	if err != nil {
		return nil, &rpcError{Code: -32002, Message: "policy denied"}
	}
	if rpcErr := s.pathPermissionError(state, "fs.write", path); rpcErr != nil {
		return nil, rpcErr
	}
	if s.root == nil {
		return nil, &rpcError{Code: -32008, Message: "file service unavailable"}
	}
	if err := s.root.RemoveAll(path); err != nil {
		return nil, fileMutationError(err)
	}
	return map[string]bool{"ok": true}, nil
}

func (s *Server) readFile(req request, state *connectionState) (interface{}, *rpcError) {
	var params readParams
	if err := decodeParams(req.Params, &params); err != nil {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	if params.Offset < 0 {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	path, err := workspacePath(params.Path)
	if err != nil {
		return nil, &rpcError{Code: -32002, Message: "policy denied"}
	}
	limit := int64(maxReadChunk)
	if params.Binary {
		limit = maxBinaryReadChunk
	}
	max := params.MaxBytes
	if max == 0 {
		max = limit
	}
	if max < 0 || max > limit {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	if rpcErr := s.pathPermissionError(state, "fs.read", path); rpcErr != nil {
		return nil, rpcErr
	}
	if s.root == nil {
		return nil, &rpcError{Code: -32008, Message: "file service unavailable"}
	}
	data, eof, err := s.root.ReadFileAt(path, params.Offset, max)
	if err != nil {
		code := -32002
		if errors.Is(err, fsroot.ErrOffsetOutOfRange) {
			code = -32602
		} else if errors.Is(err, os.ErrNotExist) {
			code = -32007
		}
		return nil, &rpcError{Code: code, Message: "read unavailable"}
	}
	next := params.Offset + int64(len(data))
	if params.Binary {
		payload := make([]byte, 17+len(data))
		if eof {
			payload[0] = 1
		}
		binary.BigEndian.PutUint64(payload[1:9], params.Seq)
		binary.BigEndian.PutUint64(payload[9:17], uint64(params.Offset))
		copy(payload[17:], data)
		return binaryResult{payload: payload}, nil
	}
	return readResult{
		Data:       base64.StdEncoding.EncodeToString(data),
		NextOffset: next,
		EOF:        eof,
	}, nil
}

func (s *Server) writeFileBinary(req request, state *connectionState, data []byte) (interface{}, *rpcError) {
	var params writeParams
	if err := decodeParams(req.Params, &params); err != nil {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	if params.Offset < 0 || params.EOF == nil || !params.Binary || len(data) > maxBinaryWriteChunk || len(data) != params.DataLen {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	if len(data) == 0 && !*params.EOF {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	path, err := workspacePath(params.Path)
	if err != nil {
		return nil, &rpcError{Code: -32002, Message: "policy denied"}
	}
	if rpcErr := s.pathPermissionError(state, "fs.write", path); rpcErr != nil {
		return nil, rpcErr
	}
	if s.root == nil {
		return nil, &rpcError{Code: -32008, Message: "file service unavailable"}
	}
	state.writesMu.Lock()
	defer state.writesMu.Unlock()
	pending := state.writes[path]
	if pending == nil {
		if params.Offset != 0 {
			return nil, &rpcError{Code: -32009, Message: "write offset conflict"}
		}
		target, err := s.root.CreateWrite(path)
		if err != nil {
			code := -32008
			if errors.Is(err, fsroot.ErrInvalidPath) {
				code = -32002
			} else if errors.Is(err, os.ErrNotExist) {
				code = -32007
			}
			return nil, &rpcError{Code: code, Message: "write unavailable"}
		}
		pending = &pendingWrite{target: target}
		state.writes[path] = pending
	}
	if params.Offset != pending.offset {
		return nil, &rpcError{Code: -32009, Message: "write offset conflict"}
	}
	if _, err := pending.target.Write(data); err != nil {
		pending.target.Abort()
		delete(state.writes, path)
		return nil, &rpcError{Code: -32008, Message: "write unavailable"}
	}
	pending.offset += int64(len(data))
	if *params.EOF {
		if err := pending.target.Commit(); err != nil {
			delete(state.writes, path)
			return nil, &rpcError{Code: -32008, Message: "write unavailable"}
		}
		delete(state.writes, path)
	}
	payload := make([]byte, 17)
	if *params.EOF {
		payload[0] = 1
	}
	binary.BigEndian.PutUint64(payload[1:9], params.Seq)
	binary.BigEndian.PutUint64(payload[9:17], uint64(pending.offset))
	return binaryWriteResult{payload: payload}, nil
}

func (s *Server) writeFile(req request, state *connectionState) (interface{}, *rpcError) {
	var params writeParams
	if err := decodeParams(req.Params, &params); err != nil {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	if params.Offset < 0 || params.EOF == nil {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	path, err := workspacePath(params.Path)
	if err != nil {
		return nil, &rpcError{Code: -32002, Message: "policy denied"}
	}
	data, err := base64.StdEncoding.DecodeString(params.Data)
	if err != nil || len(data) > maxWriteChunk {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	if len(data) == 0 && !*params.EOF {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	if rpcErr := s.pathPermissionError(state, "fs.write", path); rpcErr != nil {
		return nil, rpcErr
	}
	if s.root == nil {
		return nil, &rpcError{Code: -32008, Message: "file service unavailable"}
	}

	state.writesMu.Lock()
	defer state.writesMu.Unlock()
	pending := state.writes[path]
	if pending == nil {
		if params.Offset != 0 {
			return nil, &rpcError{Code: -32009, Message: "write offset conflict"}
		}
		target, err := s.root.CreateWrite(path)
		if err != nil {
			code := -32008
			if errors.Is(err, fsroot.ErrInvalidPath) {
				code = -32002
			} else if errors.Is(err, os.ErrNotExist) {
				code = -32007
			}
			return nil, &rpcError{Code: code, Message: "write unavailable"}
		}
		pending = &pendingWrite{target: target}
		state.writes[path] = pending
	}
	if params.Offset != pending.offset {
		return nil, &rpcError{Code: -32009, Message: "write offset conflict"}
	}
	if _, err := pending.target.Write(data); err != nil {
		pending.target.Abort()
		delete(state.writes, path)
		return nil, &rpcError{Code: -32008, Message: "write unavailable"}
	}
	pending.offset += int64(len(data))
	if *params.EOF {
		if err := pending.target.Commit(); err != nil {
			delete(state.writes, path)
			return nil, &rpcError{Code: -32008, Message: "write unavailable"}
		}
		delete(state.writes, path)
	}
	return writeFileResult{NextOffset: pending.offset}, nil
}
