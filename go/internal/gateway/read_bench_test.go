package gateway

import (
	"bytes"
	"encoding/base64"
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/GBLMX/Anvil/go/internal/fsroot"
)

const benchChunk = 5*1024*1024 + 512*1024

func benchWorkspace(b *testing.B) (*fsroot.Root, string) {
	b.Helper()
	dir := b.TempDir()
	data := make([]byte, 100<<20)
	if err := os.WriteFile(filepath.Join(dir, "large.txt"), data, 0o600); err != nil {
		b.Fatal(err)
	}
	root, err := fsroot.Open(dir)
	if err != nil {
		b.Fatal(err)
	}
	b.Cleanup(func() { _ = root.Close() })
	return root, dir
}

func BenchmarkReadFileChunk(b *testing.B) {
	root, _ := benchWorkspace(b)
	b.SetBytes(benchChunk)
	b.ReportAllocs()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		data, _, err := root.ReadFileAt("large.txt", 0, benchChunk)
		if err != nil {
			b.Fatal(err)
		}
		if len(data) != benchChunk {
			b.Fatalf("chunk = %d", len(data))
		}
	}
}

func BenchmarkReadResponseEncoding(b *testing.B) {
	data := make([]byte, benchChunk)
	b.SetBytes(benchChunk)
	b.ReportAllocs()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		result := readResult{
			Data:       base64.StdEncoding.EncodeToString(data),
			NextOffset: benchChunk,
			EOF:        true,
		}
		body, err := json.Marshal(response{JSONRPC: "2.0", ID: stringPtr("r1"), Result: result})
		if err != nil {
			b.Fatal(err)
		}
		var out bytes.Buffer
		if err := writeFrame(&out, body); err != nil {
			b.Fatal(err)
		}
	}
}

func BenchmarkBase64Encode(b *testing.B) {
	data := make([]byte, benchChunk)
	b.SetBytes(benchChunk)
	b.ReportAllocs()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		_ = base64.StdEncoding.EncodeToString(data)
	}
}

func BenchmarkResponseJSON(b *testing.B) {
	encoded := base64.StdEncoding.EncodeToString(make([]byte, benchChunk))
	result := readResult{Data: encoded, NextOffset: benchChunk, EOF: true}
	b.SetBytes(benchChunk)
	b.ReportAllocs()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		if _, err := json.Marshal(response{JSONRPC: "2.0", ID: stringPtr("r1"), Result: result}); err != nil {
			b.Fatal(err)
		}
	}
}
