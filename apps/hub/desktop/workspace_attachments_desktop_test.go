// INPUT: Actual SQLite product/native stores, selected local files and shared demo or rejected admission.
// OUTPUT: Deferred byte import, partial-retry ownership and pending/saved removal regressions.
// POS: [L7 Hub UI testing] Exercises business actions without an OS picker or external model account.
//go:build desktop

package hubdesktop

import (
	"bytes"
	"context"
	"fmt"
	"math"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/apps/hub/product"
	view "github.com/QuincySx/Alva/apps/ui/chat"
	"github.com/QuincySx/Alva/platform/storage"
	"github.com/egoist/mygo/ui"
)

func TestWorkspaceObserversKeepSeparateCapturedAttachmentDrafts(t *testing.T) {
	app, err := bootstrap.Open(t.Context(), bootstrap.Config{Demo: true, DataDirectory: filepath.Join(t.TempDir(), "private"), OpenStores: bootstrap.LocalStores})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = app.Close(context.Background()) })
	service := app.Service
	session, err := service.Create(t.Context(), "shared session observers", "")
	if err != nil {
		t.Fatal(err)
	}
	first, second := NewNativePage(service), NewNativePage(service)
	var tester *ui.Tester
	for _, page := range []*NativePage{first, second} {
		pageTester := ui.NewTester(workspaceStaticView(page), 1240, 850)
		page.selectSession(session.ID)
		waitWorkspace(t, page, pageTester, func() bool { return !page.polling && page.detail.Session.ID == session.ID })
		if page == first {
			tester = pageTester
		}
	}
	uploadA, err := service.UploadReader(t.Context(), session.ID, "first.txt", strings.NewReader("first original"))
	if err != nil {
		t.Fatal(err)
	}
	first.retainWorkspaceReferences(session.ID, []product.DraftReferenceSelection{uploadA.DraftSelection()})
	first.model.Draft = "send this observer's captured reference"
	frozen := first.captureWorkspaceReferences()
	uploadB, err := service.UploadReader(t.Context(), session.ID, "second.txt", strings.NewReader("second original"))
	if err != nil {
		t.Fatal(err)
	}
	second.retainWorkspaceReferences(session.ID, []product.DraftReferenceSelection{uploadB.DraftSelection()})
	// Upload establishes the actual native branch binding after the initial
	// cold session load. Observe that receipt before admitting its references.
	first.loadTail()
	waitWorkspace(t, first, tester, func() bool { return !first.polling })
	if files := first.workspaceDraftAttachments(session.ID); len(files) != 1 || files[0] != uploadA {
		t.Fatal("first observer displayed another observer's attachment", files)
	}
	if files := second.workspaceDraftAttachments(session.ID); len(files) != 1 || files[0] != uploadB {
		t.Fatal("second observer displayed another observer's attachment", files)
	}
	first.sendPreparedSnapshot(first.model.Draft, nil, frozen)
	waitWorkspace(t, first, tester, func() bool { return !first.busy && len(first.workspaceReferences[session.ID]) == 0 })
	history, err := service.Store.History(t.Context(), session.ID, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	var prompt string
	for _, entry := range history {
		prompt += entry.Input
	}
	if !strings.Contains(prompt, uploadA.Reference.URI) || strings.Contains(prompt, uploadB.Reference.URI) || len(second.workspaceReferences[session.ID]) != 1 {
		t.Fatal("captured Send crossed observer ownership", prompt)
	}
	remaining := service.DraftAttachments(session.ID)
	if len(remaining) != 1 || remaining[0] != uploadB {
		t.Fatal("Send consumed another observer's saved draft", remaining)
	}
}

func TestWorkspaceUnavailableCapturedReferenceCanBeRemovedWithoutDeletingOriginal(t *testing.T) {
	s, _ := fixture(t)
	session, err := s.Create(t.Context(), "removed reference receipt", "")
	if err != nil {
		t.Fatal(err)
	}
	draft, err := s.UploadReader(t.Context(), session.ID, "removed.txt", strings.NewReader("keep original"))
	if err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(s)
	p.selected = session.ID
	p.retainWorkspaceReferences(session.ID, []product.DraftReferenceSelection{draft.DraftSelection()})
	s.RemoveDraftAttachment(session.ID, draft.DraftID)
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	if p.workspaceAttachmentProblem() == "" || !tester.HasText("失效引用") {
		t.Fatal("missing captured reference became an invisible submission blocker")
	}
	if err = tester.Click("Remove unavailable reference " + draft.DraftID); err != nil {
		t.Fatal(err)
	}
	if len(p.workspaceReferences[session.ID]) != 0 || p.workspaceAttachmentProblem() != "" {
		t.Fatal("explicit removal did not clear the unavailable capture")
	}
	_, data, err := s.Repository.ReadArtifactFull(t.Context(), session.ID, draft.Info.SessionID, draft.Reference.URI, 1024)
	if err != nil || string(data) != "keep original" {
		t.Fatal("removing an unavailable receipt deleted its saved original", err)
	}
}

func TestWorkspaceHomeFileSelectionIsColdAndRemovalKeepsOriginal(t *testing.T) {
	service, _ := fixture(t)
	page := NewNativePage(service)
	path := filepath.Join(t.TempDir(), "selected.txt")
	if err := os.WriteFile(path, []byte("selected original"), 0o600); err != nil {
		t.Fatal(err)
	}
	missing := filepath.Join(t.TempDir(), "not-created-yet.txt")
	// This is the native dialog result boundary: selection accepts paths without
	// opening them, including a path which may disappear before explicit Send.
	page.retainWorkspaceFiles("", []string{path, missing})
	tester := ui.NewTester(workspaceStaticView(page), 1240, 850)
	if !tester.HasText("待导入") || !page.workspacePendingFiles() {
		t.Fatal("selected originals were not shown as pending import")
	}
	if err := tester.Click("Remove pending file " + filepath.Base(path)); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(path); err != nil || len(page.workspaceFiles[""]) != 1 {
		t.Fatal("removing a pending selection deleted its original", err)
	}
	sessions, err := service.Store.ListSessions(t.Context(), 8)
	if err != nil || len(sessions) != 0 || len(service.DraftAttachments("")) != 0 || service.Runtime.HasActive() {
		t.Fatal("selection or removal created product work or saved bytes", err)
	}
	other := service.DefaultTarget
	other.EngineID = "external"
	page.composer.target, page.model.Draft = other, "selected files need Alva"
	if page.workspaceAttachmentProblem() == "" {
		t.Fatal("external target did not explain pending-file incompatibility")
	}
	page.sendWorkspaceDraft()
	sessions, err = service.Store.ListSessions(t.Context(), 8)
	if err != nil || len(sessions) != 0 || !page.workspacePendingFiles() {
		t.Fatal("incompatible home target created a session or lost selection", err)
	}
}

func TestWorkspaceFirstFileFailureRetainsCreatedSessionAndRetriesInPlace(t *testing.T) {
	service, _ := fixture(t)
	if err := service.Runtime.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	page := NewNativePage(service)
	first, second := filepath.Join(t.TempDir(), "first.txt"), filepath.Join(t.TempDir(), "second.txt")
	if err := os.WriteFile(second, []byte("second"), 0o600); err != nil {
		t.Fatal(err)
	}
	page.model.Draft = "retry the first picked file"
	page.retainWorkspaceFiles("", []string{first, second})
	tester := ui.NewTester(workspaceStaticView(page), 1240, 850)
	page.sendWorkspaceDraft()
	waitWorkspace(t, page, tester, func() bool {
		return page.selected != "" && !page.busy && !page.polling && page.problem != ""
	})
	id := page.selected
	if len(page.workspaceFiles[id]) != 2 || len(service.DraftAttachments(id)) != 0 || page.model.Draft != "retry the first picked file" {
		t.Fatal("failed first import discarded the remaining selection, prompt or durable identity")
	}
	detail, err := service.Store.Detail(t.Context(), id)
	if err != nil {
		t.Fatal(err)
	}
	branch, err := product.ActiveBranch(detail)
	if err != nil || branch.NativeSessionID != "" || service.Runtime.HasActive() {
		t.Fatal("failed path import created a native session or executed", err)
	}
	page.send() // The semantic/debug Send path must not skip pending imports.
	if page.busy || len(page.workspaceFiles[id]) != 2 || len(service.DraftAttachments(id)) != 0 {
		t.Fatal("semantic Send bypassed the pending-import gate")
	}
	if err = os.WriteFile(first, []byte("first created only after failed import"), 0o600); err != nil {
		t.Fatal(err)
	}
	page.sendWorkspaceDraft()
	waitWorkspace(t, page, tester, func() bool {
		return !page.busy && !page.polling && !page.workspacePendingFiles() && page.problem != ""
	})
	rows, err := service.Store.ListSessions(t.Context(), 8)
	if err != nil || len(rows) != 1 || page.selected != id || len(service.DraftAttachments(id)) != 2 || page.model.Draft != "retry the first picked file" {
		t.Fatal("retry created another session, skipped files or consumed failed admission text", err)
	}
}

func TestWorkspaceCreateFailureRetainsPendingFilesOnHome(t *testing.T) {
	service, _ := fixture(t)
	page := NewNativePage(service)
	page.workspace = filepath.Join(t.TempDir(), "missing-workspace")
	page.model.Draft = "keep pending originals"
	path := filepath.Join(t.TempDir(), "selected.txt")
	page.retainWorkspaceFiles("", []string{path})
	tester := ui.NewTester(workspaceStaticView(page), 1240, 850)
	page.sendWorkspaceDraft()
	waitWorkspace(t, page, tester, func() bool { return !page.busy && page.problem != "" })
	if page.selected != "" || page.model.Draft != "keep pending originals" || len(page.workspaceFiles[""]) != 1 || page.workspaceFiles[""][0].path != path {
		t.Fatal("failed Create lost its home draft or pending original selection")
	}
	rows, err := service.Store.ListSessions(t.Context(), 8)
	if err != nil || len(rows) != 0 || service.Runtime.HasActive() {
		t.Fatal("failed Create saved or executed work", err)
	}
}

func TestWorkspaceFileImportUsesExplicitlySelectedBranch(t *testing.T) {
	service, _ := fixture(t)
	if err := service.Runtime.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	page := NewNativePage(service)
	target := service.DefaultTarget
	target.ModelID = "explicit-choice"
	service.Targets = append(service.Targets, target)
	page.composer.target, page.model.Draft = target, "use my selected model"
	path := filepath.Join(t.TempDir(), "selected.txt")
	if err := os.WriteFile(path, []byte("selected original"), 0o600); err != nil {
		t.Fatal(err)
	}
	page.retainWorkspaceFiles("", []string{path})
	tester := ui.NewTester(workspaceStaticView(page), 1240, 850)
	page.sendWorkspaceDraft()
	waitWorkspace(t, page, tester, func() bool {
		return page.selected != "" && !page.busy && !page.polling && page.problem != ""
	})
	branch, err := product.ActiveBranch(page.detail)
	files := service.DraftAttachments(page.selected)
	if err != nil || hub.RouteKey(branch.Target) != hub.RouteKey(target) || len(files) != 1 || files[0].BranchID != branch.ID || files[0].Info.SessionID != branch.NativeSessionID {
		t.Fatal("upload ran before the explicit target switch or used a different branch/native owner", err)
	}
	// Both models belong to one engine: the explicit choice stays on one branch.
	if len(page.detail.Branches) != 1 || page.workspaceAttachmentProblem() != "" {
		t.Fatal("explicit first-send routing did not keep one branch and a compatible reference")
	}
}

func TestWorkspacePartialImportRetryKeepsSavedReferencesAndNativeBytes(t *testing.T) {
	service, _ := fixture(t)
	if err := service.Runtime.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	page := NewNativePage(service)
	dir := t.TempDir()
	first, failed, remaining := filepath.Join(dir, "first.bin"), filepath.Join(dir, "failed.txt"), filepath.Join(dir, "remaining.txt")
	original := []byte{0, 1, 2, 3, 255, 0, 42}
	if err := os.WriteFile(first, original, 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(remaining, []byte("remaining"), 0o600); err != nil {
		t.Fatal(err)
	}
	page.model.Draft = "keep partial import"
	page.retainWorkspaceFiles("", []string{first, failed, remaining})
	tester := ui.NewTester(workspaceStaticView(page), 1240, 850)
	page.sendWorkspaceDraft()
	waitWorkspace(t, page, tester, func() bool {
		return page.selected != "" && !page.busy && !page.polling && page.problem != ""
	})
	id := page.selected
	saved := service.DraftAttachments(id)
	if len(saved) != 1 || len(page.workspaceFiles[id]) != 2 || page.workspaceFiles[id][0].path != failed || page.workspaceAttachmentProblem() != "" {
		t.Fatal("partial failure lost order, saved reference or refreshed native ownership")
	}
	firstID := saved[0].Info.ID
	if err := os.WriteFile(first, []byte("must never reimport this changed original"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(failed, []byte("retry successful"), 0o600); err != nil {
		t.Fatal(err)
	}
	page.sendWorkspaceDraft()
	waitWorkspace(t, page, tester, func() bool {
		return !page.busy && !page.polling && !page.workspacePendingFiles() && page.problem != ""
	})
	saved = service.DraftAttachments(id)
	if len(saved) != 3 || saved[0].Info.ID != firstID || page.model.Draft != "keep partial import" {
		t.Fatal("retry duplicated an imported file or lost failed-admission input")
	}
	_, data, err := service.Repository.ReadArtifactFull(t.Context(), id, saved[0].Info.SessionID, saved[0].Reference.URI, 1024)
	if err != nil || !bytes.Equal(data, original) {
		t.Fatal("first original was overwritten or not durably saved byte-for-byte", err)
	}
	index, err := service.Repository.PageArtifacts(t.Context(), id, saved[0].Info.SessionID, "input-attachment", "", 0, 0, 8)
	if err != nil || len(index.Entries) != 3 {
		t.Fatal("retry reuploaded a successful original", err)
	}
	if err = tester.Click("Remove draft reference " + filepath.Base(first)); err != nil {
		t.Fatal(err)
	}
	_, data, err = service.Repository.ReadArtifactFull(t.Context(), id, saved[0].Info.SessionID, saved[0].Reference.URI, 1024)
	if err != nil || !bytes.Equal(data, original) || len(service.DraftAttachments(id)) != 2 {
		t.Fatal("removing a draft reference deleted its saved original", err)
	}
	rows, err := service.Store.ListSessions(t.Context(), 8)
	if err != nil || len(rows) != 1 || page.selected != id || service.Runtime.HasActive() {
		t.Fatal("partial retry changed durable identity or executed after rejection", err)
	}
}

type workspaceDelayedArtifact struct {
	storage.Repository
	entered, release chan struct{}
	once             sync.Once
}

func (r *workspaceDelayedArtifact) PutArtifact(ctx context.Context, owner, id string, input storage.ArtifactInput) (storage.ArtifactInfo, error) {
	r.once.Do(func() {
		close(r.entered)
		select {
		case <-r.release:
		case <-ctx.Done():
		}
	})
	return r.Repository.PutArtifact(ctx, owner, id, input)
}

func TestWorkspaceHomeFilesUseSharedDemoAndKeepNewEdits(t *testing.T) {
	app, err := bootstrap.Open(t.Context(), bootstrap.Config{Demo: true, DataDirectory: filepath.Join(t.TempDir(), "private"), OpenStores: bootstrap.LocalStores})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = app.Close(context.Background()) })
	wrapped := &workspaceDelayedArtifact{Repository: app.Service.Repository, entered: make(chan struct{}), release: make(chan struct{})}
	app.Service.Repository = wrapped
	page := NewNativePage(app.Service)
	path := filepath.Join(t.TempDir(), "notes.txt")
	if err = os.WriteFile(path, []byte("selected but not imported"), 0o600); err != nil {
		t.Fatal(err)
	}
	page.retainWorkspaceFiles("", []string{path})
	// Selection must not cache the original. The bytes at explicit import time
	// are the bytes which the existing service validates and saves.
	imported := []byte("original bytes at the explicit Send boundary")
	if err = os.WriteFile(path, imported, 0o600); err != nil {
		t.Fatal(err)
	}
	page.model.Draft = "captured first prompt"
	tester := ui.NewTester(workspaceStaticView(page), 1240, 850)
	page.sendWorkspaceDraft()
	var release sync.Once
	defer release.Do(func() { close(wrapped.release) })
	awaitWorkspaceAdmissionBoundary(t, tester, wrapped.entered)
	page.model.Draft = "new edit while first-send import settles"
	release.Do(func() { close(wrapped.release) })
	waitWorkspace(t, page, tester, func() bool {
		if page.selected == "" {
			return false
		}
		requests, err := app.Service.PendingApprovals(t.Context(), page.selected)
		if err != nil {
			t.Fatal(err)
		}
		for _, request := range requests {
			if err = app.Service.RespondApproval(t.Context(), page.selected, request.ID, true); err != nil {
				t.Fatal(err)
			}
		}
		return !page.busy && !page.polling && !app.Runtime.HasActive() && page.model.Status == view.Completed
	})
	if page.model.Draft != "new edit while first-send import settles" || page.workspacePendingFiles() || len(app.Service.DraftAttachments(page.selected)) != 0 {
		t.Fatalf("admission overwrote a new edit or retained consumed attachment drafts: text=%q pending=%v attachments=%d problem=%s", page.model.Draft, page.workspacePendingFiles(), len(app.Service.DraftAttachments(page.selected)), page.problem)
	}
	history, err := app.Store.History(t.Context(), page.selected, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	var input string
	for _, entry := range history {
		input += entry.Input
	}
	if !strings.Contains(input, "captured first prompt") || strings.Contains(input, "new edit while") || !strings.Contains(input, "User-selected files") || strings.Contains(input, string(imported)) {
		t.Fatal("first Send did not submit its captured prompt with reference-only attachment data")
	}
	branch, err := product.ActiveBranch(page.detail)
	if err != nil {
		t.Fatal(err)
	}
	index, err := app.Repository.PageArtifacts(t.Context(), page.selected, branch.NativeSessionID, "input-attachment", "", 0, 0, 8)
	if err != nil || len(index.Entries) != 1 {
		t.Fatal("demo did not save exactly one raw attachment", err)
	}
	_, data, err := app.Repository.ReadArtifactFull(t.Context(), page.selected, branch.NativeSessionID, index.Entries[0].Info.Reference().URI, 1024)
	if err != nil || !bytes.Equal(data, imported) {
		t.Fatal("demo did not preserve exact selected original bytes", err)
	}
	page.startWorkspaceTask()
	if page.model.Draft != "" {
		t.Fatal("created-session edit was duplicated into the next home draft")
	}
}

func TestWorkspaceSavedAttachmentMismatchBlocksSemanticSend(t *testing.T) {
	service, _ := fixture(t)
	session, err := service.Create(t.Context(), "attachment owner", "")
	if err != nil {
		t.Fatal(err)
	}
	attachment, err := service.UploadReader(t.Context(), session.ID, "original.txt", strings.NewReader("original"))
	if err != nil {
		t.Fatal(err)
	}
	page := NewNativePage(service)
	page.selected, page.model.Draft = session.ID, "guard parked references"
	page.retainWorkspaceReferences(session.ID, []product.DraftReferenceSelection{attachment.DraftSelection()})
	page.detail, err = service.Store.Detail(t.Context(), session.ID)
	if err != nil {
		t.Fatal(err)
	}
	page.applyDetail()
	for _, mismatch := range []string{"branch", "native"} {
		t.Run(mismatch, func(t *testing.T) {
			actual := page.detail.Branches[0]
			if mismatch == "branch" {
				page.detail.Session.ActiveBranchID = "other-branch"
				page.detail.Branches = append(page.detail.Branches, hub.Branch{ID: "other-branch", NativeSessionID: actual.NativeSessionID, Target: actual.Target})
			} else {
				page.detail.Session.ActiveBranchID = actual.ID
				page.detail.Branches[0].NativeSessionID = "different-native-owner"
			}
			if page.workspaceAttachmentProblem() == "" {
				t.Fatal("incompatible saved reference was not explained")
			}
			page.sendWorkspaceDraft()
			page.send()
			if page.busy || service.Runtime.HasActive() || len(service.DraftAttachments(session.ID)) != 1 || page.model.Draft != "guard parked references" {
				t.Fatal("semantic Send bypassed attachment ownership guard")
			}
			page.detail.Branches[0] = actual
		})
	}
	if saved := service.DraftAttachments(session.ID); saved[0].Info.ID != attachment.Info.ID {
		t.Fatal("mismatch guard removed the actual saved reference")
	}
}

func TestWorkspaceAttachmentFoundationGeometryAndBoundedRows(t *testing.T) {
	for _, width := range []int{1240, 720} {
		t.Run(fmt.Sprint(width), func(t *testing.T) {
			service, requests := fixture(t)
			p := NewNativePage(service)
			name := strings.Repeat("long-project-name-", 24) + ".txt"
			paths := []string{filepath.Join(t.TempDir(), name)}
			for i := 1; i < 8; i++ {
				paths = append(paths, filepath.Join(t.TempDir(), fmt.Sprintf("selected-%d.txt", i)))
			}
			p.retainWorkspaceFiles("", paths)
			tester := ui.NewTester(workspaceStaticView(p), width, 680)
			composer, _ := tester.Find("Task composer")
			viewport, _ := tester.Find("Composer attachments")
			card, present := tester.Find("Attachment " + name)
			icon, _ := tester.Find("Attachment source icon pending-0")
			copy, _ := tester.Find("Attachment copy pending-0")
			remove, _ := tester.Find("Remove pending file " + name)
			if !present || card.H != 56 || card.W != 320 || copy.H != 40 || icon.W != 16 || icon.H != 16 || remove.W != 28 || remove.H != 28 {
				t.Fatal("Attachment does not match Foundation row/copy/icon/action geometry", card, copy, icon, remove)
			}
			if icon.X != card.X+8 || copy.X != icon.X+24 || card.X+card.W-remove.X-remove.W != 8 || copy.X+copy.W+8 != remove.X {
				t.Fatal("Attachment lost its symmetric padding or 8px gaps", card, icon, copy, remove)
			}
			if math.Abs(float64(icon.Y+icon.H/2-card.Y-card.H/2)) > .5 || math.Abs(float64(remove.Y+remove.H/2-card.Y-card.H/2)) > .5 {
				t.Fatal("Attachment icon actions are not vertically centered", card, icon, remove)
			}
			if viewport.H != 126 || viewport.X < composer.X || viewport.X+viewport.W > composer.X+composer.W || card.X+card.W > viewport.X+viewport.W {
				t.Fatal("long path expanded the composer or attachments were not bounded to two rows", composer, viewport, card)
			}
			before, _ := tester.Find("Send")
			tester.Scroll(viewport.X+viewport.W/2, viewport.Y+viewport.H/2, 0, 1000)
			after, _ := tester.Find("Send")
			last, _ := tester.Find("Remove pending file selected-7.txt")
			if before != after || last.Y < viewport.Y || last.Y+last.H > viewport.Y+viewport.H {
				t.Fatal("attachment scrolling did not expose the final Remove action while retaining Send", viewport, last, before, after)
			}
			if err := tester.Click("Remove pending file selected-7.txt"); err != nil || len(p.workspaceFiles[""]) != 7 {
				t.Fatal("scrolled attachment did not remove its own cold selection", err)
			}
			if service.Runtime.HasActive() || len(requests) != 0 || len(service.DraftAttachments("")) != 0 {
				t.Fatal("attachment layout or scrolling imported or executed selected paths")
			}
		})
	}
}
