// INPUT: Disposable regular-file/Office fixtures, real private catalogs and captured selection receipts.
// OUTPUT: Pre-session durability, exact ownership/hash reads, bounded inlining/search and honest document-processing regressions.
// POS: [L7 Hub product acceptance] Never reads real user files, opens windows, executes user documents or contacts models/accounts.
package product

import (
	"archive/zip"
	"bytes"
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	productstore "github.com/QuincySx/Alva/apps/hub/sqlite"
)

func composerFixtureFile(t *testing.T, name string, data []byte) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), name)
	if err := os.WriteFile(path, data, 0600); err != nil {
		t.Fatal(err)
	}
	return path
}
func TestComposerFilePreSessionCaptureSurvivesRestartAndSourceRemoval(t *testing.T) {
	s, requests := fixture(t)
	scope := hub.ComposerDraftScope{Kind: "home", ID: "project-first-send"}
	source := composerFixtureFile(t, "README.unknown", []byte("frozen first-send text"))
	f, err := s.CaptureComposerFile(t.Context(), scope, source)
	if err != nil {
		t.Fatal(err)
	}
	if !f.Text || f.SHA256 == "" || len(requests) != 0 || s.Runtime.HasActive() {
		t.Fatal("capture ran an engine or lost identity", f)
	}
	if err = os.Remove(source); err != nil {
		t.Fatal(err)
	}
	// Reopening only the product catalog and reconstructing the product module
	// proves that original and metadata do not depend on in-memory draft maps.
	privateDirectory := s.DataDirectory
	catalogDirectory := filepath.Join(privateDirectory, "product")
	store := s.Store.(*productstore.Store)
	if err = store.Close(); err != nil {
		t.Fatal(err)
	}
	reopened, err := productstore.Open(t.Context(), catalogDirectory)
	if err != nil {
		t.Fatal(err)
	}
	defer reopened.Close()
	cold := &HubService{Store: reopened, DataDirectory: privateDirectory}
	selection := SelectedComposerFile(f)
	page, err := cold.ReadComposerFile(t.Context(), selection, false, 0, 4096)
	if err != nil || string(page.Data) != "frozen first-send text" {
		t.Fatal("restart lost captured bytes", page, err)
	}
	foreign := selection
	foreign.Scope.ID = "another-project"
	if _, err = cold.ReadComposerFile(t.Context(), foreign, false, 0, 4096); !errors.Is(err, hub.ErrComposerFileChanged) {
		t.Fatal("foreign scope borrowed a capture", err)
	}
	path, err := cold.ComposerFilePrivatePath(t.Context(), selection, false)
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(path, []byte("forged first-send text"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err = cold.ReadComposerFile(t.Context(), selection, false, 0, 4096); !errors.Is(err, hub.ErrComposerFileChanged) {
		t.Fatal("tampering was not authenticated", err)
	}
}
func TestComposerFileInlineBudgetsBinaryAndBoundedSearch(t *testing.T) {
	s, _ := fixture(t)
	scope := hub.ComposerDraftScope{Kind: "home", ID: "inline"}
	var selections []ComposerFileSelection
	for i := 0; i < 5; i++ {
		f, err := s.CaptureComposerFile(t.Context(), scope, composerFixtureFile(t, "small.txt", bytes.Repeat([]byte("a"), 4096)))
		if err != nil {
			t.Fatal(err)
		}
		selections = append(selections, SelectedComposerFile(f))
	}
	manifests, prompt, err := s.PrepareComposerFileContext(t.Context(), selections, 1<<20)
	if err != nil {
		t.Fatal(err)
	}
	total := 0
	for _, manifest := range manifests {
		total += len(manifest.InlineText)
	}
	if total != 16<<10 || strings.Contains(prompt, "chunk_sha256") || strings.Contains(prompt, "source_path") {
		t.Fatal("request inlining exceeded budgets or leaked storage metadata", total, prompt[:min(200, len(prompt))])
	}
	manifests, _, err = s.PrepareComposerFileContext(t.Context(), selections, 4095)
	if err != nil || manifests[0].InlineText != "" {
		t.Fatal("context budget failed to clamp whole-file inlining", err)
	}
	raw := append(bytes.Repeat([]byte("x"), hub.ComposerFileChunkBytes-3), []byte("needle after chunk\n")...)
	f, err := s.CaptureComposerFile(t.Context(), scope, composerFixtureFile(t, "large.go", raw))
	if err != nil {
		t.Fatal(err)
	}
	selection := SelectedComposerFile(f)
	manifests, prompt, err = s.PrepareComposerFileContext(t.Context(), []ComposerFileSelection{selection}, 16000)
	if err != nil || manifests[0].InlineText != "" || strings.Contains(prompt, "needle after chunk") {
		t.Fatal("long code entered prompt", err)
	}
	matches, err := s.SearchComposerFile(t.Context(), selection, false, "needle", 0, 5)
	if err != nil || len(matches.Matches) != 1 || matches.Matches[0].Offset != hub.ComposerFileChunkBytes-3 {
		t.Fatal("search lost cross-chunk match", matches, err)
	}
	binary, err := s.CaptureComposerFile(t.Context(), scope, composerFixtureFile(t, "photo.weird", []byte{0, 1, 2, 3, 255}))
	if err != nil || binary.Text || binary.TextKey != "" || !strings.Contains(binary.Processing, "no decoder") {
		t.Fatal("arbitrary binary was rejected or falsely parsed", binary, err)
	}
	if _, err = s.SearchComposerFile(t.Context(), SelectedComposerFile(binary), false, "x", 0, 5); err == nil {
		t.Fatal("binary was searched as text")
	}
	if _, _, err = s.PrepareComposerFileContext(t.Context(), []ComposerFileSelection{selection, selection}, 100); err == nil {
		t.Fatal("duplicate selection accepted")
	}
}
func composerDocumentFixture(t *testing.T, ext string, entries map[string]string) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), "fixture"+ext)
	file, err := os.Create(path)
	if err != nil {
		t.Fatal(err)
	}
	writer := zip.NewWriter(file)
	for name, text := range entries {
		entry, err := writer.Create(name)
		if err != nil {
			t.Fatal(err)
		}
		if _, err = entry.Write([]byte(text)); err != nil {
			t.Fatal(err)
		}
	}
	if err = errors.Join(writer.Close(), file.Close()); err != nil {
		t.Fatal(err)
	}
	return path
}
func TestComposerFileOfficeDerivativesAndUnsupportedPDFReceipts(t *testing.T) {
	s, _ := fixture(t)
	scope := hub.ComposerDraftScope{Kind: "home", ID: "documents"}
	fixtures := []struct {
		ext     string
		entries map[string]string
		want    string
	}{{".docx", map[string]string{"word/document.xml": `<document><p><t>Word fixture</t></p></document>`, "word/vbaProject.bin": "NEVER EXECUTE"}, "Word fixture"}, {".pptx", map[string]string{"ppt/slides/slide2.xml": `<slide><p><t>second</t></p></slide>`, "ppt/slides/slide1.xml": `<slide><p><t>first</t></p></slide>`}, "first\n\nsecond"}, {".xlsx", map[string]string{"xl/sharedStrings.xml": `<sst><si><t>shared fixture</t></si></sst>`, "xl/worksheets/sheet1.xml": `<worksheet><row><c r="A1" t="s"><v>0</v></c><c r="B1"><v>42</v></c></row></worksheet>`}, "A1\tshared fixture\nB1\t42"}, {".odt", map[string]string{"content.xml": `<document><p>OpenDocument fixture</p></document>`}, "OpenDocument fixture"}}
	for _, test := range fixtures {
		t.Run(test.ext, func(t *testing.T) {
			f, err := s.CaptureComposerFile(t.Context(), scope, composerDocumentFixture(t, test.ext, test.entries))
			if err != nil || f.TextKey == "" || !strings.Contains(f.Processing, "text extracted") {
				t.Fatal("supported document not processed", f, err)
			}
			page, err := s.ReadComposerFile(t.Context(), SelectedComposerFile(f), true, 0, 4096)
			if err != nil || !strings.Contains(string(page.Data), test.want) || strings.Contains(string(page.Data), "NEVER EXECUTE") {
				t.Fatal("bad inert text derivative", string(page.Data), err)
			}
		})
	}
	f, err := s.CaptureComposerFile(t.Context(), scope, composerFixtureFile(t, "fixture.pdf", []byte("%PDF-1.7\n\x00binary fixture")))
	if err != nil || f.TextKey != "" || !strings.Contains(f.Processing, "no trusted PDF extractor") {
		t.Fatal("PDF falsely claimed extraction", f, err)
	}
}
func TestComposerFileCandidatesFuzzyProjectAbsoluteExternalAndRecent(t *testing.T) {
	s, _ := fixture(t)
	scope := hub.ComposerDraftScope{Kind: "home", ID: "paths"}
	workspace, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(filepath.Join(workspace, "src"), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(workspace, "src", "composer_selection.go"), []byte("not read for suggestions"), 0600); err != nil {
		t.Fatal(err)
	}
	page, err := s.SearchComposerFileCandidates(t.Context(), scope, workspace, "csel", 10)
	if err != nil || len(page.Entries) != 1 || page.Entries[0].DisplayPath != "src/composer_selection.go" {
		t.Fatal("missing fuzzy project candidate", page, err)
	}
	external := composerFixtureFile(t, "outside.xyz", []byte("external"))
	page, err = s.SearchComposerFileCandidates(t.Context(), scope, workspace, external, 10)
	if err != nil || len(page.Entries) != 1 || page.Scanned != 1 {
		t.Fatal("explicit external path failed", page, err)
	}
	f, err := s.CaptureComposerFile(t.Context(), scope, external)
	if err != nil {
		t.Fatal(err)
	}
	if err = os.Remove(external); err != nil {
		t.Fatal(err)
	}
	page, err = s.SearchComposerFileCandidates(t.Context(), scope, "", "outside", 10)
	if err != nil || len(page.Entries) != 1 || page.Entries[0].Captured == nil || page.Entries[0].Captured.ID != f.ID {
		t.Fatal("recent immutable capture depended on source existence", page, err)
	}
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	if _, err = s.SearchComposerFileCandidates(ctx, scope, workspace, "", 5); !errors.Is(err, context.Canceled) {
		t.Fatal("candidate query ignored cancellation", err)
	}
	oversized := filepath.Join(t.TempDir(), "oversized.bin")
	file, err := os.Create(oversized)
	if err != nil {
		t.Fatal(err)
	}
	if err = file.Truncate(hub.MaxComposerFileBytes + 1); err != nil {
		t.Fatal(err)
	}
	file.Close()
	if _, err = s.CaptureComposerFile(t.Context(), scope, oversized); err == nil {
		t.Fatal("oversized file bypassed explicit resource cap")
	}
}

func TestComposerFileRestoreRequiresCommittedAssociationAndPreservesHomeOrigin(t *testing.T) {
	s, _ := fixture(t)
	scope := hub.ComposerDraftScope{Kind: "home", ID: "new-conversation"}
	f, err := s.CaptureComposerFile(t.Context(), scope, composerFixtureFile(t, "associated.txt", []byte("durable selected content")))
	if err != nil {
		t.Fatal(err)
	}
	drafts := s.Store.(hub.ComposerDraftStore)
	draft, err := drafts.SaveComposerDraft(t.Context(), hub.ComposerDraft{Scope: scope, Text: "manual prompt", References: []hub.ComposerDraftReference{SelectedComposerFile(f).DraftSelection()}}, 0)
	if err != nil {
		t.Fatal(err)
	}
	files, records, err := s.RestoreComposerFileSelections(t.Context(), draft)
	if err != nil || len(files) != 1 || records[f.ID].SHA256 != f.SHA256 {
		t.Fatal("committed file association did not restore", err)
	}
	forged := draft
	forged.Text = "not committed"
	if _, _, err = s.RestoreComposerFileSelections(t.Context(), forged); !errors.Is(err, hub.ErrComposerDraftConflict) {
		t.Fatal("uncommitted association granted captured-file access", err)
	}
	workspace, err := s.prepareSessionWorkspace("")
	if err != nil {
		t.Fatal(err)
	}
	session, _, err := s.Store.(hub.ComposerDraftAdmissionStore).CreateComposerSession(t.Context(), draft, "first send adoption", workspace, s.DefaultTarget)
	if err != nil {
		t.Fatal(err)
	}
	adopted, err := drafts.SaveComposerDraft(t.Context(), hub.ComposerDraft{Scope: hub.ComposerDraftScope{Kind: "session", ID: session.ID}, Text: draft.Text, References: draft.References}, 0)
	if err != nil {
		t.Fatal(err)
	}
	files, _, err = s.RestoreComposerFileSelections(t.Context(), adopted)
	if err != nil || len(files) != 1 || files[0].Scope != scope {
		t.Fatal("explicit first-send association retargeted original scope", files, err)
	}
}
