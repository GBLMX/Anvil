// Package pkg verifies and installs signed .anvil-plugin archives.
//
// An archive is a gzip-compressed tar containing regular files and
// directories. The detached Ed25519 signature covers the SHA-256 digest of
// the archive bytes. Both raw 64-byte signatures and base64/hex text are
// accepted.
package pkg

import (
	"archive/tar"
	"compress/gzip"
	"crypto/ed25519"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"os"
	"path"
	"path/filepath"
	"strings"

	"github.com/GBLMX/Anvil/go/internal/fsroot"
	"golang.org/x/sys/unix"
)

const (
	// MaxPackageBytes is the maximum compressed package size.
	MaxPackageBytes int64 = 64 << 20
	// MaxUncompressedBytes bounds the extracted package.
	MaxUncompressedBytes int64 = 256 << 20
	// MaxFiles bounds the number of archive entries.
	MaxFiles = 4096
	// MaxFileBytes bounds one extracted file.
	MaxFileBytes int64 = 32 << 20
)

var (
	// ErrInvalidSignature reports a signature that does not match the package.
	ErrInvalidSignature = errors.New("invalid package signature")
	// ErrInvalidArchive reports an unsafe or malformed package archive.
	ErrInvalidArchive = errors.New("invalid plugin package")
)

// DecodePublicKey accepts a 32-byte Ed25519 public key encoded as hex or
// standard base64.
func DecodePublicKey(value string) (ed25519.PublicKey, error) {
	value = strings.TrimSpace(value)
	if value == "" {
		return nil, errors.New("empty public key")
	}
	var raw []byte
	var err error
	if len(value) == ed25519.PublicKeySize*2 {
		raw, err = hex.DecodeString(value)
	} else {
		raw, err = base64.StdEncoding.DecodeString(value)
	}
	if err != nil || len(raw) != ed25519.PublicKeySize {
		return nil, errors.New("public key must be 32 bytes in hex or base64")
	}
	return ed25519.PublicKey(raw), nil
}

// ReadSignature accepts a raw 64-byte signature or a hex/base64 text file.
func ReadSignature(path string) ([]byte, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, fmt.Errorf("read signature: %w", err)
	}
	if len(data) == ed25519.SignatureSize {
		return data, nil
	}
	data = []byte(strings.TrimSpace(string(data)))
	if len(data) == ed25519.SignatureSize {
		return data, nil
	}
	if decoded, err := hex.DecodeString(string(data)); err == nil && len(decoded) == ed25519.SignatureSize {
		return decoded, nil
	}
	if decoded, err := base64.StdEncoding.DecodeString(string(data)); err == nil && len(decoded) == ed25519.SignatureSize {
		return decoded, nil
	}
	return nil, errors.New("signature must be 64 raw bytes or hex/base64")
}

// VerifyFile verifies archivePath against a detached signature and returns the
// archive SHA-256 digest.
func VerifyFile(archivePath, signaturePath string, publicKey ed25519.PublicKey) ([32]byte, error) {
	signature, err := ReadSignature(signaturePath)
	if err != nil {
		return [32]byte{}, err
	}
	file, err := os.Open(archivePath)
	if err != nil {
		return [32]byte{}, fmt.Errorf("open package: %w", err)
	}
	defer file.Close()
	stat, err := file.Stat()
	if err != nil {
		return [32]byte{}, fmt.Errorf("stat package: %w", err)
	}
	if stat.Size() > MaxPackageBytes {
		return [32]byte{}, fmt.Errorf("package exceeds %d bytes", MaxPackageBytes)
	}
	digest, err := sha256File(file)
	if err != nil {
		return [32]byte{}, err
	}
	if !ed25519.Verify(publicKey, digest[:], signature) {
		return [32]byte{}, ErrInvalidSignature
	}
	return digest, nil
}

// Install verifies archivePath, extracts it beneath destination, and returns
// the archive digest. destination must not already exist.
func Install(archivePath, signaturePath string, publicKey ed25519.PublicKey, destination string) ([32]byte, error) {
	digest, err := VerifyFile(archivePath, signaturePath, publicKey)
	if err != nil {
		return [32]byte{}, err
	}
	absolute, err := filepath.Abs(destination)
	if err != nil {
		return [32]byte{}, fmt.Errorf("resolve destination: %w", err)
	}
	if _, err := os.Lstat(absolute); err == nil {
		return [32]byte{}, fmt.Errorf("destination already exists: %s", absolute)
	} else if !errors.Is(err, os.ErrNotExist) {
		return [32]byte{}, fmt.Errorf("stat destination: %w", err)
	}
	parent := filepath.Dir(absolute)
	if err := os.MkdirAll(parent, 0o755); err != nil {
		return [32]byte{}, fmt.Errorf("create install parent: %w", err)
	}
	stage, err := os.MkdirTemp(parent, ".anvil-plugin-stage-")
	if err != nil {
		return [32]byte{}, fmt.Errorf("create staging directory: %w", err)
	}
	removeStage := true
	defer func() {
		if removeStage {
			_ = os.RemoveAll(stage)
		}
	}()

	file, err := os.Open(archivePath)
	if err != nil {
		return [32]byte{}, fmt.Errorf("reopen package: %w", err)
	}
	defer file.Close()
	if err := verifyPackageFile(file, digest); err != nil {
		return [32]byte{}, err
	}
	if _, err := file.Seek(0, io.SeekStart); err != nil {
		return [32]byte{}, fmt.Errorf("rewind package: %w", err)
	}
	if err := extract(file, stage); err != nil {
		return [32]byte{}, err
	}
	if _, err := rootHasManifest(stage); err != nil {
		return [32]byte{}, err
	}
	if _, err := os.Lstat(absolute); err == nil {
		return [32]byte{}, fmt.Errorf("destination already exists: %s", absolute)
	} else if !errors.Is(err, os.ErrNotExist) {
		return [32]byte{}, fmt.Errorf("stat destination: %w", err)
	}
	if err := unix.Renameat2(
		unix.AT_FDCWD,
		stage,
		unix.AT_FDCWD,
		absolute,
		unix.RENAME_NOREPLACE,
	); err != nil {
		return [32]byte{}, fmt.Errorf("commit package: %w", err)
	}
	removeStage = false
	return digest, nil
}

func sha256File(file *os.File) ([32]byte, error) {
	hash := sha256.New()
	if _, err := io.Copy(hash, io.LimitReader(file, MaxPackageBytes+1)); err != nil {
		return [32]byte{}, fmt.Errorf("hash package: %w", err)
	}
	var digest [32]byte
	copy(digest[:], hash.Sum(nil))
	return digest, nil
}

func verifyPackageFile(file *os.File, expected [32]byte) error {
	if _, err := file.Seek(0, io.SeekStart); err != nil {
		return fmt.Errorf("rewind package: %w", err)
	}
	digest, err := sha256File(file)
	if err != nil {
		return err
	}
	if digest != expected {
		return fmt.Errorf("package changed during verification")
	}
	return nil
}

func extract(file *os.File, stage string) error {
	root, err := fsroot.Open(stage)
	if err != nil {
		return err
	}
	defer root.Close()
	gzipReader, err := gzip.NewReader(file)
	if err != nil {
		return fmt.Errorf("%w: gzip: %v", ErrInvalidArchive, err)
	}
	defer gzipReader.Close()
	reader := tar.NewReader(io.LimitReader(gzipReader, MaxUncompressedBytes+1))
	seen := make(map[string]struct{})
	var total int64
	var files int
	for {
		header, err := reader.Next()
		if errors.Is(err, io.EOF) {
			break
		}
		if err != nil {
			return fmt.Errorf("%w: tar: %v", ErrInvalidArchive, err)
		}
		files++
		if files > MaxFiles {
			return fmt.Errorf("%w: too many entries", ErrInvalidArchive)
		}
		name, err := safeName(header.Name)
		if err != nil {
			return err
		}
		if _, exists := seen[name]; exists {
			return fmt.Errorf("%w: duplicate entry %q", ErrInvalidArchive, name)
		}
		seen[name] = struct{}{}
		switch header.Typeflag {
		case tar.TypeDir:
			if name == "." {
				continue
			}
			if err := ensureDir(root, name); err != nil {
				return err
			}
		case tar.TypeReg, tar.TypeRegA:
			if header.Size < 0 || header.Size > MaxFileBytes {
				return fmt.Errorf("%w: %q exceeds per-file limit", ErrInvalidArchive, name)
			}
			total += header.Size
			if total > MaxUncompressedBytes {
				return fmt.Errorf("%w: extracted size exceeds limit", ErrInvalidArchive)
			}
			if err := ensureDir(root, path.Dir(name)); err != nil {
				return err
			}
			write, err := root.CreateWrite(name)
			if err != nil {
				return fmt.Errorf("create %q: %w", name, err)
			}
			_, copyErr := io.CopyN(write, reader, header.Size)
			if copyErr != nil {
				write.Abort()
				return fmt.Errorf("%w: read %q: %v", ErrInvalidArchive, name, copyErr)
			}
			if err := write.Commit(); err != nil {
				return fmt.Errorf("commit %q: %w", name, err)
			}
		default:
			return fmt.Errorf("%w: unsupported entry type %q", ErrInvalidArchive, header.Name)
		}
	}
	return nil
}

func safeName(name string) (string, error) {
	if name == "" || strings.ContainsRune(name, '\x00') {
		return "", fmt.Errorf("%w: empty or NUL entry name", ErrInvalidArchive)
	}
	if strings.HasPrefix(name, "/") || strings.HasPrefix(name, `\`) || filepath.IsAbs(name) {
		return "", fmt.Errorf("%w: absolute entry %q", ErrInvalidArchive, name)
	}
	cleaned := path.Clean(strings.ReplaceAll(name, `\`, "/"))
	if cleaned == "." {
		return ".", nil
	}
	if cleaned == ".." || strings.HasPrefix(cleaned, "../") {
		return "", fmt.Errorf("%w: escaping entry %q", ErrInvalidArchive, name)
	}
	return cleaned, nil
}

func ensureDir(root *fsroot.Root, name string) error {
	if name == "." || name == "" {
		return nil
	}
	parts := strings.Split(name, "/")
	current := ""
	for _, part := range parts {
		if current == "" {
			current = part
		} else {
			current += "/" + part
		}
		if err := root.CreateDir(current); err != nil && !errors.Is(err, os.ErrExist) {
			return fmt.Errorf("create directory %q: %w", current, err)
		}
	}
	return nil
}

func rootHasManifest(stage string) (string, error) {
	file, err := os.Open(filepath.Join(stage, "plugin.json"))
	if err != nil {
		return "", fmt.Errorf("%w: plugin.json is required", ErrInvalidArchive)
	}
	defer file.Close()
	stat, err := file.Stat()
	if err != nil || !stat.Mode().IsRegular() || stat.Size() > 1<<20 {
		return "", fmt.Errorf("%w: invalid plugin.json", ErrInvalidArchive)
	}
	return file.Name(), nil
}
