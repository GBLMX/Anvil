package pluginindex

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"strings"
	"time"
)

// RevocationEntry is one package or publisher revocation.
type RevocationEntry struct {
	Kind           string `json:"kind"`
	PluginID       string `json:"plugin_id,omitempty"`
	Digest         string `json:"digest,omitempty"`
	PublisherKeyID string `json:"publisher_key_id,omitempty"`
	Reason         string `json:"reason"`
	RevokedAt      string `json:"revoked_at"`
}

// Revocations is one signed revocation document.
type Revocations struct {
	Schema        string            `json:"schema"`
	SchemaVersion int               `json:"schema_version"`
	Sequence      uint64            `json:"sequence"`
	IssuedAt      string            `json:"issued_at"`
	ExpiresAt     string            `json:"expires_at"`
	Entries       []RevocationEntry `json:"entries"`
}

// LoadRevocations verifies and validates a revocation document.
func LoadRevocations(
	payload, envelope []byte,
	roots RootKeys,
	now time.Time,
) (*Revocations, error) {
	if int64(len(payload)) > MaxRevocationBytes {
		return nil, fmt.Errorf("%w: revocations exceed size cap", ErrInvalidDocument)
	}
	if err := VerifyDocument(payload, envelope, roots); err != nil {
		return nil, err
	}
	var document Revocations
	if err := decodeStrict(payload, &document); err != nil {
		return nil, fmt.Errorf("%w: revocation JSON: %v", ErrInvalidDocument, err)
	}
	if document.Schema != "anvil.plugin-revocations" || document.SchemaVersion != 1 {
		return nil, fmt.Errorf("%w: unsupported revocation schema", ErrInvalidDocument)
	}
	_, _, windowErr := validateWindow(document.IssuedAt, document.ExpiresAt, now)
	if len(document.Entries) > MaxRevocations {
		return nil, fmt.Errorf("%w: too many revocations", ErrInvalidDocument)
	}
	packageOwners := make(map[string]string)
	publishers := make(map[string]struct{})
	seen := make(map[string]struct{})
	for _, entry := range document.Entries {
		if err := validateRevocation(entry, now); err != nil {
			return nil, err
		}
		switch entry.Kind {
		case "package":
			if owner, exists := packageOwners[entry.Digest]; exists && owner != entry.PluginID {
				return nil, fmt.Errorf("%w: conflicting package revocation owner", ErrInvalidDocument)
			}
			packageOwners[entry.Digest] = entry.PluginID
		case "publisher":
			publishers[entry.PublisherKeyID] = struct{}{}
		}
		key := entry.Kind + "\x00" + entry.Digest + "\x00" + entry.PublisherKeyID + "\x00" + entry.PluginID
		if _, exists := seen[key]; exists {
			return nil, fmt.Errorf("%w: duplicate revocation entry", ErrInvalidDocument)
		}
		seen[key] = struct{}{}
	}
	return &document, windowErr
}

func validateRevocation(entry RevocationEntry, now time.Time) error {
	switch entry.Kind {
	case "package":
		if err := validatePluginID(entry.PluginID); err != nil {
			return fmt.Errorf("%w: revoked plugin id: %v", ErrInvalidDocument, err)
		}
		raw, err := hex.DecodeString(entry.Digest)
		if err != nil || len(raw) != sha256.Size || entry.Digest != strings.ToLower(entry.Digest) {
			return fmt.Errorf("%w: revocation digest must be 32 lowercase hex bytes", ErrInvalidDocument)
		}
	case "publisher":
		if !validKeyID(entry.PublisherKeyID) {
			return fmt.Errorf("%w: invalid revoked publisher key id", ErrInvalidDocument)
		}
	default:
		return fmt.Errorf("%w: unknown revocation kind", ErrInvalidDocument)
	}
	if err := validateText(entry.Reason, "revocation reason", false); err != nil {
		return err
	}
	revoked, err := time.Parse(time.RFC3339, entry.RevokedAt)
	if err != nil {
		return fmt.Errorf("%w: revoked_at: %v", ErrInvalidDocument, err)
	}
	if revoked.After(now.Add(MaxFutureSkew)) {
		return fmt.Errorf("%w: revoked_at is in the future", ErrInvalidDocument)
	}
	return nil
}

func validKeyID(value string) bool {
	if !strings.HasPrefix(value, "sha256:") {
		return false
	}
	raw, err := hex.DecodeString(strings.TrimPrefix(value, "sha256:"))
	return err == nil && len(raw) == sha256.Size && strings.ToLower(value) == value
}

// RevocationSet is the append-only local union of accepted revocations.
type RevocationSet struct {
	Packages   map[string]string
	Publishers map[string]struct{}
}

// NewRevocationSet returns an empty set.
func NewRevocationSet() *RevocationSet {
	return &RevocationSet{
		Packages:   make(map[string]string),
		Publishers: make(map[string]struct{}),
	}
}

// Merge adds a verified document to the union. Previously accepted entries are
// never removed.
func (set *RevocationSet) Merge(document *Revocations) error {
	if document == nil {
		return fmt.Errorf("%w: nil revocation document", ErrInvalidDocument)
	}
	if set.Packages == nil {
		set.Packages = make(map[string]string)
	}
	if set.Publishers == nil {
		set.Publishers = make(map[string]struct{})
	}
	for _, entry := range document.Entries {
		switch entry.Kind {
		case "package":
			if owner, exists := set.Packages[entry.Digest]; exists {
				if owner != entry.PluginID {
					return fmt.Errorf("%w: conflicting cached revocation owner", ErrInvalidDocument)
				}
				continue
			}
			set.Packages[entry.Digest] = entry.PluginID
		case "publisher":
			set.Publishers[entry.PublisherKeyID] = struct{}{}
		}
	}
	return nil
}

// PackageRevoked reports whether a package digest is revoked.
func (set *RevocationSet) PackageRevoked(digest, pluginID string) bool {
	if set == nil {
		return false
	}
	owner, ok := set.Packages[digest]
	return ok && owner == pluginID
}

// PackageDigests returns a copy of the revoked digest to plugin mapping.
func (set *RevocationSet) PackageDigests() map[string]string {
	digests := make(map[string]string)
	if set == nil {
		return digests
	}
	for digest, pluginID := range set.Packages {
		digests[digest] = pluginID
	}
	return digests
}

// PublisherRevoked reports whether a publisher key is revoked.
func (set *RevocationSet) PublisherRevoked(keyID string) bool {
	if set == nil {
		return false
	}
	_, ok := set.Publishers[keyID]
	return ok
}

// PublisherKeyIDs returns a copy of revoked publisher key ids.
func (set *RevocationSet) PublisherKeyIDs() []string {
	keyIDs := make([]string, 0)
	if set == nil {
		return keyIDs
	}
	for keyID := range set.Publishers {
		keyIDs = append(keyIDs, keyID)
	}
	return keyIDs
}
