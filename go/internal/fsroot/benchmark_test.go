package fsroot

import (
	"fmt"
	"os"
	"path/filepath"
	"testing"
)

func benchRoot(b *testing.B) (*Root, string) {
	b.Helper()
	dir := b.TempDir()
	root, err := Open(dir)
	if err != nil {
		b.Fatalf("Open: %v", err)
	}
	b.Cleanup(func() {
		if err := root.Close(); err != nil {
			b.Errorf("Close: %v", err)
		}
	})
	return root, dir
}

func BenchmarkCopyFile(b *testing.B) {
	for _, megabytes := range []int{1, 16} {
		b.Run(fmt.Sprintf("%dMiB", megabytes), func(b *testing.B) {
			root, dir := benchRoot(b)
			size := megabytes << 20
			if err := os.WriteFile(filepath.Join(dir, "source.txt"), make([]byte, size), 0o600); err != nil {
				b.Fatal(err)
			}
			b.SetBytes(int64(size))
			b.ReportAllocs()
			b.ResetTimer()
			i := 0
			for b.Loop() {
				target := fmt.Sprintf("copy-%d.txt", i)
				i++
				if err := root.Copy("source.txt", target); err != nil {
					b.Fatalf("Copy: %v", err)
				}
				b.StopTimer()
				if err := os.Remove(filepath.Join(dir, target)); err != nil {
					b.Fatalf("remove fixture: %v", err)
				}
				b.StartTimer()
			}
		})
	}
}

func BenchmarkRenameEntry(b *testing.B) {
	root, dir := benchRoot(b)
	if err := os.WriteFile(filepath.Join(dir, "a.txt"), []byte("x"), 0o600); err != nil {
		b.Fatal(err)
	}
	from, to := "a.txt", "b.txt"
	b.ReportAllocs()
	b.ResetTimer()
	for b.Loop() {
		if err := root.Rename(from, to); err != nil {
			b.Fatalf("Rename: %v", err)
		}
		from, to = to, from
	}
}

func BenchmarkRemoveAllTree(b *testing.B) {
	root, dir := benchRoot(b)
	b.ReportAllocs()
	b.ResetTimer()
	for b.Loop() {
		b.StopTimer()
		if err := buildBenchTree(dir); err != nil {
			b.Fatal(err)
		}
		b.StartTimer()
		if err := root.RemoveAll("tree"); err != nil {
			b.Fatalf("RemoveAll: %v", err)
		}
	}
}

func buildBenchTree(root string) error {
	tree := filepath.Join(root, "tree")
	for directory := 0; directory < 8; directory++ {
		dir := filepath.Join(tree, fmt.Sprintf("dir-%d", directory))
		if err := os.MkdirAll(dir, 0o755); err != nil {
			return err
		}
		for file := 0; file < 16; file++ {
			path := filepath.Join(dir, fmt.Sprintf("file-%d.txt", file))
			if err := os.WriteFile(path, []byte("benchmark"), 0o600); err != nil {
				return err
			}
		}
	}
	return nil
}
