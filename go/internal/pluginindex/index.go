// Package pluginindex verifies signed plugin index and revocation documents.
//
// The index root keys are build-time trust roots. Document signatures are
// detached Ed25519 signatures over SHA-256 of the exact downloaded bytes.
package pluginindex

import (
	"bytes"
	"crypto/ed25519"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/url"
	"strconv"
	"strings"
	"time"
	"unicode/utf8"
)

const (
	// MaxIndexBytes bounds the signed index payload.
	MaxIndexBytes int64 = 4 << 20
	// MaxRevocationBytes bounds the signed revocation payload.
	MaxRevocationBytes int64 = 1 << 20
	// MaxSignatureBytes bounds a detached signature envelope.
	MaxSignatureBytes int64 = 4 << 10
	// MaxIndexPublishers bounds publishers in one index.
	MaxIndexPublishers = 1024
	// MaxIndexPlugins bounds plugins in one index.
	MaxIndexPlugins = 4096
	// MaxVersionsPerPlugin bounds versions for one plugin.
	MaxVersionsPerPlugin = 64
	// MaxIndexText bounds one untrusted text field.
	MaxIndexText = 4096
	// MaxIndexURL bounds one URL field.
	MaxIndexURL = 2048
	// MaxPackageBytes is the maximum compressed package size.
	MaxPackageBytes int64 = 64 << 20
	// MaxRevocations bounds entries in one revocation document.
	MaxRevocations = 8192
	// MaxDocumentAge bounds the validity window.
	MaxDocumentAge = 7 * 24 * time.Hour
	// MaxFutureSkew bounds issued_at/revoked_at clock skew.
	MaxFutureSkew = 5 * time.Minute
)

var (
	// ErrInvalidSignature reports a document signature that does not verify.
	ErrInvalidSignature = errors.New("invalid document signature")
	// ErrInvalidDocument reports a malformed or out-of-bounds document.
	ErrInvalidDocument = errors.New("invalid document")
)

// RootKeys maps a key id to a build-time Ed25519 public key.
type RootKeys map[string]ed25519.PublicKey

// KeyID returns the ADR-0021 key id for a public key.
func KeyID(public ed25519.PublicKey) string {
	sum := sha256.Sum256(public)
	return "sha256:" + hex.EncodeToString(sum[:])
}

type signatureEnvelope struct {
	SchemaVersion int    `json:"schema_version"`
	KeyID         string `json:"key_id"`
	Signature     string `json:"signature"`
}

// VerifyDocument checks the exact payload bytes against a detached signature
// envelope and a build-time root key.
func VerifyDocument(payload, envelope []byte, roots RootKeys) error {
	if !utf8.Valid(payload) {
		return fmt.Errorf("%w: payload is not UTF-8", ErrInvalidDocument)
	}
	if int64(len(envelope)) > MaxSignatureBytes {
		return fmt.Errorf("%w: signature envelope too large", ErrInvalidDocument)
	}
	var parsed signatureEnvelope
	if err := decodeStrict(envelope, &parsed); err != nil {
		return fmt.Errorf("%w: signature envelope: %v", ErrInvalidSignature, err)
	}
	if parsed.SchemaVersion != 1 {
		return fmt.Errorf("%w: unsupported signature schema", ErrInvalidSignature)
	}
	if parsed.KeyID == "" {
		return fmt.Errorf("%w: missing key id", ErrInvalidSignature)
	}
	public, ok := roots[parsed.KeyID]
	if !ok {
		return fmt.Errorf("%w: unknown key id", ErrInvalidSignature)
	}
	if len(public) != ed25519.PublicKeySize {
		return fmt.Errorf("%w: invalid root public key", ErrInvalidSignature)
	}
	if KeyID(public) != parsed.KeyID {
		return fmt.Errorf("%w: key id mismatch", ErrInvalidSignature)
	}
	if parsed.Signature != strings.ToLower(parsed.Signature) {
		return fmt.Errorf("%w: signature must be lowercase hex", ErrInvalidSignature)
	}
	signature, err := hex.DecodeString(parsed.Signature)
	if err != nil || len(signature) != ed25519.SignatureSize {
		return fmt.Errorf("%w: signature must be 64 hex bytes", ErrInvalidSignature)
	}
	digest := sha256.Sum256(payload)
	if !ed25519.Verify(public, digest[:], signature) {
		return fmt.Errorf("%w: signature mismatch", ErrInvalidSignature)
	}
	return nil
}

func decodeStrict(data []byte, target any) error {
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(target); err != nil {
		return err
	}
	var trailing any
	if err := decoder.Decode(&trailing); err != io.EOF {
		return errors.New("trailing data")
	}
	return nil
}

// Publisher is one signed publisher entry.
type Publisher struct {
	ID        string `json:"id"`
	Name      string `json:"name"`
	KeyID     string `json:"key_id"`
	PublicKey string `json:"public_key"`
	Official  bool   `json:"official"`
}

// Version is one signed plugin version.
type Version struct {
	Version      string   `json:"version"`
	Digest       string   `json:"digest"`
	ManifestHash string   `json:"manifest_hash"`
	PackageURL   string   `json:"package_url"`
	SignatureURL string   `json:"signature_url"`
	PackageSize  int64    `json:"package_size"`
	APIMin       string   `json:"api_min"`
	APIMax       string   `json:"api_max"`
	Platforms    []string `json:"platforms"`
	Permissions  []string `json:"permissions"`
	PublishedAt  string   `json:"published_at"`
	Yanked       bool     `json:"yanked"`
}

// Plugin is one signed catalog entry.
type Plugin struct {
	ID          string    `json:"id"`
	Name        string    `json:"name"`
	Summary     string    `json:"summary"`
	PublisherID string    `json:"publisher_id"`
	Homepage    string    `json:"homepage"`
	License     string    `json:"license"`
	Versions    []Version `json:"versions"`
}

// Index is one signed plugin catalog.
type Index struct {
	Schema        string      `json:"schema"`
	SchemaVersion int         `json:"schema_version"`
	Sequence      uint64      `json:"sequence"`
	IssuedAt      string      `json:"issued_at"`
	ExpiresAt     string      `json:"expires_at"`
	Publishers    []Publisher `json:"publishers"`
	Plugins       []Plugin    `json:"plugins"`
}

// LoadIndex verifies and validates an index document.
//
// allowedHosts is the installer's exact host allowlist. A nil map allows any
// HTTPS host; production callers must pass their allowlist.
func LoadIndex(
	payload, envelope []byte,
	roots RootKeys,
	now time.Time,
	allowedHosts map[string]bool,
) (*Index, error) {
	if int64(len(payload)) > MaxIndexBytes {
		return nil, fmt.Errorf("%w: index exceeds size cap", ErrInvalidDocument)
	}
	if err := VerifyDocument(payload, envelope, roots); err != nil {
		return nil, err
	}
	var index Index
	if err := decodeStrict(payload, &index); err != nil {
		return nil, fmt.Errorf("%w: index JSON: %v", ErrInvalidDocument, err)
	}
	if err := validateIndex(&index, now, allowedHosts); err != nil {
		return nil, err
	}
	return &index, nil
}

func validateIndex(index *Index, now time.Time, allowedHosts map[string]bool) error {
	if index.Schema != "anvil.plugin-index" || index.SchemaVersion != 1 {
		return fmt.Errorf("%w: unsupported index schema", ErrInvalidDocument)
	}
	if _, _, err := validateWindow(index.IssuedAt, index.ExpiresAt, now); err != nil {
		return err
	}
	if len(index.Publishers) > MaxIndexPublishers {
		return fmt.Errorf("%w: too many publishers", ErrInvalidDocument)
	}
	if len(index.Plugins) > MaxIndexPlugins {
		return fmt.Errorf("%w: too many plugins", ErrInvalidDocument)
	}
	publishers := make(map[string]Publisher, len(index.Publishers))
	for _, publisher := range index.Publishers {
		if err := validatePublisher(publisher); err != nil {
			return err
		}
		if _, exists := publishers[publisher.ID]; exists {
			return fmt.Errorf("%w: duplicate publisher %q", ErrInvalidDocument, publisher.ID)
		}
		publishers[publisher.ID] = publisher
	}
	plugins := make(map[string]struct{}, len(index.Plugins))
	for _, plugin := range index.Plugins {
		if err := validatePlugin(plugin, publishers, allowedHosts); err != nil {
			return err
		}
		if _, exists := plugins[plugin.ID]; exists {
			return fmt.Errorf("%w: duplicate plugin %q", ErrInvalidDocument, plugin.ID)
		}
		plugins[plugin.ID] = struct{}{}
	}
	return nil
}

func validatePublisher(publisher Publisher) error {
	if err := validatePluginID(publisher.ID); err != nil {
		return fmt.Errorf("%w: publisher id: %v", ErrInvalidDocument, err)
	}
	if err := validateText(publisher.Name, "publisher name", false); err != nil {
		return err
	}
	public, err := hex.DecodeString(publisher.PublicKey)
	if err != nil || len(public) != ed25519.PublicKeySize || publisher.PublicKey != strings.ToLower(publisher.PublicKey) {
		return fmt.Errorf("%w: publisher public key must be 32 lowercase hex bytes", ErrInvalidDocument)
	}
	if publisher.KeyID != KeyID(ed25519.PublicKey(public)) {
		return fmt.Errorf("%w: publisher key id mismatch", ErrInvalidDocument)
	}
	return nil
}

func validatePlugin(plugin Plugin, publishers map[string]Publisher, allowedHosts map[string]bool) error {
	if err := validatePluginID(plugin.ID); err != nil {
		return fmt.Errorf("%w: plugin id: %v", ErrInvalidDocument, err)
	}
	if err := validateText(plugin.Name, "plugin name", false); err != nil {
		return err
	}
	if err := validateText(plugin.Summary, "plugin summary", true); err != nil {
		return err
	}
	if _, ok := publishers[plugin.PublisherID]; !ok {
		return fmt.Errorf("%w: plugin %q names unknown publisher", ErrInvalidDocument, plugin.ID)
	}
	if plugin.Homepage != "" {
		if err := validateURL(plugin.Homepage, "homepage", allowedHosts); err != nil {
			return err
		}
	}
	if err := validateText(plugin.License, "license", true); err != nil {
		return err
	}
	if len(plugin.Versions) == 0 || len(plugin.Versions) > MaxVersionsPerPlugin {
		return fmt.Errorf("%w: plugin %q has invalid version count", ErrInvalidDocument, plugin.ID)
	}
	versions := make(map[string]struct{}, len(plugin.Versions))
	for _, version := range plugin.Versions {
		if _, exists := versions[version.Version]; exists {
			return fmt.Errorf("%w: plugin %q has duplicate version %q", ErrInvalidDocument, plugin.ID, version.Version)
		}
		versions[version.Version] = struct{}{}
		if err := validateVersion(version, allowedHosts); err != nil {
			return err
		}
	}
	return nil
}

func validateVersion(version Version, allowedHosts map[string]bool) error {
	if _, err := parseSemver(version.Version); err != nil {
		return fmt.Errorf("%w: plugin version: %v", ErrInvalidDocument, err)
	}
	for field, value := range map[string]string{
		"digest":        version.Digest,
		"manifest_hash": version.ManifestHash,
	} {
		raw, err := hex.DecodeString(value)
		if err != nil || len(raw) != sha256.Size || value != strings.ToLower(value) {
			return fmt.Errorf("%w: %s must be 32 lowercase hex bytes", ErrInvalidDocument, field)
		}
	}
	if version.PackageSize <= 0 || version.PackageSize > MaxPackageBytes {
		return fmt.Errorf("%w: package size out of bounds", ErrInvalidDocument)
	}
	if err := validateURL(version.PackageURL, "package_url", allowedHosts); err != nil {
		return err
	}
	if err := validateURL(version.SignatureURL, "signature_url", allowedHosts); err != nil {
		return err
	}
	minimum, err := parseSemver(version.APIMin)
	if err != nil {
		return fmt.Errorf("%w: api_min: %v", ErrInvalidDocument, err)
	}
	maximum, err := parseSemver(version.APIMax)
	if err != nil {
		return fmt.Errorf("%w: api_max: %v", ErrInvalidDocument, err)
	}
	if compareSemver(minimum, maximum) >= 0 {
		return fmt.Errorf("%w: api range must be non-empty", ErrInvalidDocument)
	}
	if len(version.Platforms) == 0 {
		return fmt.Errorf("%w: platforms must not be empty", ErrInvalidDocument)
	}
	platforms := make(map[string]struct{}, len(version.Platforms))
	for _, platform := range version.Platforms {
		if err := validateText(platform, "platform", false); err != nil {
			return err
		}
		if _, exists := platforms[platform]; exists {
			return fmt.Errorf("%w: duplicate platform %q", ErrInvalidDocument, platform)
		}
		platforms[platform] = struct{}{}
	}
	permissions := make(map[string]struct{}, len(version.Permissions))
	for _, permission := range version.Permissions {
		if err := validateText(permission, "permission", false); err != nil {
			return err
		}
		if _, exists := permissions[permission]; exists {
			return fmt.Errorf("%w: duplicate permission %q", ErrInvalidDocument, permission)
		}
		permissions[permission] = struct{}{}
	}
	if _, err := time.Parse(time.RFC3339, version.PublishedAt); err != nil {
		return fmt.Errorf("%w: published_at: %v", ErrInvalidDocument, err)
	}
	return nil
}

func validateText(value, field string, allowEmpty bool) error {
	if value == "" {
		if allowEmpty {
			return nil
		}
		return fmt.Errorf("%w: %s must not be empty", ErrInvalidDocument, field)
	}
	if len(value) > MaxIndexText {
		return fmt.Errorf("%w: %s exceeds text cap", ErrInvalidDocument, field)
	}
	for _, char := range value {
		if char < 0x20 || char == 0x7f || char == '<' || char == '>' {
			return fmt.Errorf("%w: %s contains unsafe text", ErrInvalidDocument, field)
		}
	}
	return nil
}

func validatePluginID(value string) error {
	if value == "" || len(value) > 128 {
		return errors.New("must be 1-128 bytes")
	}
	for _, char := range value {
		if (char >= 'a' && char <= 'z') || (char >= 'A' && char <= 'Z') ||
			(char >= '0' && char <= '9') || char == '.' || char == '_' || char == '-' {
			continue
		}
		return errors.New("contains invalid characters")
	}
	return nil
}

func validateURL(value, field string, allowedHosts map[string]bool) error {
	if len(value) > MaxIndexURL {
		return fmt.Errorf("%w: %s exceeds URL cap", ErrInvalidDocument, field)
	}
	parsed, err := url.Parse(value)
	if err != nil || parsed.Scheme != "https" || parsed.Host == "" || parsed.Opaque != "" {
		return fmt.Errorf("%w: %s must be an absolute https URL", ErrInvalidDocument, field)
	}
	if parsed.User != nil || parsed.Fragment != "" {
		return fmt.Errorf("%w: %s must not contain userinfo or fragment", ErrInvalidDocument, field)
	}
	if allowedHosts != nil && !allowedHosts[strings.ToLower(parsed.Hostname())] {
		return fmt.Errorf("%w: %s host is not allowed", ErrInvalidDocument, field)
	}
	return nil
}

func validateWindow(issuedRaw, expiresRaw string, now time.Time) (time.Time, time.Time, error) {
	issued, err := time.Parse(time.RFC3339, issuedRaw)
	if err != nil {
		return time.Time{}, time.Time{}, fmt.Errorf("%w: issued_at: %v", ErrInvalidDocument, err)
	}
	expires, err := time.Parse(time.RFC3339, expiresRaw)
	if err != nil {
		return time.Time{}, time.Time{}, fmt.Errorf("%w: expires_at: %v", ErrInvalidDocument, err)
	}
	if issued.After(now.Add(MaxFutureSkew)) {
		return time.Time{}, time.Time{}, fmt.Errorf("%w: issued_at is in the future", ErrInvalidDocument)
	}
	if !expires.After(issued) || expires.Sub(issued) > MaxDocumentAge {
		return time.Time{}, time.Time{}, fmt.Errorf("%w: invalid validity window", ErrInvalidDocument)
	}
	return issued, expires, nil
}

// Compatible reports whether a signed version accepts the running API and
// platform.
func (version Version) Compatible(apiVersion, platform string) bool {
	current, err := parseSemver(apiVersion)
	if err != nil {
		return false
	}
	minimum, err := parseSemver(version.APIMin)
	if err != nil {
		return false
	}
	maximum, err := parseSemver(version.APIMax)
	if err != nil {
		return false
	}
	if compareSemver(current, minimum) < 0 || compareSemver(current, maximum) >= 0 {
		return false
	}
	for _, candidate := range version.Platforms {
		if candidate == platform {
			return true
		}
	}
	return false
}

type semver struct {
	major, minor, patch uint64
	prerelease          string
}

func parseSemver(value string) (semver, error) {
	value = strings.TrimPrefix(value, "v")
	if value == "" || len(value) > MaxIndexText {
		return semver{}, errors.New("empty or oversized version")
	}
	core, prerelease, _ := strings.Cut(value, "+")
	if before, after, found := strings.Cut(core, "-"); found {
		core, prerelease = before, after
	}
	parts := strings.Split(core, ".")
	if len(parts) != 3 {
		return semver{}, errors.New("version must be major.minor.patch")
	}
	numbers := make([]uint64, 3)
	for index, part := range parts {
		if part == "" {
			return semver{}, errors.New("empty version component")
		}
		number, err := strconv.ParseUint(part, 10, 64)
		if err != nil {
			return semver{}, errors.New("invalid version component")
		}
		numbers[index] = number
	}
	return semver{major: numbers[0], minor: numbers[1], patch: numbers[2], prerelease: prerelease}, nil
}

func compareSemver(left, right semver) int {
	if left.major != right.major {
		return compareUint(left.major, right.major)
	}
	if left.minor != right.minor {
		return compareUint(left.minor, right.minor)
	}
	if left.patch != right.patch {
		return compareUint(left.patch, right.patch)
	}
	if left.prerelease == right.prerelease {
		return 0
	}
	if left.prerelease == "" {
		return 1
	}
	if right.prerelease == "" {
		return -1
	}
	if left.prerelease < right.prerelease {
		return -1
	}
	return 1
}

func compareUint(left, right uint64) int {
	if left < right {
		return -1
	}
	return 1
}
