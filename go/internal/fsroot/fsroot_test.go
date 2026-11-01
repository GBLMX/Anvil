package fsroot

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"golang.org/x/sys/unix"
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

func TestReadOperationsDoNotBlockOnFIFO(t *testing.T) {
	root, dir := testRoot(t)
	if err := unix.Mkfifo(filepath.Join(dir, "fifo"), 0o600); err != nil {
		t.Fatal(err)
	}
	for name, operation := range map[string]func() error{
		"read_file": func() error { _, err := root.ReadFile("fifo", 0); return err },
		"read_dir":  func() error { _, err := root.ReadDir("fifo"); return err },
		"stat":      func() error { _, err := root.Size("fifo"); return err },
	} {
		done := make(chan error, 1)
		go func() { done <- operation() }()
		select {
		case err := <-done:
			if err == nil {
				t.Fatalf("%s accepted a FIFO", name)
			}
		case <-time.After(2 * time.Second):
			t.Fatalf("%s blocked on a FIFO", name)
		}
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

func TestReadDirSkipsBuildAndVCS(t *testing.T) {
	root, dir := testRoot(t)
	if err := os.MkdirAll(filepath.Join(dir, "src"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(filepath.Join(dir, ".git"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(filepath.Join(dir, "target"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, "src", "main.rs"), []byte("fn main() {}"), 0o600); err != nil {
		t.Fatal(err)
	}
	entries, err := root.ReadDir(".")
	if err != nil {
		t.Fatal(err)
	}
	if len(entries) != 1 || entries[0].Name != "src" || !entries[0].IsDir {
		t.Fatalf("entries = %#v", entries)
	}
	children, err := root.ReadDir("src")
	if err != nil {
		t.Fatal(err)
	}
	if len(children) != 1 || children[0].Name != "main.rs" || children[0].IsDir {
		t.Fatalf("children = %#v", children)
	}
}

func TestCreateRenameAndRemoveEntries(t *testing.T) {
	root, dir := testRoot(t)
	if err := root.CreateDir("components"); err != nil {
		t.Fatalf("CreateDir: %v", err)
	}
	if info, err := os.Stat(filepath.Join(dir, "components")); err != nil || !info.IsDir() {
		t.Fatalf("created directory: info=%v err=%v", info, err)
	}
	write, err := root.CreateWrite("components/old.txt")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := write.Write([]byte("content")); err != nil {
		write.Abort()
		t.Fatal(err)
	}
	if err := write.Commit(); err != nil {
		t.Fatal(err)
	}
	if err := root.Rename("components/old.txt", "components/new.txt"); err != nil {
		t.Fatalf("Rename: %v", err)
	}
	if _, err := os.Stat(filepath.Join(dir, "components", "old.txt")); !os.IsNotExist(err) {
		t.Fatalf("old entry still exists: %v", err)
	}
	if got, err := os.ReadFile(filepath.Join(dir, "components", "new.txt")); err != nil || string(got) != "content" {
		t.Fatalf("renamed file = %q, %v", got, err)
	}
	if err := os.WriteFile(filepath.Join(dir, "components", "taken.txt"), []byte("x"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := root.Rename("components/new.txt", "components/taken.txt"); err == nil {
		t.Fatal("rename replaced an existing target")
	}
	if err := root.Remove("components/new.txt"); err != nil {
		t.Fatalf("Remove file: %v", err)
	}
	if err := root.Remove("components/taken.txt"); err != nil {
		t.Fatalf("Remove file: %v", err)
	}
	if err := root.Remove("components"); err != nil {
		t.Fatalf("Remove empty directory: %v", err)
	}
	if err := root.CreateDir("../escape"); err == nil {
		t.Fatal("CreateDir accepted traversal")
	}
}

func TestCopyFileDoesNotOverwriteOrFollowSymlinks(t *testing.T) {
	root, dir := testRoot(t)
	source := filepath.Join(dir, "source.txt")
	if err := os.WriteFile(source, []byte("copy me"), 0o640); err != nil {
		t.Fatal(err)
	}
	if err := root.Copy("source.txt", "copy.txt"); err != nil {
		t.Fatalf("Copy: %v", err)
	}
	got, err := os.ReadFile(filepath.Join(dir, "copy.txt"))
	if err != nil || string(got) != "copy me" {
		t.Fatalf("copy = %q, %v", got, err)
	}
	info, err := os.Stat(filepath.Join(dir, "copy.txt"))
	if err != nil {
		t.Fatal(err)
	}
	if info.Mode().Perm() != 0o640 {
		t.Fatalf("copy mode = %o", info.Mode().Perm())
	}
	if err := root.Copy("source.txt", "copy.txt"); err == nil {
		t.Fatal("copy replaced an existing destination")
	}

	outside := t.TempDir()
	if err := os.Symlink(source, filepath.Join(dir, "source-link")); err != nil {
		t.Fatal(err)
	}
	if err := root.Copy("source-link", "via-link.txt"); err == nil {
		t.Fatal("copy followed a source symlink")
	}
	if err := os.Symlink(outside, filepath.Join(dir, "outside-link")); err != nil {
		t.Fatal(err)
	}
	if err := root.Copy("source.txt", "outside-link/escape.txt"); err == nil {
		t.Fatal("copy escaped through a destination-parent symlink")
	}
	if _, err := os.Stat(filepath.Join(outside, "escape.txt")); !os.IsNotExist(err) {
		t.Fatal("copy wrote outside the workspace")
	}
}

func TestRemoveAllRecursiveDoesNotFollowSymlinks(t *testing.T) {
	root, dir := testRoot(t)
	tree := filepath.Join(dir, "tree")
	if err := os.MkdirAll(filepath.Join(tree, "nested"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(tree, "nested", "file.txt"), []byte("x"), 0o600); err != nil {
		t.Fatal(err)
	}
	outside := t.TempDir()
	outsideFile := filepath.Join(outside, "keep.txt")
	if err := os.WriteFile(outsideFile, []byte("keep"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(outside, filepath.Join(tree, "outside")); err != nil {
		t.Fatal(err)
	}
	if err := root.RemoveAll("tree"); err != nil {
		t.Fatalf("RemoveAll: %v", err)
	}
	if _, err := os.Stat(tree); !os.IsNotExist(err) {
		t.Fatalf("tree still exists: %v", err)
	}
	if got, err := os.ReadFile(outsideFile); err != nil || string(got) != "keep" {
		t.Fatalf("outside file changed: %q, %v", got, err)
	}
}

func TestStatReturnsSizeAndModificationTime(t *testing.T) {
	root, dir := testRoot(t)
	path := filepath.Join(dir, "watched.txt")
	if err := os.WriteFile(path, []byte("watch"), 0o600); err != nil {
		t.Fatal(err)
	}
	metadata, err := root.Stat("watched.txt")
	if err != nil {
		t.Fatalf("Stat: %v", err)
	}
	if metadata.Size != 5 {
		t.Fatalf("size = %d", metadata.Size)
	}
	if metadata.ModifiedNS <= 0 {
		t.Fatalf("modified = %d", metadata.ModifiedNS)
	}
}
