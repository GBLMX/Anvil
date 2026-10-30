package fsroot

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func testRoot(t *testing.T) (*Root, string) {
	t.Helper()
	dir := t.TempDir()
	root, err := Open(dir)
	if err != nil {
		t.Fatalf("Open: %v", err)
	}
	t.Cleanup(func() {
		if err := root.Close(); err != nil {
			t.Errorf("Close: %v", err)
		}
	})
	return root, dir
}

func TestReadFileBeneathRoot(t *testing.T) {
	root, dir := testRoot(t)
	want := []byte("hello anvil")
	if err := os.WriteFile(filepath.Join(dir, "file.txt"), want, 0o600); err != nil {
		t.Fatal(err)
	}
	got, err := root.ReadFile("file.txt", 0)
	if err != nil {
		t.Fatalf("ReadFile: %v", err)
	}
	if string(got) != string(want) {
		t.Fatalf("got %q, want %q", got, want)
	}
}

func TestRejectsTraversalAndAbsolutePaths(t *testing.T) {
	root, dir := testRoot(t)
	if err := os.WriteFile(filepath.Join(filepath.Dir(dir), "secret"), []byte("secret"), 0o600); err != nil {
		t.Fatal(err)
	}
	for _, path := range []string{"", ".", "..", "../secret", "/etc/passwd"} {
		if _, err := root.ReadFile(path, 0); err == nil {
			t.Fatalf("ReadFile(%q) unexpectedly succeeded", path)
		}
	}
}

func TestRejectsSymlinkOutsideRoot(t *testing.T) {
	root, dir := testRoot(t)
	outside := t.TempDir()
	if err := os.WriteFile(filepath.Join(outside, "secret"), []byte("secret"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(filepath.Join(outside, "secret"), filepath.Join(dir, "link")); err != nil {
		t.Fatal(err)
	}
	if _, err := root.ReadFile("link", 0); err == nil {
		t.Fatal("symlink outside the workspace was read")
	}
}

func TestReadLimitAndRegularFiles(t *testing.T) {
	root, dir := testRoot(t)
	if err := os.WriteFile(filepath.Join(dir, "large"), []byte("12345"), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := root.ReadFile("large", 4); err == nil {
		t.Fatal("oversized file was accepted")
	}
	if err := os.Mkdir(filepath.Join(dir, "sub"), 0o700); err != nil {
		t.Fatal(err)
	}
	if _, err := root.ReadFile("sub", 0); err == nil || !strings.Contains(err.Error(), "regular file") {
		t.Fatalf("directory read error = %v", err)
	}
}
