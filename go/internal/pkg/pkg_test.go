package pkg

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"crypto/ed25519"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"os"
	"path/filepath"
	"testing"
)

type entry struct {
	name string
	data string
	mode int64
	typ  byte
}

func archiveBytes(t *testing.T, entries []entry) []byte {
	t.Helper()
	var buffer bytes.Buffer
	gzipWriter := gzip.NewWriter(&buffer)
	tarWriter := tar.NewWriter(gzipWriter)
	for _, item := range entries {
		mode := item.mode
		if mode == 0 {
			mode = 0o644
		}
		typ := item.typ
		if typ == 0 {
			typ = tar.TypeReg
		}
		header := &tar.Header{
			Name:     item.name,
			Mode:     int64(mode),
			Size:     int64(len(item.data)),
			Typeflag: typ,
		}
		if typ == tar.TypeDir {
			header.Size = 0
		}
		if err := tarWriter.WriteHeader(header); err != nil {
			t.Fatalf("write header: %v", err)
		}
		if typ == tar.TypeReg || typ == tar.TypeRegA {
			if _, err := tarWriter.Write([]byte(item.data)); err != nil {
				t.Fatalf("write body: %v", err)
			}
		}
	}
	if err := tarWriter.Close(); err != nil {
		t.Fatalf("close tar: %v", err)
	}
	if err := gzipWriter.Close(); err != nil {
		t.Fatalf("close gzip: %v", err)
	}
	return buffer.Bytes()
}

func writeFile(t *testing.T, path string, data []byte) {
	t.Helper()
	if err := os.WriteFile(path, data, 0o600); err != nil {
		t.Fatalf("write %s: %v", path, err)
	}
}

func signedPackage(t *testing.T, dir string, entries []entry) (archive, signature string, public ed25519.PublicKey) {
	t.Helper()
	public, private, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("key: %v", err)
	}
	data := archiveBytes(t, entries)
	archive = filepath.Join(dir, "demo.anvil-plugin")
	signature = filepath.Join(dir, "demo.sig")
	writeFile(t, archive, data)
	digest := sha256.Sum256(data)
	writeFile(t, signature, ed25519.Sign(private, digest[:]))
	return archive, signature, public
}

func TestInstallVerifiesAndExtracts(t *testing.T) {
	root := t.TempDir()
	archive, signature, public := signedPackage(t, root, []entry{
		{name: "plugin.json", data: `{"id":"demo","version":"1.0.0","entry":"main.luau"}`},
		{name: "main.luau", data: `return 42`},
		{name: "nested", typ: tar.TypeDir},
		{name: "nested/data.txt", data: "data"},
	})
	destination := filepath.Join(root, "installed")
	digest, err := Install(archive, signature, public, destination)
	if err != nil {
		t.Fatalf("install: %v", err)
	}
	if digest != sha256.Sum256([]byte(mustRead(t, archive))) {
		t.Fatal("digest mismatch")
	}
	if got := string(mustRead(t, filepath.Join(destination, "main.luau"))); got != "return 42" {
		t.Fatalf("main.luau = %q", got)
	}
	if got := string(mustRead(t, filepath.Join(destination, "nested", "data.txt"))); got != "data" {
		t.Fatalf("nested/data.txt = %q", got)
	}
	if _, err := os.Stat(destination); err != nil {
		t.Fatalf("destination missing: %v", err)
	}
}

func TestInstallRejectsInvalidSignatureWithoutExtraction(t *testing.T) {
	root := t.TempDir()
	archive, signature, public := signedPackage(t, root, []entry{
		{name: "plugin.json", data: `{"id":"demo"}`},
	})
	writeFile(t, signature, bytes.Repeat([]byte{0x42}, ed25519.SignatureSize))
	destination := filepath.Join(root, "installed")
	if _, err := Install(archive, signature, public, destination); err == nil {
		t.Fatal("invalid signature accepted")
	}
	if _, err := os.Stat(destination); !os.IsNotExist(err) {
		t.Fatalf("destination was created after rejected signature: %v", err)
	}
}

func TestInstallRejectsTraversalAndSymlink(t *testing.T) {
	for _, test := range []struct {
		name  string
		entry entry
	}{
		{name: "traversal", entry: entry{name: "../escape", data: "x"}},
		{name: "absolute", entry: entry{name: "/tmp/escape", data: "x"}},
		{name: "symlink", entry: entry{name: "link", data: "/etc/passwd", typ: tar.TypeSymlink}},
	} {
		t.Run(test.name, func(t *testing.T) {
			root := t.TempDir()
			archive, signature, public := signedPackage(t, root, []entry{test.entry})
			destination := filepath.Join(root, "installed")
			if _, err := Install(archive, signature, public, destination); err == nil {
				t.Fatal("unsafe archive accepted")
			}
			if _, err := os.Stat(destination); !os.IsNotExist(err) {
				t.Fatalf("destination was created after rejected archive: %v", err)
			}
		})
	}
}

func TestDecodePublicKeyHexAndBase64(t *testing.T) {
	public, _, err := ed25519.GenerateKey(rand.Reader)
	if err != nil {
		t.Fatalf("key: %v", err)
	}
	for _, encoded := range []string{hex.EncodeToString(public), base64.StdEncoding.EncodeToString(public)} {
		decoded, err := DecodePublicKey(encoded)
		if err != nil {
			t.Fatalf("decode %q: %v", encoded, err)
		}
		if !bytes.Equal(decoded, public) {
			t.Fatal("decoded key mismatch")
		}
	}
}

func mustRead(t *testing.T, path string) []byte {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read %s: %v", path, err)
	}
	return data
}
