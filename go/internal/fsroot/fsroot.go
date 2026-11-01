// Package fsroot confines file access to a workspace root using openat2.
package fsroot

import (
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"

	"golang.org/x/sys/unix"
)

// DefaultMaxRead is the default per-file read limit.
const DefaultMaxRead = 5 << 20

// ErrOffsetOutOfRange reports a read offset beyond the file size.
var ErrOffsetOutOfRange = errors.New("offset beyond end of file")

// ErrInvalidPath reports a path that is not a valid workspace-relative path.
var ErrInvalidPath = errors.New("invalid workspace path")

// Root is an O_PATH descriptor for a workspace directory.
type Root struct {
	mu   sync.RWMutex
	fd   int
	path string
}

// Open opens path as a directory root.
func Open(path string) (*Root, error) {
	absolute, err := filepath.Abs(path)
	if err != nil {
		return nil, fmt.Errorf("resolve workspace root: %w", err)
	}
	fd, err := unix.Open(absolute, unix.O_PATH|unix.O_DIRECTORY|unix.O_CLOEXEC, 0)
	if err != nil {
		return nil, fmt.Errorf("open workspace root %q: %w", absolute, err)
	}
	return &Root{fd: fd, path: absolute}, nil
}

// Path returns the absolute path used when the root was opened.
func (r *Root) Path() string {
	r.mu.RLock()
	defer r.mu.RUnlock()
	return r.path
}

// Close releases the root descriptor.
func (r *Root) Close() error {
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.fd < 0 {
		return nil
	}
	err := unix.Close(r.fd)
	r.fd = -1
	return err
}

// ReadFile reads a regular file beneath the root.
//
// Paths are relative to the root. Absolute paths, traversal, empty paths, and
// non-regular files are rejected. A non-positive max uses DefaultMaxRead.
func (r *Root) ReadFile(path string, max int64) ([]byte, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	if max <= 0 {
		max = DefaultMaxRead
	}
	relative, err := cleanRelative(path)
	if err != nil {
		return nil, err
	}
	fd, err := openBeneath(r.fd, relative)
	if err != nil {
		return nil, fmt.Errorf("open %q beneath workspace: %w", relative, err)
	}

	file := os.NewFile(uintptr(fd), relative)
	if file == nil {
		_ = unix.Close(fd)
		return nil, errors.New("create file handle")
	}
	defer file.Close()

	var stat unix.Stat_t
	if err := unix.Fstat(fd, &stat); err != nil {
		return nil, fmt.Errorf("stat %q: %w", relative, err)
	}
	if stat.Mode&unix.S_IFMT != unix.S_IFREG {
		return nil, fmt.Errorf("%q is not a regular file", relative)
	}
	if stat.Size > max {
		return nil, fmt.Errorf("%q is %d bytes, limit is %d", relative, stat.Size, max)
	}

	data, err := io.ReadAll(io.LimitReader(file, max+1))
	if err != nil {
		return nil, fmt.Errorf("read %q: %w", relative, err)
	}
	if int64(len(data)) > max {
		return nil, fmt.Errorf("%q exceeds the %d-byte limit", relative, max)
	}
	return data, nil
}

// ReadFileAt reads at most max bytes starting at offset beneath the root.
//
// Offsets are absolute byte offsets. The returned bool is true when the
// returned chunk reaches the end of the file. A non-positive max uses
// DefaultMaxRead.
func (r *Root) ReadFileAt(path string, offset, max int64) ([]byte, bool, error) {
	r.mu.RLock()
	defer r.mu.RUnlock()
	if offset < 0 {
		return nil, false, errors.New("offset must be non-negative")
	}
	if max <= 0 {
		max = DefaultMaxRead
	}
	relative, err := cleanRelative(path)
	if err != nil {
		return nil, false, err
	}
	fd, err := openBeneath(r.fd, relative)
	if err != nil {
		return nil, false, fmt.Errorf("open %q beneath workspace: %w", relative, err)
	}
	file := os.NewFile(uintptr(fd), relative)
	if file == nil {
		_ = unix.Close(fd)
		return nil, false, errors.New("create file handle")
	}
	defer file.Close()

	var stat unix.Stat_t
	if err := unix.Fstat(fd, &stat); err != nil {
		return nil, false, fmt.Errorf("stat %q: %w", relative, err)
	}
	if stat.Mode&unix.S_IFMT != unix.S_IFREG {
		return nil, false, fmt.Errorf("%q is not a regular file", relative)
	}
	if offset > stat.Size {
		return nil, false, fmt.Errorf("%w: offset %d is beyond %q", ErrOffsetOutOfRange, offset, relative)
	}
	remaining := stat.Size - offset
	if remaining == 0 {
		return []byte{}, true, nil
	}
	n := min(max, remaining)
	data := make([]byte, n)
	read, err := file.ReadAt(data, offset)
	if err != nil && !errors.Is(err, io.EOF) {
		return nil, false, fmt.Errorf("read %q: %w", relative, err)
	}
	data = data[:read]
	return data, offset+int64(read) >= stat.Size, nil
}

// DirEntry is one child of a workspace directory.
type DirEntry struct {
	Name  string `json:"name"`
	IsDir bool   `json:"is_dir"`
}

// ReadDir lists a directory beneath the root, excluding VCS and build output.
func (r *Root) ReadDir(path string) ([]DirEntry, error) {
	relative := "."
	if path != "" && path != "." {
		cleaned, err := cleanRelative(path)
		if err != nil {
			return nil, err
		}
		relative = cleaned
	}
	r.mu.RLock()
	if r.fd < 0 {
		r.mu.RUnlock()
		return nil, errors.New("workspace is closed")
	}
	fd, err := openBeneath(r.fd, relative)
	r.mu.RUnlock()
	if err != nil {
		return nil, fmt.Errorf("open directory %q beneath workspace: %w", relative, err)
	}
	dir := os.NewFile(uintptr(fd), relative)
	if dir == nil {
		_ = unix.Close(fd)
		return nil, errors.New("create directory handle")
	}
	defer dir.Close()
	entries, err := dir.ReadDir(-1)
	if err != nil {
		return nil, fmt.Errorf("read directory %q: %w", relative, err)
	}
	result := make([]DirEntry, 0, len(entries))
	for _, entry := range entries {
		name := entry.Name()
		if name == ".git" || name == "target" || name == writeTempPrefix {
			continue
		}
		result = append(result, DirEntry{Name: name, IsDir: entry.IsDir()})
	}
	sort.Slice(result, func(i, j int) bool {
		if result[i].IsDir != result[j].IsDir {
			return result[i].IsDir
		}
		return result[i].Name < result[j].Name
	})
	return result, nil
}

func splitRelative(path string) (string, string, error) {
	relative, err := cleanRelative(path)
	if err != nil {
		return "", "", err
	}
	dirName := filepath.Dir(relative)
	name := filepath.Base(relative)
	if name == "." || name == ".." || name == string(filepath.Separator) {
		return "", "", fmt.Errorf("%w: invalid entry name %q", ErrInvalidPath, name)
	}
	return dirName, name, nil
}

// CreateDir creates one directory beneath the root without creating parents.
func (r *Root) CreateDir(path string) error {
	dirName, name, err := splitRelative(path)
	if err != nil {
		return err
	}
	r.mu.RLock()
	if r.fd < 0 {
		r.mu.RUnlock()
		return errors.New("workspace is closed")
	}
	dirFD, err := openDirBeneath(r.fd, dirName)
	r.mu.RUnlock()
	if err != nil {
		return fmt.Errorf("open parent directory for %q: %w", path, err)
	}
	defer unix.Close(dirFD)
	if err := unix.Mkdirat(dirFD, name, 0o755); err != nil {
		return fmt.Errorf("create directory %q: %w", path, err)
	}
	return nil
}

// Rename moves one entry beneath the root without replacing an existing target.
func (r *Root) Rename(from, to string) error {
	fromDirName, fromName, err := splitRelative(from)
	if err != nil {
		return err
	}
	toDirName, toName, err := splitRelative(to)
	if err != nil {
		return err
	}
	r.mu.RLock()
	if r.fd < 0 {
		r.mu.RUnlock()
		return errors.New("workspace is closed")
	}
	fromFD, err := openDirBeneath(r.fd, fromDirName)
	if err != nil {
		r.mu.RUnlock()
		return fmt.Errorf("open source directory for %q: %w", from, err)
	}
	toFD, err := openDirBeneath(r.fd, toDirName)
	r.mu.RUnlock()
	if err != nil {
		_ = unix.Close(fromFD)
		return fmt.Errorf("open destination directory for %q: %w", to, err)
	}
	defer unix.Close(fromFD)
	defer unix.Close(toFD)
	if err := unix.Renameat2(fromFD, fromName, toFD, toName, unix.RENAME_NOREPLACE); err != nil {
		return fmt.Errorf("rename %q to %q: %w", from, to, err)
	}
	return nil
}

// Remove deletes one file, symlink, or empty directory beneath the root.
func (r *Root) Remove(path string) error {
	dirName, name, err := splitRelative(path)
	if err != nil {
		return err
	}
	r.mu.RLock()
	if r.fd < 0 {
		r.mu.RUnlock()
		return errors.New("workspace is closed")
	}
	dirFD, err := openDirBeneath(r.fd, dirName)
	r.mu.RUnlock()
	if err != nil {
		return fmt.Errorf("open parent directory for %q: %w", path, err)
	}
	defer unix.Close(dirFD)
	if err := unix.Unlinkat(dirFD, name, 0); err != nil {
		if errors.Is(err, unix.EISDIR) {
			err = unix.Unlinkat(dirFD, name, unix.AT_REMOVEDIR)
		}
		if err != nil {
			return fmt.Errorf("remove %q: %w", path, err)
		}
	}
	return nil
}

const writeTempPrefix = ".anvil-write-"

// WriteTarget is an atomic file replacement in progress.
//
// The temporary file lives in the destination directory. Commit syncs it once,
// closes it, and renames it over the destination through the same directory
// descriptor that was opened with RESOLVE_BENEATH. Abort removes the temporary
// file and never touches the destination.
type WriteTarget struct {
	file     *os.File
	dir      *os.File
	tempName string
	destName string
}

// CreateWrite starts an atomic replacement of path beneath the root.
func (r *Root) CreateWrite(path string) (*WriteTarget, error) {
	relative, err := cleanRelative(path)
	if err != nil {
		return nil, err
	}
	dirName := filepath.Dir(relative)
	destName := filepath.Base(relative)
	if destName == "." || destName == ".." || destName == string(filepath.Separator) {
		return nil, fmt.Errorf("invalid destination name %q", destName)
	}

	r.mu.RLock()
	if r.fd < 0 {
		r.mu.RUnlock()
		return nil, errors.New("workspace is closed")
	}
	dirFD, err := openDirBeneath(r.fd, dirName)
	r.mu.RUnlock()
	if err != nil {
		return nil, fmt.Errorf("open destination directory for %q: %w", relative, err)
	}
	dir := os.NewFile(uintptr(dirFD), dirName)
	if dir == nil {
		_ = unix.Close(dirFD)
		return nil, errors.New("create directory handle")
	}

	tempName, file, err := createTemp(dir)
	if err != nil {
		_ = dir.Close()
		return nil, fmt.Errorf("create temporary file for %q: %w", relative, err)
	}
	return &WriteTarget{
		file:     file,
		dir:      dir,
		tempName: tempName,
		destName: destName,
	}, nil
}

// Write appends bytes to the temporary file.
func (w *WriteTarget) Write(data []byte) (int, error) {
	if w == nil || w.file == nil {
		return 0, errors.New("write target is closed")
	}
	return w.file.Write(data)
}

// Commit synchronizes the temporary file and atomically replaces the target.
func (w *WriteTarget) Commit() error {
	if w == nil || w.file == nil {
		return errors.New("write target is closed")
	}
	if err := w.file.Sync(); err != nil {
		w.Abort()
		return fmt.Errorf("sync temporary file: %w", err)
	}
	if err := w.file.Close(); err != nil {
		w.file = nil
		w.cleanup()
		return fmt.Errorf("close temporary file: %w", err)
	}
	w.file = nil
	dirFD := int(w.dir.Fd())
	if err := unix.Renameat(dirFD, w.tempName, dirFD, w.destName); err != nil {
		w.cleanup()
		return fmt.Errorf("rename temporary file: %w", err)
	}
	w.tempName = ""
	if w.dir != nil {
		_ = w.dir.Close()
		w.dir = nil
	}
	return nil
}

// Abort discards the temporary file.
func (w *WriteTarget) Abort() {
	if w == nil {
		return
	}
	if w.file != nil {
		_ = w.file.Close()
		w.file = nil
	}
	w.cleanup()
}

func (w *WriteTarget) cleanup() {
	if w.dir != nil && w.tempName != "" {
		_ = unix.Unlinkat(int(w.dir.Fd()), w.tempName, 0)
		w.tempName = ""
	}
	if w.dir != nil {
		_ = w.dir.Close()
		w.dir = nil
	}
}

func createTemp(dir *os.File) (string, *os.File, error) {
	dirFD := int(dir.Fd())
	for range 16 {
		var suffix [8]byte
		if _, err := rand.Read(suffix[:]); err != nil {
			return "", nil, err
		}
		name := writeTempPrefix + hex.EncodeToString(suffix[:])
		fd, err := unix.Openat(dirFD, name, unix.O_WRONLY|unix.O_CREAT|unix.O_EXCL|unix.O_CLOEXEC, 0o600)
		if errors.Is(err, unix.EEXIST) {
			continue
		}
		if err != nil {
			return "", nil, err
		}
		file := os.NewFile(uintptr(fd), name)
		if file == nil {
			_ = unix.Close(fd)
			_ = unix.Unlinkat(dirFD, name, 0)
			return "", nil, errors.New("create temporary file handle")
		}
		return name, file, nil
	}
	return "", nil, errors.New("temporary file name collision")
}

func openDirBeneath(dirfd int, path string) (int, error) {
	how := &unix.OpenHow{
		Flags:   uint64(unix.O_PATH | unix.O_DIRECTORY | unix.O_CLOEXEC),
		Resolve: uint64(unix.RESOLVE_BENEATH | unix.RESOLVE_NO_MAGICLINKS),
	}
	for range 4 {
		fd, err := unix.Openat2(dirfd, path, how)
		if errors.Is(err, unix.EAGAIN) {
			continue
		}
		return fd, err
	}
	return -1, unix.EAGAIN
}

func cleanRelative(path string) (string, error) {
	if path == "" {
		return "", fmt.Errorf("%w: empty path", ErrInvalidPath)
	}
	if strings.ContainsRune(path, 0) {
		return "", fmt.Errorf("%w: path contains NUL", ErrInvalidPath)
	}
	if filepath.IsAbs(path) {
		return "", fmt.Errorf("%w: absolute paths are not allowed", ErrInvalidPath)
	}
	clean := filepath.Clean(path)
	if clean == "." || clean == ".." || strings.HasPrefix(clean, "../") {
		return "", fmt.Errorf("%w: path %q escapes the workspace", ErrInvalidPath, path)
	}
	return clean, nil
}

func openBeneath(dirfd int, path string) (int, error) {
	how := &unix.OpenHow{
		Flags:   uint64(unix.O_RDONLY | unix.O_CLOEXEC | unix.O_NONBLOCK),
		Resolve: uint64(unix.RESOLVE_BENEATH | unix.RESOLVE_NO_MAGICLINKS),
	}
	for range 4 {
		fd, err := unix.Openat2(dirfd, path, how)
		if errors.Is(err, unix.EAGAIN) {
			continue
		}
		return fd, err
	}
	return -1, unix.EAGAIN
}

// Size returns the size of a regular file beneath the root.
func (r *Root) Size(path string) (int64, error) {
	relative, err := cleanRelative(path)
	if err != nil {
		return 0, err
	}
	r.mu.RLock()
	defer r.mu.RUnlock()
	if r.fd < 0 {
		return 0, errors.New("workspace is closed")
	}
	fd, err := openBeneath(r.fd, relative)
	if err != nil {
		return 0, fmt.Errorf("open %q beneath workspace: %w", relative, err)
	}
	defer unix.Close(fd)
	var stat unix.Stat_t
	if err := unix.Fstat(fd, &stat); err != nil {
		return 0, fmt.Errorf("stat %q: %w", relative, err)
	}
	if stat.Mode&unix.S_IFMT != unix.S_IFREG {
		return 0, fmt.Errorf("%q is not a regular file", relative)
	}
	return stat.Size, nil
}
