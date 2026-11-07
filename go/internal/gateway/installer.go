package gateway

import (
	"context"
	"fmt"
)

// installer owns the long-running package download queue. The store and RPC
// handlers are separate so downloading never runs inside the RPC loop.
type installer struct {
	downloads *downloadQueue
}

func newInstaller(cfg Config) *installer {
	if cfg.PackageFetcher == nil {
		return &installer{}
	}
	return &installer{
		downloads: newDownloadQueue(
			context.Background(),
			cfg.PackageFetcher,
			cfg.TrustStorePath,
			cfg.DownloadWorkers,
			cfg.DownloadQueue,
		),
	}
}

func (i *installer) close() {
	if i != nil && i.downloads != nil {
		i.downloads.close()
	}
}

func (i *installer) packageDownloader() (*downloadQueue, error) {
	if i == nil || i.downloads == nil {
		return nil, fmt.Errorf("package downloader unavailable")
	}
	return i.downloads, nil
}
