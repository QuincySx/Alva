// INPUT: context, bounded immutable local-file metadata and explicit composer draft ownership.
// OUTPUT: ComposerFile facts and a separate pre-session immutable file catalog storage contract.
// POS: [L7 Hub product] Retains file identity independently of native engines; metadata never grants host filesystem access.
package hub

import (
	"context"
	"encoding/hex"
	"errors"
	"path/filepath"
	"strings"
	"unicode/utf8"
)

const MaxComposerFileBytes int64 = 256 << 20

// ComposerFileChunkBytes keeps even a 256 MiB file catalog below the product
// metadata row budget while authenticating reads in bounded memory.
const ComposerFileChunkBytes = 256 << 10
const ComposerFileReadBytes = 64 << 10
const MaxComposerExtractedBytes int64 = 2 << 20

var ErrComposerFileChanged = errors.New("captured composer file identity or bytes changed")

// ComposerFile is immutable metadata for an original and its optional bounded
// text derivative. Keys name files inside the product-owned ID directory, not
// arbitrary host paths. Chunk hashes permit authenticated bounded reads.
type ComposerFile struct {
	ID              string             `json:"id"`
	Scope           ComposerDraftScope `json:"scope"`
	Name            string             `json:"name"`
	SourcePath      string             `json:"source_path"`
	MediaType       string             `json:"media_type"`
	SHA256          string             `json:"sha256"`
	Bytes           int64              `json:"bytes"`
	OriginalKey     string             `json:"original_key"`
	ChunkSHA256     []string           `json:"chunk_sha256"`
	Text            bool               `json:"is_text"`
	TextKey         string             `json:"text_key,omitempty"`
	TextBytes       int64              `json:"text_bytes,omitempty"`
	TextSHA256      string             `json:"text_sha256,omitempty"`
	TextChunkSHA256 []string           `json:"text_chunk_sha256,omitempty"`
	Processing      string             `json:"processing"`
	CreatedAt       int64              `json:"created_at"`
}

func composerSHA(value string) bool {
	data, err := hex.DecodeString(value)
	return err == nil && len(data) == 32 && strings.ToLower(value) == value
}
func composerFileText(value string, max int) bool {
	return value != "" && len(value) <= max && utf8.ValidString(value) && !strings.ContainsRune(value, 0)
}
func composerChunks(values []string, size int64) bool {
	if len(values) != int((size+ComposerFileChunkBytes-1)/ComposerFileChunkBytes) {
		return false
	}
	for _, value := range values {
		if !composerSHA(value) {
			return false
		}
	}
	return true
}
func (f ComposerFile) Validate() error {
	if err := f.Scope.Validate(); err != nil {
		return err
	}
	if !strings.HasPrefix(f.ID, "file-") || !composerFileText(f.ID, 128) || strings.ContainsAny(f.ID, "/\\") || !composerFileText(f.Name, 512) || !composerFileText(f.SourcePath, 4096) || !filepath.IsAbs(f.SourcePath) || !composerFileText(f.MediaType, 256) || !composerSHA(f.SHA256) || f.Bytes < 0 || f.Bytes > MaxComposerFileBytes || f.OriginalKey != "original" || !composerChunks(f.ChunkSHA256, f.Bytes) || !composerFileText(f.Processing, 4096) || f.CreatedAt <= 0 {
		return errors.New("invalid captured composer file metadata")
	}
	if f.TextKey == "" {
		if f.TextBytes != 0 || f.TextSHA256 != "" || len(f.TextChunkSHA256) != 0 {
			return errors.New("invalid absent composer text derivative")
		}
	} else if f.TextKey != "extracted.txt" || f.TextBytes < 0 || f.TextBytes > MaxComposerExtractedBytes || !composerSHA(f.TextSHA256) || !composerChunks(f.TextChunkSHA256, f.TextBytes) {
		return errors.New("invalid composer text derivative")
	}
	return nil
}

// ComposerFileStore saves immutable file facts. A repeated identical ID is
// idempotent; changed metadata for an existing ID must fail atomically.
type ComposerFileStore interface {
	SaveComposerFile(context.Context, ComposerFile) (ComposerFile, error)
	ComposerFile(context.Context, string) (ComposerFile, error)
	ListComposerFiles(context.Context, ComposerDraftScope, int) ([]ComposerFile, error)
}

// ComposerFileOwnershipStore reads immutable grants committed by the actual
// home-to-session transition. Draft reference DTOs cannot manufacture a grant;
// consuming or removing a draft never moves or deletes the original.
type ComposerFileOwnershipStore interface {
	ComposerFileSession(context.Context, string, string, string) (bool, error)
}
