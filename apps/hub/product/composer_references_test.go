// INPUT: Captured UTF-8 files, immutable catalogs and the actual SDK reference-reader validation boundary.
// OUTPUT: Bounded Unicode pages, long-file search, native delegation and forged-selection/tamper regressions.
// POS: [L7 product testing] Proves selected-file reads use existing context retrieval rather than host-path prompt parsing.
package product

import (
	"context"
	"errors"
	"os"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type capturedNativeReferences struct{ reads int }

func (*capturedNativeReferences) StoreText(context.Context, string) (wire.Reference, error) {
	return wire.Reference{URI: "native://stored"}, nil
}
func (n *capturedNativeReferences) ReadText(_ context.Context, uri string, offset int64, limit int) (agentcontext.TextPage, error) {
	n.reads++
	return agentcontext.TextPage{URI: uri, Offset: offset, NextOffset: offset, TotalBytes: offset}, nil
}
func (*capturedNativeReferences) SearchText(context.Context, string, string, int) ([]agentcontext.TextMatch, error) {
	return nil, nil
}

func TestComposerReferencesUseActualSDKPagingAndBoundedLongFileSearch(t *testing.T) {
	s, _ := fixture(t)
	session, err := s.Create(t.Context(), "captured references", "")
	if err != nil {
		t.Fatal(err)
	}
	text := "a中" + strings.Repeat("x", (1<<20)+25) + "needle结束"
	f, err := s.CaptureComposerFile(t.Context(), hub.ComposerDraftScope{Kind: "session", ID: session.ID}, composerFixtureFile(t, "long.txt", []byte(text)))
	if err != nil {
		t.Fatal(err)
	}
	path, err := s.ComposerFilePrivatePath(t.Context(), SelectedComposerFile(f), false)
	if err != nil {
		t.Fatal(err)
	}
	input := engine.InputFile{ID: f.ID, Name: f.Name, URI: composerReference(f, false).URI, Path: path, MediaType: f.MediaType, SHA256: f.SHA256, Bytes: f.Bytes, TextURI: composerReference(f, false).URI, TextPath: path, TextSHA256: f.SHA256, TextBytes: f.Bytes}
	native := &capturedNativeReferences{}
	backend, err := s.ComposerFileReferences(t.Context(), session.ID, []engine.InputFile{input}, native)
	if err != nil {
		t.Fatal(err)
	}
	reader := agentcontext.ReferenceReader{Backend: backend}
	resumed, err := s.ComposerFileReferences(t.Context(), session.ID, nil, native)
	if err != nil {
		t.Fatal(err)
	}
	if page, err := (agentcontext.ReferenceReader{Backend: resumed}).Read(t.Context(), input.URI, 0, 4); err != nil || page.Text != "a中" {
		t.Fatal("follow-up lost an explicitly selected original", page, err)
	}
	foreign, err := s.Create(t.Context(), "different session", "")
	if err != nil {
		t.Fatal(err)
	}
	isolated, err := s.ComposerFileReferences(t.Context(), foreign.ID, nil, native)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = isolated.ReadText(t.Context(), input.URI, 0, 4); err == nil {
		t.Fatal("another session borrowed a durable file reference")
	}
	page, err := reader.Read(t.Context(), input.URI, 0, 3)
	if err != nil || page.Text != "a" || page.NextOffset != 1 || !page.Truncated {
		t.Fatal("UTF-8 page split a rune", page, err)
	}
	page, err = reader.Read(t.Context(), input.URI, 1, 3)
	if err != nil || page.Text != "中" || page.NextOffset != 4 {
		t.Fatal("continuation lost Unicode", page, err)
	}
	if _, err = reader.Read(t.Context(), input.URI, 2, 3); err == nil {
		t.Fatal("mid-rune offset accepted")
	}
	matches, err := reader.Search(t.Context(), input.URI, "needle", 2)
	if err != nil || len(matches) != 1 || matches[0].Offset != int64(strings.Index(text, "needle")) {
		t.Fatal("long selected-file search failed", matches, err)
	}
	if _, err = reader.Read(t.Context(), "composer://another", 0, 20); !errors.Is(err, storage.ErrNotFound) || native.reads != 0 {
		t.Fatal("unknown capture delegated to native authority", err)
	}
	if _, err = reader.Read(t.Context(), "native://known", 0, 20); err != nil || native.reads != 1 {
		t.Fatal("native references lost delegation", err)
	}
	forged := input
	forged.Path += ".forged"
	if _, err = s.ComposerFileReferences(t.Context(), session.ID, []engine.InputFile{forged}, native); !errors.Is(err, hub.ErrComposerFileChanged) {
		t.Fatal("forged path accepted", err)
	}
	if err = os.WriteFile(path, []byte("tampered"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err = reader.Read(t.Context(), input.URI, 0, 20); !errors.Is(err, hub.ErrComposerFileChanged) {
		t.Fatal("tampered private bytes read", err)
	}
}
