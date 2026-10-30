// Package fsroot confines file reads to a workspace root using openat2.
package fsroot

import (
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"

	"golang.org/x/sys/unix"
)

// DefaultMaxRead is the default per-file read limit.
const DefaultMaxRead = 5 << 20

// Root is an O_PATH descriptor for a workspace directory.
type Root struct {
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
	return r.path
}

// Close releases the root descriptor.
func (r *Root) Close() error {
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

func cleanRelative(path string) (string, error) {
	if path == "" {
		return "", errors.New("empty path")
	}
	if strings.ContainsRune(path, 0) {
		return "", errors.New("path contains NUL")
	}
	if filepath.IsAbs(path) {
		return "", errors.New("absolute paths are not allowed")
	}
	clean := filepath.Clean(path)
	if clean == "." || clean == ".." || strings.HasPrefix(clean, "../") {
		return "", fmt.Errorf("path %q escapes the workspace", path)
	}
	return clean, nil
}

func openBeneath(dirfd int, path string) (int, error) {
	how := &unix.OpenHow{
		Flags:   uint64(unix.O_RDONLY | unix.O_CLOEXEC),
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
