// INPUT: context, crypto/sha256, encoding/hex, errors, strings, sync, unicode/utf8, llm/wire
// OUTPUT: MemoryReferences, NewMemoryReferences
// POS: [L3 capability library] Bounded fixed-session volatile reference backend without global hash lookup or silent eviction.
package agentcontext

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
	"strings"
	"sync"
	"unicode/utf8"
)

type MemoryReferences struct {
	mu              sync.RWMutex
	sessionID       string
	maxBytes, bytes int
	values          map[string]string
}

func NewMemoryReferences(sessionID string, maxBytes int) (*MemoryReferences, error) {
	if sessionID == "" || len(sessionID) > 256 || !utf8.ValidString(sessionID) || maxBytes <= 0 {
		return nil, errors.New("invalid volatile reference owner/budget")
	}
	return &MemoryReferences{sessionID: sessionID, maxBytes: maxBytes, values: make(map[string]string)}, nil
}
func (backend *MemoryReferences) StoreText(ctx context.Context, text string) (wire.Reference, error) {
	if err := ctx.Err(); err != nil {
		return wire.Reference{}, err
	}
	if !utf8.ValidString(text) {
		return wire.Reference{}, errors.New("reference text must be UTF-8")
	}
	hash := sha256.Sum256([]byte(text))
	owner := sha256.Sum256([]byte(backend.sessionID))
	digest := hex.EncodeToString(hash[:])
	uri := "memory-ref://" + hex.EncodeToString(owner[:16]) + "/" + digest
	backend.mu.Lock()
	defer backend.mu.Unlock()
	if _, exists := backend.values[uri]; !exists {
		if len(backend.values) >= 256 || len(text) > backend.maxBytes-backend.bytes {
			return wire.Reference{}, errors.New("volatile reference budget exceeded; text not delivered")
		}
		backend.values[uri] = text
		backend.bytes += len(text)
	}
	return wire.Reference{URI: uri, MediaType: "text/plain; charset=utf-8", SHA256: digest, Bytes: int64(len(text)), Processing: "Complete UTF-8 text stored in bounded volatile session memory; lost at restart; reference only, no media processing"}, nil
}
func (backend *MemoryReferences) ReadText(ctx context.Context, uri string, offset int64, limit int) (TextPage, error) {
	if err := ctx.Err(); err != nil {
		return TextPage{}, err
	}
	if offset < 0 || limit < 1 || limit > MaxTextPageBytes {
		return TextPage{}, errors.New("invalid reference page bounds")
	}
	backend.mu.RLock()
	text, ok := backend.values[uri]
	backend.mu.RUnlock()
	if !ok {
		return TextPage{}, errors.New("reference unavailable in this session")
	}
	if offset > int64(len(text)) || (offset < int64(len(text)) && !utf8.RuneStart(text[offset])) {
		return TextPage{}, errors.New("reference offset is not an in-range UTF-8 boundary")
	}
	end := offset + int64(limit)
	if end > int64(len(text)) {
		end = int64(len(text))
	}
	for end > offset && end < int64(len(text)) && !utf8.RuneStart(text[end]) {
		end--
	}
	if end == offset && offset < int64(len(text)) {
		return TextPage{}, errors.New("page budget cannot fit the next UTF-8 rune")
	}
	return TextPage{URI: uri, Text: text[offset:end], Offset: offset, NextOffset: end, TotalBytes: int64(len(text)), Truncated: end < int64(len(text))}, nil
}
func (backend *MemoryReferences) SearchText(ctx context.Context, uri, query string, limit int) ([]TextMatch, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if query == "" || !utf8.ValidString(query) || len(query) > 1024 || limit < 1 || limit > MaxTextMatches {
		return nil, errors.New("invalid reference literal search")
	}
	backend.mu.RLock()
	text, ok := backend.values[uri]
	backend.mu.RUnlock()
	if !ok {
		return nil, errors.New("reference unavailable in this session")
	}
	var result []TextMatch
	for at := 0; at < len(text) && len(result) < limit; {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		relative := strings.Index(text[at:], query)
		if relative < 0 {
			break
		}
		start := at + relative
		end := start + 256
		if end > len(text) {
			end = len(text)
		}
		for end > start && end < len(text) && !utf8.RuneStart(text[end]) {
			end--
		}
		result = append(result, TextMatch{URI: uri, Text: text[start:end], Offset: int64(start)})
		at = start + len(query)
	}
	return result, nil
}
