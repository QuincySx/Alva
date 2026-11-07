// INPUT: Two independent native pages and real shared product/runtime authority
// OUTPUT: Independent drafts, guarded resource returns, closed-origin release and in-flight admission protection
// POS: [L7 Hub product] Headless window-controller behavior; actual window/AX acceptance is separately observed.
//go:build desktop

package hubdesktop

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/platform/storage"
	"github.com/egoist/mygo/ui"
)

func TestResourceReturnWaitsForActualCloseAndPreservesOriginContext(t *testing.T) {
	service, _ := fixture(t)
	origin, popup := NewNativePage(service), NewNativePage(service)
	origin.selected, origin.pane, origin.model.Draft = "other-session", "connections", "unsaved workbench prompt"
	popup.selected, popup.resources.popout, popup.model.Draft = "resource-session", "resource-session\x00tab", "shared session draft"
	m := &nativeWindows{
		service: service, pages: map[*NativePage]struct{}{origin: {}, popup: {}},
		resourceOrigins: map[*NativePage]*NativePage{popup: origin},
	}
	if !m.prepareResourceReturn(popup) || len(m.pages) != 2 || m.resourceOrigins[popup] != origin {
		t.Fatal("requesting a return prematurely disposed its observer or lost its origin")
	}
	// A normal Close guard may still be waiting for a real accepted save.
	// Only OnClosed invokes forgetPage; a request alone must keep both owners.
	if m.resourceReturns[popup] != popup.resources.popout || origin.selected != "other-session" || origin.pane != "connections" || origin.model.Draft != "unsaved workbench prompt" {
		t.Fatal("return navigated the workbench or replaced its unsaved form/draft")
	}
	if target := m.forgetPage(popup); target != origin || len(m.pages) != 1 || len(m.resourceOrigins) != 0 || len(m.resourceReturns) != 0 {
		t.Fatal("real close did not return focus to the live origin and release popup relations")
	}
	if popup.model.Draft != "shared session draft" || service.Runtime.HasActive() {
		t.Fatal("window return changed the draft or executed a Turn")
	}
}

func TestResourceReturnAfterOriginClosedKeepsCurrentObserver(t *testing.T) {
	service, _ := fixture(t)
	origin, popup := NewNativePage(service), NewNativePage(service)
	popup.selected, popup.resources.session, popup.resources.popout = "session", "session", "session\x00tab"
	popup.model.Draft = "keep this draft"
	state := &workspaceResourceSession{view: hub.WorkspaceView{SessionID: "session", Focused: true}, loaded: true}
	popup.resources.sessions = map[string]*workspaceResourceSession{"session": state}
	popup.returnResourceWindow = func() { t.Fatal("old popup callback unexpectedly called") }
	m := &nativeWindows{
		service: service, pages: map[*NativePage]struct{}{origin: {}, popup: {}},
		resourceOrigins: map[*NativePage]*NativePage{popup: origin},
	}
	if target := m.forgetPage(origin); target != nil || len(m.resourceOrigins) != 0 {
		t.Fatal("closed workbench is still retained by its popup")
	}
	if m.prepareResourceReturn(popup) || len(m.pages) != 1 || popup.resources.popout != "" || popup.returnResourceWindow != nil || state.view.Focused || state.changed != 1 {
		t.Fatal("missing origin did not restore the current observer's workbench in place")
	}
	if popup.selected != "session" || popup.model.Draft != "keep this draft" || service.Runtime.HasActive() {
		t.Fatal("restoring the remaining workbench changed its session/draft or executed")
	}
}

func TestResourceReturnRechecksLiveFallbackAndIgnoresStaleNavigation(t *testing.T) {
	for _, navigated := range []bool{false, true} {
		t.Run(fmt.Sprintf("navigated=%v", navigated), func(t *testing.T) {
			service, _ := fixture(t)
			origin, popup, matching, unrelated := NewNativePage(service), NewNativePage(service), NewNativePage(service), NewNativePage(service)
			origin.selected, popup.selected, matching.selected, unrelated.selected = "old", "resource", "resource", "unrelated"
			popup.resources.popout = "resource\x00tab"
			m := &nativeWindows{
				service: service, pages: map[*NativePage]struct{}{origin: {}, popup: {}, matching: {}, unrelated: {}},
				resourceOrigins: map[*NativePage]*NativePage{popup: origin},
			}
			if !m.prepareResourceReturn(popup) {
				t.Fatal("live origin did not accept return request")
			}
			m.forgetPage(origin)
			if navigated {
				popup.resources.popout = "" // selectWorkspaceResources clears the old window-local marker.
			}
			target := m.forgetPage(popup)
			if !navigated && target != matching || navigated && target != nil || len(m.resourceOrigins) != 0 || len(m.resourceReturns) != 0 {
				t.Fatal("late close used a closed origin or a return from before navigation")
			}
			if matching.selected != "resource" || unrelated.selected != "unrelated" {
				t.Fatal("fallback selection navigated another window")
			}
		})
	}
}

func TestClosingObserverReleasesOnlyItsUnsubmittedStagingBudget(t *testing.T) {
	s, _ := fixture(t)
	session, err := s.Create(t.Context(), "window staging owners", "")
	if err != nil {
		t.Fatal(err)
	}
	first, second := NewNativePage(s), NewNativePage(s)
	var original product.DraftAttachment
	for i := 0; i < 7; i++ {
		draft, err := s.UploadReader(t.Context(), session.ID, fmt.Sprintf("first-%d.txt", i), strings.NewReader("first observer original"))
		if err != nil {
			t.Fatal(err)
		}
		first.retainWorkspaceReferences(session.ID, []product.DraftReferenceSelection{draft.DraftSelection()})
		original = draft
	}
	other, err := s.UploadReader(t.Context(), session.ID, "second.txt", strings.NewReader("second observer original"))
	if err != nil {
		t.Fatal(err)
	}
	second.retainWorkspaceReferences(session.ID, []product.DraftReferenceSelection{other.DraftSelection()})
	if _, err = s.UploadReader(t.Context(), session.ID, "full.txt", strings.NewReader("must not fit")); err == nil {
		t.Fatal("fixture did not fill the shared staging limit")
	}
	first.referenceOwner.close()
	first.referenceOwner.close() // Repeated native disposal is harmless.
	remaining := s.DraftAttachments(session.ID)
	if len(remaining) != 1 || remaining[0] != other {
		t.Fatal("closing an observer removed another observer's claims", remaining)
	}
	if _, err = s.UploadReader(t.Context(), session.ID, "new.txt", strings.NewReader("new observer selection")); err != nil {
		t.Fatal("closed observer stranded the shared staging budget", err)
	}
	_, data, err := s.Repository.ReadArtifactFull(t.Context(), session.ID, original.Info.SessionID, original.Reference.URI, 1024)
	if err != nil || string(data) != "first observer original" || s.Runtime.HasActive() {
		t.Fatal("closing a draft observer deleted an original or executed", err)
	}
}

func TestClosingObserverRetainsCapturedClaimsUntilRealAdmissionReturns(t *testing.T) {
	for _, canceled := range []bool{false, true} {
		t.Run(fmt.Sprintf("canceled=%t", canceled), func(t *testing.T) {
			app, err := bootstrap.Open(t.Context(), bootstrap.Config{Demo: true, DataDirectory: filepath.Join(t.TempDir(), "private"), OpenStores: bootstrap.LocalStores})
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() { _ = app.Close(context.Background()) })
			session, err := app.Service.Create(t.Context(), "closing during admission", "")
			if err != nil {
				t.Fatal(err)
			}
			draft, err := app.Service.UploadReader(t.Context(), session.ID, "captured.txt", strings.NewReader("retained original"))
			if err != nil {
				t.Fatal(err)
			}
			p := NewNativePage(app.Service)
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			p.ctx = ctx
			tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
			p.selectSession(session.ID)
			waitWorkspace(t, p, tester, func() bool { return !p.polling && p.detail.Session.ID == session.ID })
			p.model.Draft = "captured Send while closing"
			p.retainWorkspaceReferences(session.ID, []product.DraftReferenceSelection{draft.DraftSelection()})
			wrapped := &workspaceDelayedDetail{ServiceStore: app.Service.Store, entered: make(chan struct{}), release: make(chan struct{})}
			wrapped.block.Store(true)
			app.Service.Store = wrapped
			p.sendSnapshot(p.model.Draft)
			var release sync.Once
			defer release.Do(func() { close(wrapped.release) })
			awaitWorkspaceAdmissionBoundary(t, tester, wrapped.entered)
			p.referenceOwner.close()
			if len(app.Service.DraftAttachments(session.ID)) != 1 {
				t.Fatal("observer disposal removed a captured claim during admission")
			}
			if canceled {
				cancel()
			}
			release.Do(func() { close(wrapped.release) })
			waitClosedReferenceOwner(t, p)
			if len(app.Service.DraftAttachments(session.ID)) != 0 {
				t.Fatal("closed observer's returned operation stranded a staging claim")
			}
			history, err := app.Service.Store.History(t.Context(), session.ID, 0, 128)
			if err != nil {
				t.Fatal(err)
			}
			var input string
			for _, entry := range history {
				input += entry.Input
			}
			if canceled && input != "" || !canceled && !strings.Contains(input, draft.Reference.URI) {
				t.Fatal("closing changed the actual admission outcome", input)
			}
		})
	}
}

type workspaceDelayedArtifactReceipt struct {
	storage.Repository
	entered, release chan struct{}
}

func (r workspaceDelayedArtifactReceipt) PutArtifact(ctx context.Context, owner, id string, input storage.ArtifactInput) (storage.ArtifactInfo, error) {
	info, err := r.Repository.PutArtifact(ctx, owner, id, input)
	close(r.entered)
	<-r.release // Freeze a real committed receipt, including after caller cancellation.
	return info, err
}

func TestClosedObserverReleasesUploadReceiptArrivingAfterCancellation(t *testing.T) {
	s, _ := fixture(t)
	session, err := s.Create(t.Context(), "late original receipt", "")
	if err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(s)
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	p.ctx, p.selected, p.model.Draft = ctx, session.ID, "retain the original"
	p.detail, err = s.Store.Detail(t.Context(), session.ID)
	if err != nil {
		t.Fatal(err)
	}
	p.applyDetail()
	path := filepath.Join(t.TempDir(), "late.txt")
	if err = os.WriteFile(path, []byte("late retained original"), 0600); err != nil {
		t.Fatal(err)
	}
	p.retainWorkspaceFiles(session.ID, []string{path})
	wrapped := workspaceDelayedArtifactReceipt{Repository: s.Repository, entered: make(chan struct{}), release: make(chan struct{})}
	s.Repository = wrapped
	p.sendWorkspacePendingFiles()
	var release sync.Once
	defer release.Do(func() { close(wrapped.release) })
	select {
	case <-wrapped.entered:
	case <-time.After(time.Second):
		t.Fatal("upload did not durably save its selected original")
	}
	p.referenceOwner.close()
	cancel()
	release.Do(func() { close(wrapped.release) })
	waitClosedReferenceOwner(t, p)
	if len(s.DraftAttachments(session.ID)) != 0 || s.Runtime.HasActive() {
		t.Fatal("late receipt leaked staging budget or started a Turn")
	}
	detail, err := s.Store.Detail(t.Context(), session.ID)
	if err != nil {
		t.Fatal(err)
	}
	branch, err := product.ActiveBranch(detail)
	if err != nil {
		t.Fatal(err)
	}
	page, err := s.Repository.PageArtifacts(t.Context(), session.ID, branch.NativeSessionID, "input-attachment", "", 0, 0, 8)
	if err != nil || len(page.Entries) != 1 {
		t.Fatal("late committed original was lost", page, err)
	}
}

func TestCanceledPickerRetainsUnadoptedCommittedCapturedFile(t *testing.T) {
	for _, beforeReceipt := range []bool{false, true} {
		t.Run(fmt.Sprintf("cancel-before-receipt=%t", beforeReceipt), func(t *testing.T) {
			s, _ := fixture(t)
			workspace := t.TempDir()
			source := filepath.Join(workspace, "picked.txt")
			if err := os.WriteFile(source, []byte("saved project original"), 0600); err != nil {
				t.Fatal(err)
			}
			session, err := s.Create(t.Context(), "canceled picker receipt", workspace)
			if err != nil {
				t.Fatal(err)
			}
			wrapped := workspaceDelayedComposerFileReceipt{ServiceStore: s.Store, files: s.Store.(hub.ComposerFileStore), entered: make(chan hub.ComposerFile, 1), release: make(chan struct{})}
			s.Store = wrapped
			p := NewNativePage(s)
			p.selected, p.detail = session.ID, mustPickerDetail(t, s, session.ID)
			p.model.Draft = "preserve this draft"
			tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
			p.openWorkspacePicker("references")
			p.picker.query = "picked.txt"
			p.queryWorkspacePicker(false)
			waitWorkspace(t, p, tester, func() bool { return !p.picker.loading && len(p.picker.rows) == 1 })
			p.picker.active = 0
			p.chooseWorkspacePicker()
			var release sync.Once
			defer release.Do(func() { close(wrapped.release) })
			var captured hub.ComposerFile
			select {
			case captured = <-wrapped.entered:
			case <-time.After(time.Second):
				t.Fatal("file selection did not commit a captured original")
			}
			if beforeReceipt {
				p.closeWorkspacePicker()
			}
			release.Do(func() { close(wrapped.release) })
			if !beforeReceipt {
				deadline := time.Now().Add(time.Second)
				for len(p.updates) == 0 && time.Now().Before(deadline) {
					time.Sleep(time.Millisecond)
				}
				if len(p.updates) == 0 {
					t.Fatal("successful capture did not queue its adoption receipt")
				}
				p.closeWorkspacePicker()
			}
			waitWorkspace(t, p, tester, func() bool {
				p.referenceOwner.mu.Lock()
				idle := p.referenceOwner.active == 0 && len(p.referenceOwner.claims) == 0
				p.referenceOwner.mu.Unlock()
				return idle
			})
			if len(p.composerFiles[session.ID]) != 0 || p.model.Draft != "preserve this draft" || s.Runtime.HasActive() {
				t.Fatal("canceled picker adopted an invisible captured file or altered its draft")
			}
			page, err := s.ReadComposerFile(t.Context(), product.SelectedComposerFile(captured), false, 0, 4096)
			if err != nil || string(page.Data) != "saved project original" {
				t.Fatal("canceling adoption deleted a committed original", page, err)
			}
			detail, err := s.Store.Detail(t.Context(), session.ID)
			branch, branchErr := product.ActiveBranch(detail)
			if err != nil || branchErr != nil || branch.NativeSessionID != "" {
				t.Fatal("capturing a file created native execution state", err, branchErr)
			}
		})
	}
}

func waitClosedReferenceOwner(t *testing.T, p *NativePage) {
	t.Helper()
	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		p.referenceOwner.mu.Lock()
		idle := p.referenceOwner.active == 0
		p.referenceOwner.mu.Unlock()
		if idle && len(p.Service.DraftAttachments(p.selected)) == 0 {
			return
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("closed observer's actual reference operation did not settle")
}

func TestNewWindowActionSharesServiceAndKeepsViewDraftsIndependent(t *testing.T) {
	s, requests := fixture(t)
	session, err := s.Create(context.Background(), "two observers", "")
	if err != nil {
		t.Fatal(err)
	}
	first, second := NewNativePage(s), NewNativePage(s)
	first.selected, second.selected = session.ID, session.ID
	first.model.Draft = "first window unsent draft"
	second.model.Draft = "second window unsent draft"
	opened := ""
	first.openWindow = func(id string) error { opened = id; return nil }
	tester := ui.NewTester(workspaceStaticView(first), 1400, 1400)
	if err = workspaceTestMenu(tester, "新窗口"); err != nil {
		t.Fatal(err)
	}
	if opened != session.ID || first.Service != second.Service || first.model.Draft == second.model.Draft || s.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("window action copied execution or view draft")
	}
}
