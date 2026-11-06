package gateway

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"sync"

	"github.com/GBLMX/Anvil/go/internal/fetch"
	"github.com/GBLMX/Anvil/go/internal/pkg"
)

const (
	defaultDownloadWorkers = 3
	defaultDownloadQueue   = 16
	maxSignatureBytes      = 16 << 10
)

type downloadJob struct {
	id           string
	packageURL   string
	signatureURL string
	notify       func(string, any)
}

type downloadQueue struct {
	jobs      chan downloadJob
	done      chan struct{}
	once      sync.Once
	cancel    context.CancelFunc
	fetcher   *fetch.Client
	trustPath string
	wg        sync.WaitGroup
}

func newDownloadQueue(parent context.Context, fetcher *fetch.Client, trustPath string, workers, capacity int) *downloadQueue {
	if workers <= 0 {
		workers = defaultDownloadWorkers
	}
	if capacity <= 0 {
		capacity = defaultDownloadQueue
	}
	ctx, cancel := context.WithCancel(parent)
	queue := &downloadQueue{
		jobs:      make(chan downloadJob, capacity),
		done:      make(chan struct{}),
		cancel:    cancel,
		fetcher:   fetcher,
		trustPath: trustPath,
	}
	for range workers {
		queue.wg.Add(1)
		go func() {
			defer queue.wg.Done()
			for {
				select {
				case job := <-queue.jobs:
					queue.run(ctx, job)
				case <-queue.done:
					return
				}
			}
		}()
	}
	return queue
}

func (q *downloadQueue) submit(job downloadJob) error {
	select {
	case q.jobs <- job:
		return nil
	case <-q.done:
		return fmt.Errorf("download queue is closed")
	default:
		return fmt.Errorf("download queue is full")
	}
}

func (q *downloadQueue) close() {
	q.once.Do(func() {
		q.cancel()
		close(q.done)
		q.wg.Wait()
	})
}

func (q *downloadQueue) run(ctx context.Context, job downloadJob) {
	fail := func(err error) {
		job.notify("pkg.failed", map[string]any{"job_id": job.id, "message": err.Error()})
	}
	job.notify("pkg.progress", map[string]any{
		"job_id": job.id, "stage": "package", "fraction": 0,
	})
	packageResponse, err := q.fetcher.Fetch(ctx, job.packageURL, pkg.MaxPackageBytes, nil)
	if err != nil {
		fail(err)
		return
	}
	if packageResponse.Status != 200 || packageResponse.Truncated {
		fail(fmt.Errorf("package HTTP status %d", packageResponse.Status))
		return
	}
	job.notify("pkg.progress", map[string]any{
		"job_id": job.id, "stage": "package", "fraction": 100,
	})
	job.notify("pkg.progress", map[string]any{
		"job_id": job.id, "stage": "signature", "fraction": 0,
	})
	signatureResponse, err := q.fetcher.Fetch(ctx, job.signatureURL, maxSignatureBytes, nil)
	if err != nil {
		fail(err)
		return
	}
	if signatureResponse.Status != 200 || signatureResponse.Truncated {
		fail(fmt.Errorf("signature HTTP status %d", signatureResponse.Status))
		return
	}
	job.notify("pkg.progress", map[string]any{
		"job_id": job.id, "stage": "signature", "fraction": 100,
	})
	dir, err := os.MkdirTemp("", "anvil-pkg-download-")
	if err != nil {
		fail(err)
		return
	}
	keepDir := false
	defer func() {
		if !keepDir {
			_ = os.RemoveAll(dir)
		}
	}()
	packagePath := filepath.Join(dir, "plugin.anvil-plugin")
	signaturePath := filepath.Join(dir, "plugin.sig")
	if err := os.WriteFile(packagePath, packageResponse.Body, 0o600); err != nil {
		fail(err)
		return
	}
	if err := os.WriteFile(signaturePath, signatureResponse.Body, 0o600); err != nil {
		fail(err)
		return
	}
	manifest, err := pkg.InspectArchive(packagePath)
	if err != nil {
		fail(err)
		return
	}
	if manifest.Permissions == nil {
		manifest.Permissions = []string{}
	}
	signature, publicKey, err := pkg.ReadSignatureWithKey(signaturePath)
	if err != nil {
		fail(err)
		return
	}
	trusted := false
	if publicKey != "" {
		decoded, err := pkg.DecodePublicKey(publicKey)
		if err == nil {
			store, loadErr := pkg.LoadTrustStore(q.trustPath)
			if loadErr == nil && store.Contains(decoded) {
				trusted = true
			}
		}
	}
	digest, err := pkg.DigestFile(packagePath)
	if err != nil {
		fail(err)
		return
	}
	keepDir = true
	job.notify("pkg.downloaded", map[string]any{
		"job_id":         job.id,
		"package_path":   packagePath,
		"signature_path": signaturePath,
		"manifest":       manifest,
		"public_key":     publicKey,
		"trusted":        trusted,
		"signature_len":  len(signature),
		"digest":         fmt.Sprintf("%x", digest),
	})
}
