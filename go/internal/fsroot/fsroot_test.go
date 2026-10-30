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

func TestReadFileAtChunks(t *testing.T) {
	root, dir := testRoot(t)
	if err := os.WriteFile(filepath.Join(dir, "file.txt"), []byte("0123456789"), 0o600); err != nil {
		t.Fatal(err)
	}
	first, eof, err := root.ReadFileAt("file.txt", 0, 4)
	if err != nil {
		t.Fatal(err)
	}
	if string(first) != "0123" || eof {
		t.Fatalf("first chunk = %q, eof=%v", first, eof)
	}
	last, eof, err := root.ReadFileAt("file.txt", 8, 4)
	if err != nil {
		t.Fatal(err)
	}
	if string(last) != "89" || !eof {
		t.Fatalf("last chunk = %q, eof=%v", last, eof)
	}
	empty, eof, err := root.ReadFileAt("file.txt", 10, 4)
	if err != nil {
		t.Fatal(err)
	}
	if len(empty) != 0 || !eof {
		t.Fatalf("end chunk = %q, eof=%v", empty, eof)
	}
	if _, _, err := root.ReadFileAt("file.txt", 11, 4); err == nil {
		t.Fatal("offset beyond EOF was accepted")
	}
}

func TestWriteFileAtomicReplacement(t *testing.T) {
	root, dir := testRoot(t)
	target := filepath.Join(dir, "file.txt")
	if err := os.WriteFile(target, []byte("old"), 0o600); err != nil {
		t.Fatal(err)
	}
	write, err := root.CreateWrite("file.txt")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := write.Write([]byte("new ")); err != nil {
		write.Abort()
		t.Fatal(err)
	}
	if _, err := write.Write([]byte("content")); err != nil {
		write.Abort()
		t.Fatal(err)
	}
	if err := write.Commit(); err != nil {
		t.Fatal(err)
	}
	got, err := os.ReadFile(target)
	if err != nil {
		t.Fatal(err)
	}
	if string(got) != "new content" {
		t.Fatalf("got %q", got)
	}
	entries, err := os.ReadDir(dir)
	if err != nil {
		t.Fatal(err)
	}
	for _, entry := range entries {
		if strings.HasPrefix(entry.Name(), writeTempPrefix) {
			t.Fatalf("temporary file left behind: %s", entry.Name())
		}
	}
}

func TestWriteAbortLeavesDestination(t *testing.T) {
	root, dir := testRoot(t)
	target := filepath.Join(dir, "file.txt")
	if err := os.WriteFile(target, []byte("old"), 0o600); err != nil {
		t.Fatal(err)
	}
	write, err := root.CreateWrite("file.txt")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := write.Write([]byte("discarded")); err != nil {
		t.Fatal(err)
	}
	write.Abort()
	got, err := os.ReadFile(target)
	if err != nil {
		t.Fatal(err)
	}
	if string(got) != "old" {
		t.Fatalf("abort changed destination to %q", got)
	}
}

func TestWriteRejectsSymlinkedParent(t *testing.T) {
	root, dir := testRoot(t)
	outside := t.TempDir()
	if err := os.Symlink(outside, filepath.Join(dir, "link")); err != nil {
		t.Fatal(err)
	}
	if _, err := root.CreateWrite("link/file.txt"); err == nil {
		t.Fatal("write through symlink outside the workspace was accepted")
	}
}

func TestWriteRejectsMissingParent(t *testing.T) {
	root, _ := testRoot(t)
	if _, err := root.CreateWrite("missing/file.txt"); err == nil {
		t.Fatal("write with missing parent was accepted")
	}
}

func TestWriteFileInSubdirectory(t *testing.T) {
	root, dir := testRoot(t)
	if err := os.Mkdir(filepath.Join(dir, "sub"), 0o700); err != nil {
		t.Fatal(err)
	}
	write, err := root.CreateWrite("sub/file.txt")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := write.Write([]byte("nested")); err != nil {
		write.Abort()
		t.Fatal(err)
	}
	if err := write.Commit(); err != nil {
		t.Fatal(err)
	}
	got, err := os.ReadFile(filepath.Join(dir, "sub", "file.txt"))
	if err != nil {
		t.Fatal(err)
	}
	if string(got) != "nested" {
		t.Fatalf("file = %q", got)
	}
}
