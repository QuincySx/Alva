// INPUT: Disposable project/external files, shared product catalogs and MyGo headless keyboard/control events.
// OUTPUT: Pre-first-Send @ capture, external-engine attachment parity and visible retry/remove ownership regressions.
// POS: [L7 Hub UI acceptance] No real windows, user files, accounts or model calls are required for file selection.
//go:build desktop

package hubdesktop

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/egoist/mygo/ui"
)

func TestWorkspaceComposerFilesAtBeforeSessionCapturesOnlyAndKeepsText(t *testing.T) {
	service, requests := fixture(t)
	p := NewNativePage(service)
	workspace, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	p.workspace = workspace
	source := filepath.Join(workspace, "composer_notes.odd")
	if err = os.WriteFile(source, []byte("captured before first Send"), 0600); err != nil {
		t.Fatal(err)
	}
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	if err = tester.Click("Message"); err != nil {
		t.Fatal(err)
	}
	tester.Type("keep task ")
	tester.Type("@")
	waitWorkspace(t, p, tester, func() bool { return p.picker.open && !p.picker.loading && len(p.picker.rows) > 0 })
	if err = tester.Click("Select @composer_notes.odd"); err != nil {
		t.Fatal(err)
	}
	waitWorkspace(t, p, tester, func() bool { return !p.picker.open && len(p.composerFiles[""]) == 1 })
	if p.model.Draft != "keep task " || p.selected != "" || len(requests) != 0 || service.Runtime.HasActive() {
		t.Fatal("@ capture sent or replaced the manual draft", p.model.Draft, p.problem)
	}
	selection := p.composerFiles[""][0]
	record := p.composerFileRecords[selection.ID]
	if record.SourcePath != source || selection.SHA256 == "" {
		t.Fatal("missing frozen source receipt", record)
	}
	if err = os.Remove(source); err != nil {
		t.Fatal(err)
	}
	page, err := service.ReadComposerFile(t.Context(), selection, false, 0, 4096)
	if err != nil || string(page.Data) != "captured before first Send" {
		t.Fatal("selected source did not become durable immediately", page, err)
	}
	if err = tester.Click("Remove captured file composer_notes.odd"); err != nil {
		t.Fatal(err)
	}
	if len(p.composerFiles[""]) != 0 || p.model.Draft != "keep task " {
		t.Fatal("removal cleared text or retained selection")
	}
	if _, err = service.ComposerFilePrivatePath(t.Context(), selection, false); err != nil {
		t.Fatal("removing a draft deleted the retained original", err)
	}
}
func TestWorkspaceComposerFilesExternalPathDirectoryEnterSelection(t *testing.T) {
	service, requests := fixture(t)
	p := NewNativePage(service)
	external, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	source := filepath.Join(external, "external.pdf")
	if err = os.WriteFile(source, []byte("%PDF-1.7\nfixture"), 0600); err != nil {
		t.Fatal(err)
	}
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	p.openWorkspacePicker("references")
	p.picker.query = external + string(filepath.Separator)
	p.queryWorkspacePicker(false)
	waitWorkspace(t, p, tester, func() bool { return !p.picker.loading && len(p.picker.rows) == 1 })
	tester.Key(ui.Cmd, ui.KeyEnter)
	if !p.picker.open || p.selected != "" {
		t.Fatal("send shortcut bypassed picker")
	}
	p.picker.active = 0
	if err = tester.Click("Search composer items"); err != nil {
		t.Fatal(err)
	}
	tester.Compose("输入", 2)
	tester.Key(0, ui.KeyEnter)
	if !p.picker.open || len(p.composerFiles[""]) != 0 {
		t.Fatal("IME Enter selected or sent an unfinished file query")
	}
	tester.Compose("", 0)
	tester.Key(0, ui.KeyEnter)
	waitWorkspace(t, p, tester, func() bool { return !p.picker.open && len(p.composerFiles[""]) == 1 })
	record := p.composerFileRecords[p.composerFiles[""][0].ID]
	if record.Text || record.TextKey != "" || !strings.Contains(record.Processing, "no trusted PDF extractor") {
		t.Fatal("ASCII PDF was falsely sent as plain text", record)
	}
	if len(requests) != 0 || service.Runtime.HasActive() {
		t.Fatal("external path selection executed an engine")
	}
}
func TestWorkspaceComposerFilesAllEnginesNativeSelectionRetriesWithoutLosingDraft(t *testing.T) {
	service, requests := fixture(t)
	p := NewNativePage(service)
	p.composer.target = service.DefaultTarget
	p.composer.target.EngineID = "codex"
	p.model.Draft = "keep this draft while capturing"
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	source := filepath.Join(t.TempDir(), "not-created.yet")
	p.captureWorkspaceFilePaths([]string{source})
	waitWorkspace(t, p, tester, func() bool { return !p.busy && len(p.composerFilePending[""]) == 1 })
	if p.model.Draft != "keep this draft while capturing" || p.workspaceCapturedFileProblem() == "" {
		t.Fatal("failed capture lost text or source intention", p.problem)
	}
	if _, ok := tester.Find("Retry captured files"); !ok {
		t.Fatal("capture failure had no visible retry")
	}
	if err := os.WriteFile(source, []byte{0, 1, 2, 255}, 0600); err != nil {
		t.Fatal(err)
	}
	if err := tester.Click("Retry captured files"); err != nil {
		t.Fatal(err)
	}
	waitWorkspace(t, p, tester, func() bool { return !p.busy && len(p.composerFiles[""]) == 1 && len(p.composerFilePending[""]) == 0 })
	if p.workspaceAttachmentProblem() != "" || p.composerFileRecords[p.composerFiles[""][0].ID].Text || len(requests) != 0 || p.selected != "" {
		t.Fatal("Codex file capture borrowed Alva upload or falsely parsed bytes", p.problem)
	}
	before := p.captureWorkspaceComposerFiles()
	p.consumeWorkspaceComposerFiles("", []product.ComposerFileSelection{{ID: "unrelated"}})
	if len(p.composerFiles[""]) != 1 {
		t.Fatal("old receipt consumed a different captured file")
	}
	p.consumeWorkspaceComposerFiles("", before)
	if len(p.composerFiles[""]) != 0 || p.model.Draft != "keep this draft while capturing" {
		t.Fatal("captured file consumption changed text")
	}
}

type workspaceDelayedComposerFileReceipt struct {
	product.ServiceStore
	files   hub.ComposerFileStore
	entered chan hub.ComposerFile
	release chan struct{}
}

func (s workspaceDelayedComposerFileReceipt) SaveComposerFile(ctx context.Context, f hub.ComposerFile) (hub.ComposerFile, error) {
	receipt, err := s.files.SaveComposerFile(ctx, f)
	if err == nil {
		s.entered <- receipt
		<-s.release
	}
	return receipt, err
}
func (s workspaceDelayedComposerFileReceipt) ComposerFile(ctx context.Context, id string) (hub.ComposerFile, error) {
	return s.files.ComposerFile(ctx, id)
}
func (s workspaceDelayedComposerFileReceipt) ListComposerFiles(ctx context.Context, scope hub.ComposerDraftScope, limit int) ([]hub.ComposerFile, error) {
	return s.files.ListComposerFiles(ctx, scope, limit)
}
