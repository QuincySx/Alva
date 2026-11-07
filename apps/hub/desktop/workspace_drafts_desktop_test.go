// INPUT: Actual private draft stores, shared NativePages and MyGo headless edit/navigation controls.
// OUTPUT: Debounced cross-window drafts, dirty conflicts, late-ack revisions and restart-without-Send regressions.
// POS: [L7 Hub UI tests] Exercises manual persistence without real windows, external accounts or model execution.
//go:build desktop

package hubdesktop

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/apps/hub/product"
	productstore "github.com/QuincySx/Alva/apps/hub/sqlite"
	"github.com/egoist/mygo/ui"
)

func durableWorkspaceService(t *testing.T) *product.HubService {
	t.Helper()
	service, _ := fixture(t)
	store, ok := service.Store.(hub.ComposerDraftStore)
	if !ok {
		t.Fatal("fixture has no actual draft storage")
	}
	owner, err := product.NewComposerDraftsService(store)
	if err != nil {
		t.Fatal(err)
	}
	service.ComposerDrafts = owner
	t.Cleanup(func() { _ = owner.Close(context.Background()) })
	return service
}

func awaitWorkspaceDraft(t *testing.T, p *NativePage, tester *ui.Tester, done func() bool) {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		tester.Frame()
		p.observeWorkspaceDraft()
		if done() {
			return
		}
		time.Sleep(time.Millisecond)
	}
	e := p.workspaceDraftEntry(p.workspaceComposerDraftScope())
	t.Fatalf("draft did not settle: selected=%q local=%q known=%v dirty=%v saving=%v conflict=%v problem=%s", p.selected, p.model.Draft, e.known, e.dirty, e.saving, e.conflict, e.problem)
}

// A headless UI must keep delivering asynchronous save receipts before the
// actual admission/import boundary, just as the native window event loop does.
func awaitWorkspaceAdmissionBoundary(t *testing.T, tester *ui.Tester, entered <-chan struct{}) {
	t.Helper()
	deadline := time.NewTimer(3 * time.Second)
	defer deadline.Stop()
	for {
		tester.Frame()
		select {
		case <-entered:
			return
		case <-deadline.C:
			t.Fatal("actual admission/import boundary was not reached")
		default:
			time.Sleep(time.Millisecond)
		}
	}
}

func TestWorkspaceDurableDraftDebouncesAndSynchronizesSameLogicalScope(t *testing.T) {
	service := durableWorkspaceService(t)
	p := NewNativePage(service)
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	awaitWorkspaceDraft(t, p, tester, func() bool { return p.workspaceDraftEntry(p.workspaceComposerDraftScope()).known })
	if err := tester.Click("Message"); err != nil {
		t.Fatal(err)
	}
	tester.Type("shared manual input")
	awaitWorkspaceDraft(t, p, tester, func() bool {
		e := p.workspaceDraftEntry(p.workspaceComposerDraftScope())
		return e.committed.Text == "shared manual input" && !e.dirty && !e.saving
	})
	other := NewNativePage(service)
	otherTester := ui.NewTester(workspaceStaticView(other), 1240, 850)
	awaitWorkspaceDraft(t, other, otherTester, func() bool { return other.model.Draft == "shared manual input" })
	// Keep a conflicting local edit intact when the shared draft advances.
	other.model.Draft = "this window keeps its own unsaved edit"
	other.markWorkspaceDraftEdited()
	p.model.Draft = "new shared revision"
	p.markWorkspaceDraftEdited()
	p.flushWorkspaceDraft()
	awaitWorkspaceDraft(t, p, tester, func() bool {
		return !p.workspaceDraftEntry(p.workspaceComposerDraftScope()).saving && p.workspaceDraftEntry(p.workspaceComposerDraftScope()).committed.Text == "new shared revision"
	})
	other.observeWorkspaceDraft()
	if e := other.workspaceDraftEntry(other.workspaceComposerDraftScope()); !e.conflict || other.model.Draft != "this window keeps its own unsaved edit" {
		t.Fatal("remote committed revision silently replaced dirty input")
	}
	otherTester.Frame()
	if err := otherTester.Click("Reload shared draft"); err != nil {
		t.Fatal(err)
	}
	awaitWorkspaceDraft(t, other, otherTester, func() bool {
		return other.model.Draft == "new shared revision" && !other.workspaceDraftEntry(other.workspaceComposerDraftScope()).conflict
	})
	sessions, err := service.Store.ListSessions(t.Context(), 8)
	if err != nil || len(sessions) != 0 || service.Runtime.HasActive() {
		t.Fatal("restoring or sharing input created work", err)
	}
}

func TestWorkspaceDurableDraftOldAckKeepsEditedBackText(t *testing.T) {
	service := durableWorkspaceService(t)
	p := NewNativePage(service)
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	awaitWorkspaceDraft(t, p, tester, func() bool { return p.workspaceDraftEntry(p.workspaceComposerDraftScope()).known })
	p.model.Draft = "same text"
	p.markWorkspaceDraftEdited()
	var captured workspaceDraftSubmission
	p.prepareWorkspaceDraftSubmission(func(token workspaceDraftSubmission) { captured = token })
	awaitWorkspaceDraft(t, p, tester, func() bool { return captured.valid })
	p.model.Draft = "temporary"
	p.markWorkspaceDraftEdited()
	p.model.Draft = "same text"
	p.markWorkspaceDraftEdited()
	// Simulate only the acknowledgement of an actually admitted old capture.
	if _, err := service.ComposerDrafts.Consume(t.Context(), captured.scope, captured.revision); err != nil {
		t.Fatal(err)
	}
	if p.finishWorkspaceDraftSubmission(captured) || p.model.Draft != "same text" {
		t.Fatal("old acknowledgement cleared a newer edit with equal text")
	}
	p.flushWorkspaceDraft()
	awaitWorkspaceDraft(t, p, tester, func() bool {
		e := p.workspaceDraftEntry(p.workspaceComposerDraftScope())
		return !e.saving && !e.dirty && e.committed.Text == "same text" && e.committed.Revision > captured.revision
	})
}

func TestWorkspaceDurableDraftNavigationFlushAndRestartDoNotSend(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "private")
	app, err := bootstrap.Open(t.Context(), bootstrap.Config{Demo: true, DataDirectory: directory, OpenStores: bootstrap.LocalStores})
	if err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(app.Service)
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	awaitWorkspaceDraft(t, p, tester, func() bool { return p.workspaceDraftEntry(p.workspaceComposerDraftScope()).known })
	p.model.Draft = "home survives a restart"
	p.markWorkspaceDraftEdited()
	p.flushWorkspaceDraft()
	awaitWorkspaceDraft(t, p, tester, func() bool { e := p.workspaceDraftEntry(p.workspaceComposerDraftScope()); return !e.dirty && !e.saving })
	if err = app.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	app, err = bootstrap.Open(t.Context(), bootstrap.Config{Demo: true, DataDirectory: directory, OpenStores: bootstrap.LocalStores})
	if err != nil {
		t.Fatal(err)
	}
	defer app.Close(context.Background())
	restored := NewNativePage(app.Service)
	restoredTester := ui.NewTester(workspaceStaticView(restored), 1240, 850)
	awaitWorkspaceDraft(t, restored, restoredTester, func() bool { return restored.model.Draft == "home survives a restart" })
	sessions, err := app.Store.ListSessions(t.Context(), 8)
	if err != nil || len(sessions) != 0 || app.Runtime.HasActive() || restored.selected != "" {
		t.Fatal("draft restart executed or created a session", len(sessions), err)
	}
}

func TestWorkspaceDurableFirstCreateFailureRestoresCommittedIdentityWithoutSend(t *testing.T) {
	service := durableWorkspaceService(t)
	if err := service.Runtime.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(service)
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	awaitWorkspaceDraft(t, p, tester, func() bool { return p.workspaceDraftEntry(p.workspaceComposerDraftScope()).known })
	p.model.Draft = "retain first identity"
	p.markWorkspaceDraftEdited()
	p.sendWorkspaceDraft()
	awaitWorkspaceDraft(t, p, tester, func() bool { return p.selected != "" && !p.busy && p.problem != "" })
	id := p.selected
	other := NewNativePage(service)
	otherTester := ui.NewTester(workspaceStaticView(other), 1240, 850)
	awaitWorkspaceDraft(t, other, otherTester, func() bool { return other.workspaceDraftEntry(other.workspaceComposerDraftScope()).known })
	if other.model.Draft != "" || other.selected != "" {
		t.Fatal("a successfully relocated session draft was duplicated into a new home composer")
	}
	other.selectSession(id)
	awaitWorkspaceDraft(t, other, otherTester, func() bool { return other.selected == id && other.model.Draft == "retain first identity" })
	sessions, err := service.Store.ListSessions(t.Context(), 8)
	if err != nil || len(sessions) != 1 || service.Runtime.HasActive() || other.pendingFirstSend != "" {
		t.Fatal("pending identity restore recreated or sent the manual draft", len(sessions), err)
	}
}

func TestWorkspaceHomeSlashOffersSelectedProjectSkillBeforeFirstSend(t *testing.T) {
	service, requests := fixture(t)
	workspace, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	directory := filepath.Join(workspace, product.ManagedSkillDirectory, "exact-home")
	if err = os.MkdirAll(directory, 0700); err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(directory, "SKILL.md"), []byte("---\nname: exact-home\ndescription: Selected project Skill before first Send\ninvocation: explicit\n---\nRead only the selected source.\n"), 0600); err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(service)
	p.workspace = workspace
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	if err = tester.Click("Message"); err != nil {
		t.Fatal(err)
	}
	tester.Type("/")
	tester.Type("exact-home")
	waitWorkspace(t, p, tester, func() bool { return p.picker.open && !p.picker.loading && len(p.picker.rows) == 1 })
	tester.Key(0, ui.KeyEnter)
	waitWorkspace(t, p, tester, func() bool { return !p.picker.open && len(p.composerSelections[""]) == 1 })
	selected := p.composerSelections[""][0]
	if selected.SkillRef == nil || selected.SkillRef.Workspace != workspace || selected.SkillRef.SessionID != "" || selected.SkillRef.BranchID != "" || p.model.Draft != selected.Text {
		t.Fatal("pre-session slash lost its frozen explicit workspace source", selected, p.model.Draft)
	}
	sessions, err := service.Store.ListSessions(t.Context(), 8)
	if err != nil || len(sessions) != 0 || len(requests) != 0 || service.Runtime.HasActive() {
		t.Fatal("selecting a home Skill created or executed work", err)
	}
}

type workspaceDraftReadBarrier struct {
	hub.ComposerDraftStore
	block            atomic.Bool
	entered, release chan struct{}
}

func (s *workspaceDraftReadBarrier) ComposerDraft(ctx context.Context, scope hub.ComposerDraftScope) (hub.ComposerDraft, error) {
	value, err := s.ComposerDraftStore.ComposerDraft(ctx, scope)
	if s.block.CompareAndSwap(true, false) {
		close(s.entered)
		select {
		case <-s.release:
		case <-ctx.Done():
			return hub.ComposerDraft{}, ctx.Err()
		}
	}
	return value, err
}

func TestWorkspaceDurableLateReadCannotOverwriteAnEdit(t *testing.T) {
	service, _ := fixture(t)
	scope := hub.ComposerDraftScope{Kind: "home", ID: "new-conversation"}
	store := service.Store.(hub.ComposerDraftStore)
	if _, err := store.SaveComposerDraft(t.Context(), hub.ComposerDraft{Scope: scope, Text: "stored earlier"}, 0); err != nil {
		t.Fatal(err)
	}
	barrier := &workspaceDraftReadBarrier{ComposerDraftStore: store, entered: make(chan struct{}), release: make(chan struct{})}
	barrier.block.Store(true)
	owner, err := product.NewComposerDraftsService(barrier)
	if err != nil {
		t.Fatal(err)
	}
	defer owner.Close(context.Background())
	defer close(barrier.release)
	service.ComposerDrafts = owner
	p := NewNativePage(service)
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	p.loadWorkspaceDraft()
	select {
	case <-barrier.entered:
	case <-time.After(time.Second):
		t.Fatal("draft read did not reach selected storage")
	}
	if err = tester.Click("Message"); err != nil {
		t.Fatal(err)
	}
	tester.Type("new edit while restoring")
	barrier.release <- struct{}{}
	awaitWorkspaceDraft(t, p, tester, func() bool { return !p.workspaceDraftEntry(scope).loading })
	if p.model.Draft != "new edit while restoring" || !p.workspaceDraftEntry(scope).conflict {
		t.Fatal("late restore overwrote a newer UI edit", p.model.Draft)
	}
}

type workspaceHomeMoveFault struct {
	hub.ComposerDraftStore
	admission hub.ComposerDraftAdmissionStore
}

func (s workspaceHomeMoveFault) SaveComposerDraft(ctx context.Context, value hub.ComposerDraft, expected uint64) (hub.ComposerDraft, error) {
	if value.Scope.Kind == "home" && value.Text == "" && value.PendingSessionID == "" {
		return hub.ComposerDraft{}, errors.New("home relocation fixture failure")
	}
	return s.ComposerDraftStore.SaveComposerDraft(ctx, value, expected)
}
func (s workspaceHomeMoveFault) CreateComposerSession(ctx context.Context, captured hub.ComposerDraft, title, workspace string, target hub.Target) (hub.Session, hub.ComposerDraft, error) {
	return s.admission.CreateComposerSession(ctx, captured, title, workspace, target)
}

func TestWorkspaceDurableRestartBetweenSessionCopyAndHomeClearRestoresIdentity(t *testing.T) {
	service, requests := fixture(t)
	if err := service.Runtime.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	store := service.Store.(hub.ComposerDraftStore)
	fault := workspaceHomeMoveFault{ComposerDraftStore: store, admission: service.Store.(hub.ComposerDraftAdmissionStore)}
	owner, err := product.NewComposerDraftsService(fault)
	if err != nil {
		t.Fatal(err)
	}
	service.ComposerDrafts = owner
	p := NewNativePage(service)
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	p.model.Draft = "survive the scope relocation gap"
	p.sendWorkspaceDraft()
	waitWorkspace(t, p, tester, func() bool { return p.selected != "" && !p.busy && !p.polling && p.problem != "" })
	id := p.selected
	home, err := store.ComposerDraft(t.Context(), hub.ComposerDraftScope{Kind: "home", ID: "new-conversation"})
	if err != nil || home.PendingSessionID != id || home.Text != "survive the scope relocation gap" {
		t.Fatal("failed relocation lost the committed home identity", home, err)
	}
	sessionDraft, err := store.ComposerDraft(t.Context(), hub.ComposerDraftScope{Kind: "session", ID: id})
	if err != nil || sessionDraft.Text != home.Text || sessionDraft.Revision == 0 {
		t.Fatal("home clear was attempted before the complete session copy committed", sessionDraft, err)
	}
	if err = owner.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	if err = service.Store.(interface{ Close() error }).Close(); err != nil {
		t.Fatal(err)
	}
	reopened, err := productstore.Open(t.Context(), filepath.Join(service.DataDirectory, "product"))
	if err != nil {
		t.Fatal(err)
	}
	defer reopened.Close()
	restoredOwner, err := product.NewComposerDraftsService(reopened)
	if err != nil {
		t.Fatal(err)
	}
	defer restoredOwner.Close(context.Background())
	service.Store, service.ComposerDrafts = reopened, restoredOwner
	restored := NewNativePage(service)
	restoredTester := ui.NewTester(workspaceStaticView(restored), 1240, 850)
	awaitWorkspaceDraft(t, restored, restoredTester, func() bool { return restored.selected == id && restored.model.Draft == sessionDraft.Text })
	sessions, err := reopened.ListSessions(t.Context(), 8)
	if err != nil || len(sessions) != 1 || len(requests) != 0 || restored.pendingFirstSend != "" || service.Runtime.HasActive() {
		t.Fatal("restart created or executed a replacement session", len(sessions), err)
	}
}
