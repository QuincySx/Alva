// INPUT: Exact trusted Turn file selections, immutable product file catalog and actual native reference backend.
// OUTPUT: A session reference backend with actual-preparation byte verification of selected originals/derivatives.
// POS: [L7 product] Reuses SDK context_read/context_search without granting arbitrary paths or another model context store.
package product

import (
	"context"
	"errors"
	"path/filepath"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type composerReferenceFile struct {
	selection  ComposerFileSelection
	derivative bool
}
type composerReferences struct {
	service *HubService
	ownerID string
	native  agentcontext.ReferenceBackend
	files   map[string]composerReferenceFile
}

// ComposerFileReferences is installed by the actual admitted Alva factory.
// Files come from that Turn's typed immutable inputs, never from prompt text.
func (s *HubService) ComposerFileReferences(ctx context.Context, ownerID string, files []engine.InputFile, native agentcontext.ReferenceBackend) (agentcontext.ReferenceBackend, error) {
	if native == nil || ownerID == "" {
		return nil, errors.New("actual native reference backend required")
	}
	if err := engine.ValidateInputFiles(files); err != nil {
		return nil, err
	}
	b := &composerReferences{service: s, ownerID: ownerID, native: native, files: make(map[string]composerReferenceFile)}
	if len(files) == 0 {
		return b, nil
	}
	store, err := s.composerFileStore()
	if err != nil {
		return nil, err
	}
	for _, input := range files {
		f, err := store.ComposerFile(ctx, input.ID)
		if err != nil {
			return nil, err
		}
		if err = f.Validate(); err != nil {
			return nil, err
		}
		if err = s.ValidateComposerFileOwner(ctx, ownerID, f); err != nil {
			return nil, err
		}
		dir, err := s.composerFileDirectory(f.ID)
		if err != nil {
			return nil, err
		}
		if input.Name != f.Name || input.MediaType != f.MediaType || input.SHA256 != f.SHA256 || input.Bytes != f.Bytes || input.URI != composerReference(f, false).URI || input.Path != filepath.Join(dir, f.OriginalKey) {
			return nil, hub.ErrComposerFileChanged
		}
		selection := SelectedComposerFile(f)
		if _, err = s.ComposerFilePrivatePath(ctx, selection, false); err != nil {
			return nil, err
		}
		if f.Text {
			b.files[input.URI] = composerReferenceFile{selection: selection}
		}
		if input.TextPath != "" {
			key, size, sha, uri := f.TextKey, f.TextBytes, f.TextSHA256, composerReference(f, true).URI
			if f.Text {
				key, size, sha, uri = f.OriginalKey, f.Bytes, f.SHA256, input.URI
			}
			if key == "" || input.TextPath != filepath.Join(dir, key) || input.TextBytes != size || input.TextSHA256 != sha || input.TextURI != uri {
				return nil, hub.ErrComposerFileChanged
			}
			if !f.Text {
				if _, err = s.ComposerFilePrivatePath(ctx, selection, true); err != nil {
					return nil, err
				}
			}
			b.files[uri] = composerReferenceFile{selection: selection, derivative: !f.Text}
		}
	}
	return b, nil
}

// resolve also admits previously selected session files by durable ownership.
// Follow-up context_read/search need no automatic image or prompt replay.
func (b *composerReferences) resolve(ctx context.Context, uri string) (composerReferenceFile, bool, error) {
	if f, ok := b.files[uri]; ok {
		return f, true, nil
	}
	if !strings.HasPrefix(uri, "composer://") {
		return composerReferenceFile{}, false, nil
	}
	parts := strings.Split(strings.TrimPrefix(uri, "composer://"), "/")
	if len(parts) < 1 || len(parts) > 2 || parts[0] == "" || len(parts) == 2 && parts[1] != "text" {
		return composerReferenceFile{}, true, storage.ErrNotFound
	}
	store, err := b.service.composerFileStore()
	if err != nil {
		return composerReferenceFile{}, true, err
	}
	f, err := store.ComposerFile(ctx, parts[0])
	if err != nil {
		if errors.Is(err, hub.ErrNotFound) {
			return composerReferenceFile{}, true, storage.ErrNotFound
		}
		return composerReferenceFile{}, true, err
	}
	if err = b.service.ValidateComposerFileOwner(ctx, b.ownerID, f); err != nil {
		return composerReferenceFile{}, true, err
	}
	derivative := len(parts) == 2
	if derivative && f.TextKey == "" || !derivative && !f.Text {
		return composerReferenceFile{}, true, storage.ErrNotFound
	}
	return composerReferenceFile{selection: SelectedComposerFile(f), derivative: derivative}, true, nil
}

func (b *composerReferences) StoreText(ctx context.Context, text string) (wire.Reference, error) {
	return b.native.StoreText(ctx, text)
}
func (b *composerReferences) ReadText(ctx context.Context, uri string, offset int64, limit int) (agentcontext.TextPage, error) {
	f, known, err := b.resolve(ctx, uri)
	if err != nil {
		return agentcontext.TextPage{}, err
	}
	if !known {
		if strings.HasPrefix(uri, "composer://") {
			return agentcontext.TextPage{}, storage.ErrNotFound
		}
		return b.native.ReadText(ctx, uri, offset, limit)
	}
	if offset < 0 || limit < 1 || limit > agentcontext.MaxTextPageBytes {
		return agentcontext.TextPage{}, errors.New("invalid captured reference page")
	}
	page, err := b.service.ReadComposerFile(ctx, f.selection, f.derivative, offset, limit+3)
	if err != nil {
		return agentcontext.TextPage{}, err
	}
	data := page.Data
	if len(data) > 0 && !utf8.RuneStart(data[0]) {
		return agentcontext.TextPage{}, errors.New("reference offset is not a UTF-8 boundary")
	}
	end := min(limit, len(data))
	for end > 0 && end < len(data) && !utf8.RuneStart(data[end]) {
		end--
	}
	if end == 0 && len(data) > 0 || !utf8.Valid(data[:end]) {
		return agentcontext.TextPage{}, errors.New("reference page is not valid UTF-8")
	}
	next := offset + int64(end)
	return agentcontext.TextPage{URI: uri, Text: string(data[:end]), Offset: offset, NextOffset: next, TotalBytes: page.Info.Bytes, Truncated: next < page.Info.Bytes}, nil
}
func (b *composerReferences) SearchText(ctx context.Context, uri, query string, limit int) ([]agentcontext.TextMatch, error) {
	f, known, err := b.resolve(ctx, uri)
	if err != nil {
		return nil, err
	}
	if !known {
		if strings.HasPrefix(uri, "composer://") {
			return nil, storage.ErrNotFound
		}
		return b.native.SearchText(ctx, uri, query, limit)
	}
	if limit < 1 || limit > agentcontext.MaxTextMatches {
		return nil, errors.New("invalid captured reference search limit")
	}
	var matches []agentcontext.TextMatch
	var offset int64
	for {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		page, err := b.service.SearchComposerFile(ctx, f.selection, f.derivative, query, offset, limit-len(matches))
		if err != nil {
			return nil, err
		}
		for _, match := range page.Matches {
			text := match.Text
			if len(text) > 256 {
				text = text[:256]
				for !utf8.ValidString(text) {
					text = text[:len(text)-1]
				}
			}
			matches = append(matches, agentcontext.TextMatch{URI: uri, Text: text, Offset: match.Offset})
		}
		if len(matches) >= limit || !page.Truncated {
			return matches, nil
		}
		if page.NextOffset <= offset {
			return nil, errors.New("captured reference search made no progress")
		}
		offset = page.NextOffset
	}
}
