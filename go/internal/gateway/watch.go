package gateway

import (
	"github.com/GBLMX/Anvil/go/internal/fswatch"
)

type watchStartParams struct {
	Root        string   `json:"root"`
	Exclude     []string `json:"exclude"`
	MaxDirs     int      `json:"max_dirs"`
	CrossMounts bool     `json:"cross_mounts"`
	Mode        string   `json:"mode"`
}

func (s *Server) watchStart(req request, state *connectionState) (interface{}, *rpcError) {
	if rpcErr := s.permissionError(state, "fs.read"); rpcErr != nil {
		return nil, rpcErr
	}
	if s.root == nil {
		return nil, &rpcError{Code: -32008, Message: "file service unavailable"}
	}
	var params watchStartParams
	if err := decodeParams(req.Params, &params); err != nil {
		return nil, &rpcError{Code: -32602, Message: "invalid params"}
	}
	if params.Mode == "off" || params.Mode == "poll" {
		state.stopWatch()
		return fswatch.Status{
			Root:     params.Root,
			Mode:     params.Mode,
			Coverage: "off",
			Reason:   "polling fallback",
		}, nil
	}
	rootDir := s.cfg.Workspace
	rootRel := "."
	if params.Root != "" && params.Root != "." {
		resolved, err := confinedDir(s.cfg.Workspace, params.Root)
		if err != nil {
			return nil, &rpcError{Code: -32602, Message: "invalid root"}
		}
		rootDir = resolved
		rootRel = params.Root
	}
	state.stopWatch()
	watcher, status, err := fswatch.Start(fswatch.Config{
		Root:       rootDir,
		RootRel:    rootRel,
		Exclude:    params.Exclude,
		MaxDirs:    params.MaxDirs,
		CrossMount: params.CrossMounts,
	}, func(method string, value any) {
		state.notify(method, value)
	})
	if err != nil {
		return nil, &rpcError{Code: -32008, Message: "watch start unavailable"}
	}
	state.watchMu.Lock()
	state.watch = watcher
	state.watchMu.Unlock()
	return status, nil
}

func (s *Server) watchStatus(_ request, state *connectionState) (interface{}, *rpcError) {
	state.watchMu.Lock()
	watcher := state.watch
	state.watchMu.Unlock()
	if watcher == nil {
		return fswatch.Status{
			Mode:     "off",
			Coverage: "off",
			Reason:   "not started",
		}, nil
	}
	return watcher.Status(), nil
}

func (s *Server) watchStop(_ request, state *connectionState) (interface{}, *rpcError) {
	state.stopWatch()
	return map[string]bool{"stopped": true}, nil
}
