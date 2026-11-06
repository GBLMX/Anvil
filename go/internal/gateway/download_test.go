package gateway

import (
	"context"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"path/filepath"
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
	if err := queue.submit(downloadJob{
		id:           "job",
		packageURL:   server.URL + "/plugin",
		signatureURL: server.URL + "/plugin.sig",
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
