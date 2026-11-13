package gateway

import (
	"context"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/GBLMX/Anvil/go/internal/fetch"
	"github.com/GBLMX/Anvil/go/internal/pkg"
)

func TestDownloadQueueReportsSignedPackage(t *testing.T) {
	root := t.TempDir()
	packagePath, signaturePath, publicKey, digest := testPackage(t, root)
	trustPath := filepath.Join(root, "trust.json")
	decoded, err := pkg.DecodePublicKey(publicKey)
	if err != nil {
		t.Fatalf("decode key: %v", err)
	}
	if err := pkg.AddTrustedKey(trustPath, "demo", decoded); err != nil {
		t.Fatalf("trust key: %v", err)
	}
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		switch request.URL.Path {
		case "/plugin":
			http.ServeFile(writer, request, packagePath)
		case "/plugin.sig":
			http.ServeFile(writer, request, signaturePath)
		default:
			http.NotFound(writer, request)
		}
	}))
	defer server.Close()
	parsed, err := url.Parse(server.URL)
	if err != nil {
		t.Fatalf("parse server URL: %v", err)
	}
	client, err := fetch.New(fetch.Config{
		AllowedHosts:  []string{parsed.Hostname()},
		MaxBody:       pkg.MaxPackageBytes,
		Timeout:       5 * time.Second,
		AllowHTTP:     true,
		AllowLoopback: true,
		AllowAnyPort:  true,
	})
	if err != nil {
		t.Fatalf("fetch client: %v", err)
	}
	queue := newDownloadQueue(context.Background(), client, trustPath, 1, 1)
	defer queue.close()
	notifications := make(chan struct {
		method string
		params map[string]any
	}, 8)
	manifest, err := pkg.InspectArchive(packagePath)
	if err != nil {
		t.Fatalf("inspect package: %v", err)
	}
	info, err := os.Stat(packagePath)
	if err != nil {
		t.Fatalf("stat package: %v", err)
	}
	if err := queue.submit(downloadJob{
		id:             "job",
		packageURL:     server.URL + "/plugin",
		signatureURL:   server.URL + "/plugin.sig",
		expectedDigest: digest,
		expectedSize:   info.Size(),
		publisherKey:   publicKey,
		manifestHash:   manifest.ManifestHash,
		permissions:    manifest.Permissions,
		notify: func(method string, params any) {
			if values, ok := params.(map[string]any); ok && method != "pkg.progress" {
				notifications <- struct {
					method string
					params map[string]any
				}{method, values}
			}
		},
	}); err != nil {
		t.Fatalf("submit: %v", err)
	}
	select {
	case notification := <-notifications:
		if notification.method == "pkg.failed" {
			t.Fatalf("download failed: %#v", notification.params)
		}
		if notification.method != "pkg.downloaded" {
			t.Fatalf("unexpected notification: %s %#v", notification.method, notification.params)
		}
		params := notification.params
		if params["digest"] != digest {
			t.Fatalf("digest: %#v != %s", params["digest"], digest)
		}
		if params["trusted"] != true {
			t.Fatalf("trusted: %#v", params["trusted"])
		}
		downloaded := params["package_path"].(string)
		if _, err := os.Stat(downloaded); err != nil {
			t.Fatalf("downloaded package: %v", err)
		}
		if err := os.RemoveAll(filepath.Dir(downloaded)); err != nil {
			t.Fatalf("cleanup: %v", err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("download timed out")
	}
}

func TestDownloadQueueRejectsIndexedMetadataMismatch(t *testing.T) {
	root := t.TempDir()
	packagePath, signaturePath, publicKey, digest := testPackage(t, root)
	manifest, err := pkg.InspectArchive(packagePath)
	if err != nil {
		t.Fatalf("inspect package: %v", err)
	}
	info, err := os.Stat(packagePath)
	if err != nil {
		t.Fatalf("stat package: %v", err)
	}
	trustPath := filepath.Join(root, "trust.json")
	decoded, err := pkg.DecodePublicKey(publicKey)
	if err != nil {
		t.Fatalf("decode key: %v", err)
	}
	if err := pkg.AddTrustedKey(trustPath, "demo", decoded); err != nil {
		t.Fatalf("trust key: %v", err)
	}
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		switch request.URL.Path {
		case "/plugin":
			http.ServeFile(writer, request, packagePath)
		case "/plugin.sig":
			http.ServeFile(writer, request, signaturePath)
		default:
			http.NotFound(writer, request)
		}
	}))
	defer server.Close()
	parsed, err := url.Parse(server.URL)
	if err != nil {
		t.Fatalf("parse server URL: %v", err)
	}
	client, err := fetch.New(fetch.Config{
		AllowedHosts:  []string{parsed.Hostname()},
		MaxBody:       pkg.MaxPackageBytes,
		Timeout:       5 * time.Second,
		AllowHTTP:     true,
		AllowLoopback: true,
		AllowAnyPort:  true,
	})
	if err != nil {
		t.Fatalf("fetch client: %v", err)
	}
	cases := []struct {
		name string
		job  downloadJob
	}{
		{name: "digest", job: downloadJob{expectedDigest: strings.Repeat("00", 32)}},
		{name: "size", job: downloadJob{expectedSize: info.Size() + 1}},
		{name: "publisher", job: downloadJob{publisherKey: strings.Repeat("00", 32)}},
		{name: "manifest", job: downloadJob{manifestHash: strings.Repeat("00", 32)}},
		{name: "permissions", job: downloadJob{permissions: []string{"fs.write:**"}}},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			queue := newDownloadQueue(context.Background(), client, trustPath, 1, 1)
			defer queue.close()
			notifications := make(chan string, 1)
			job := test.job
			job.id = test.name
			job.packageURL = server.URL + "/plugin"
			job.signatureURL = server.URL + "/plugin.sig"
			job.notify = func(method string, params any) {
				if method == "pkg.failed" || method == "pkg.downloaded" {
					notifications <- method
					if method == "pkg.downloaded" {
						if values, ok := params.(map[string]any); ok {
							if downloaded, ok := values["package_path"].(string); ok {
								_ = os.RemoveAll(filepath.Dir(downloaded))
							}
						}
					}
				}
			}
			base := test.job
			if base.expectedDigest == "" {
				job.expectedDigest = digest
			}
			if base.expectedSize == 0 {
				job.expectedSize = info.Size()
			}
			if base.publisherKey == "" {
				job.publisherKey = publicKey
			}
			if base.manifestHash == "" {
				job.manifestHash = manifest.ManifestHash
			}
			if base.permissions == nil {
				job.permissions = manifest.Permissions
			}
			if err := queue.submit(job); err != nil {
				t.Fatalf("submit: %v", err)
			}
			select {
			case method := <-notifications:
				if method != "pkg.failed" {
					t.Fatalf("mismatched %s metadata was accepted", test.name)
				}
			case <-time.After(5 * time.Second):
				t.Fatal("download timed out")
			}
		})
	}
}
