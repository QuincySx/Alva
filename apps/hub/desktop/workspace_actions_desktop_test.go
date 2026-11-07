// INPUT: Actual native Send controls, shared bootstrap/SQLite and failure-only engine fixtures.
// OUTPUT: Cold welcome, first-send durable admission and partial-failure draft/identity regressions.
// POS: [L7 Hub UI testing] Verifies the real product chain without external model accounts.
//go:build desktop

package hubdesktop

import (
	"context"
	"path/filepath"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/apps/hub/product"
	view "github.com/QuincySx/Alva/apps/ui/chat"
	"github.com/egoist/mygo/ui"
)

func waitWorkspace(t *testing.T, page *NativePage, tester *ui.Tester, complete func() bool) {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		tester.Frame()
		if complete() {
			return
		}
		page.poll()
		time.Sleep(time.Millisecond)
	}
	t.Fatalf("workspace did not settle: selected=%s busy=%v pending=%s error=%s", page.selected, page.busy, page.pendingFirstSend, page.problem)
}

func TestWorkspaceFirstSendUsesActualSharedDemoAndPersists(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "private")
	app, err := bootstrap.Open(t.Context(), bootstrap.Config{Demo: true, DataDirectory: dir, OpenStores: bootstrap.LocalStores})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := app.Close(context.Background()); err != nil {
			t.Error(err)
		}
	})
	page := NewNativePage(app.Service)
	tester := ui.NewTester(workspaceStaticView(page), 1240, 850)
	if err = tester.Click("Message"); err != nil {
		t.Fatal(err)
	}
	tester.Type("\n请整理这一段演示任务")
	rows, err := app.Store.ListSessions(t.Context(), 8)
	if err != nil || len(rows) != 0 || app.Runtime.HasActive() {
		t.Fatal("typing on home created work", rows, err)
	}
	if err = tester.Click("Send"); err != nil {
		t.Fatal(err)
	}
	waitWorkspace(t, page, tester, func() bool {
		if page.selected == "" {
			return false
		}
		requests, err := app.Service.PendingApprovals(t.Context(), page.selected)
		if err != nil {
			t.Fatal(err)
		}
		// Explicit test-only consent for this local deterministic echo fixture.
		for _, request := range requests {
			if err := app.Service.RespondApproval(t.Context(), page.selected, request.ID, true); err != nil {
				t.Fatal(err)
			}
		}
		return !page.busy && !page.polling && !app.Runtime.HasActive() && page.model.Status == view.Completed && len(page.model.Messages) >= 2
	})
	rows, err = app.Store.ListSessions(t.Context(), 8)
	if err != nil || len(rows) != 1 || page.model.Draft != "" || page.pendingFirstSend != "" {
		t.Fatalf("first send duplicated/lost lifecycle: rows=%+v draft=%q err=%v", rows, page.model.Draft, err)
	}
	history, err := app.Store.History(t.Context(), page.selected, 0, 128)
	if err != nil || len(history) == 0 {
		t.Fatal("actual execution was not durable", err)
	}
	page.startWorkspaceTask()
	tester.Frame()
	if page.selected != "" || !tester.HasText("今天，想完成什么？") {
		t.Fatal("new-task navigation did not return home")
	}
	rows, err = app.Store.ListSessions(t.Context(), 8)
	if err != nil || len(rows) != 1 {
		t.Fatal("new-task navigation created another session", err)
	}
}

func TestWorkspaceFirstAdmissionFailureKeepsCommittedIdentityAndDraft(t *testing.T) {
	service, _ := fixture(t)
	// A closed actual runtime rejects admission before creating a Turn.
	if err := service.Runtime.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	page := NewNativePage(service)
	tester := ui.NewTester(workspaceStaticView(page), 1240, 850)
	page.model.Draft = "retain this prompt"
	page.sendWorkspaceDraft()
	waitWorkspace(t, page, tester, func() bool { return page.selected != "" && !page.busy && !page.polling && page.problem != "" })
	id := page.selected
	if page.model.Draft != "retain this prompt" {
		t.Fatal("failed admission consumed draft")
	}
	page.sendWorkspaceDraft()
	waitWorkspace(t, page, tester, func() bool { return !page.busy && page.problem != "" })
	rows, err := service.Store.ListSessions(t.Context(), 8)
	if err != nil || len(rows) != 1 || page.selected != id || service.Runtime.HasActive() {
		t.Fatal("retry created another session or executed", rows, err)
	}
	page.startWorkspaceTask()
	page.model.Draft = "home draft survives navigation"
	page.selectSession(id)
	waitWorkspace(t, page, tester, func() bool { return !page.polling })
	page.startWorkspaceTask()
	if page.model.Draft != "home draft survives navigation" {
		t.Fatal("navigation discarded home draft")
	}
}

func TestWorkspaceCreateFailureKeepsVisibleHomeDraft(t *testing.T) {
	service, _ := fixture(t)
	page := NewNativePage(service)
	page.workspace = filepath.Join(t.TempDir(), "missing-directory")
	page.model.Draft = "keep my input"
	tester := ui.NewTester(workspaceStaticView(page), 1240, 850)
	page.sendWorkspaceDraft()
	waitWorkspace(t, page, tester, func() bool { return !page.busy && page.problem != "" })
	if page.selected != "" || page.model.Draft != "keep my input" || !tester.HasText(page.problem) {
		t.Fatal("create failure hid or consumed draft")
	}
	rows, err := service.Store.ListSessions(t.Context(), 8)
	if err != nil || len(rows) != 0 {
		t.Fatal("failed creation produced a session", rows, err)
	}
}

func TestWorkspaceRemovedStaticRouteCannotCreateHomeSession(t *testing.T) {
	service, _ := fixture(t)
	page := NewNativePage(service)
	stale := service.DefaultTarget
	stale.ModelID = "removed-model"
	page.composer.target, page.model.Draft = stale, "keep my draft"
	page.sendWorkspaceDraft()
	rows, err := service.Store.ListSessions(t.Context(), 8)
	if err != nil || len(rows) != 0 || page.selected != "" || page.model.Draft != "keep my draft" || page.problem == "" {
		t.Fatal("missing static route created work or lost draft", err)
	}
}

type workspaceDelayedDetail struct {
	product.ServiceStore
	block            atomic.Bool
	entered, release chan struct{}
}

func (s *workspaceDelayedDetail) Detail(ctx context.Context, id string) (hub.Detail, error) {
	value, err := s.ServiceStore.Detail(ctx, id)
	if s.block.CompareAndSwap(true, false) {
		close(s.entered)
		select {
		case <-s.release:
		case <-ctx.Done():
		}
	}
	return value, err
}

func TestWorkspaceSameSessionReloadBlocksSendUntilTailReturns(t *testing.T) {
	for _, navigation := range []string{"select", "latest"} {
		t.Run(navigation, func(t *testing.T) {
			service, _ := fixture(t)
			session, err := service.Create(t.Context(), "same session", "")
			if err != nil {
				t.Fatal(err)
			}
			base := service.Store
			wrapped := &workspaceDelayedDetail{ServiceStore: base, entered: make(chan struct{}), release: make(chan struct{})}
			service.Store = wrapped
			page := NewNativePage(service)
			page.selected = session.ID
			page.detail, err = base.Detail(t.Context(), session.ID)
			if err != nil {
				t.Fatal(err)
			}
			page.applyDetail()
			page.model.Draft = "retain during reload"
			initialStatus := page.detail.Session.Status
			tester := ui.NewTester(workspaceStaticView(page), 1240, 850)
			wrapped.block.Store(true)
			if navigation == "select" {
				page.selectSession(session.ID)
			} else if err := workspaceTestMenu(tester, "最新消息"); err != nil {
				t.Fatal(err)
			}
			var release sync.Once
			defer release.Do(func() { close(wrapped.release) })
			select {
			case <-wrapped.entered:
			case <-time.After(time.Second):
				t.Fatal("tail did not enter delayed read")
			}
			tester.Frame()
			if err = tester.Click("Send"); err != nil {
				t.Fatal(err)
			}
			page.send() // Semantic/debug entry must enforce the same in-flight-read gate.
			detail, err := base.Detail(t.Context(), session.ID)
			if err != nil || detail.Session.Status != initialStatus || service.Runtime.HasActive() || page.model.Draft != "retain during reload" {
				t.Fatal("stale read allowed admission", err)
			}
			release.Do(func() { close(wrapped.release) })
			waitWorkspace(t, page, tester, func() bool { return !page.polling && !page.chat.Busy })
		})
	}
}

func TestWorkspaceAdmissionRejectsEarlierIdleObservation(t *testing.T) {
	app, err := bootstrap.Open(t.Context(), bootstrap.Config{Demo: true, DataDirectory: filepath.Join(t.TempDir(), "private"), OpenStores: bootstrap.LocalStores})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := app.Close(context.Background()); err != nil {
			t.Error(err)
		}
	})
	session, err := app.Service.Create(t.Context(), "observe before admission", "")
	if err != nil {
		t.Fatal(err)
	}
	wrapped := &workspaceDelayedDetail{ServiceStore: app.Service.Store, entered: make(chan struct{}), release: make(chan struct{})}
	app.Service.Store = wrapped
	page := NewNativePage(app.Service)
	tester := ui.NewTester(workspaceStaticView(page), 1240, 850)
	page.selectSession(session.ID)
	waitWorkspace(t, page, tester, func() bool { return !page.polling && page.detail.Session.ID == session.ID })
	page.model.Draft = "local fixture awaiting explicit approval"
	wrapped.block.Store(true)
	page.poll()
	var release sync.Once
	defer release.Do(func() { close(wrapped.release) })
	select {
	case <-wrapped.entered:
	case <-time.After(time.Second):
		t.Fatal("observer did not capture idle detail")
	}
	page.send()
	deadline := time.Now().Add(5 * time.Second)
	for page.busy && time.Now().Before(deadline) {
		tester.Frame()
		time.Sleep(time.Millisecond)
	}
	if page.busy || page.model.Status != view.Running || !app.Runtime.HasActive() {
		t.Fatalf("actual admission did not start: status=%s problem=%s", page.model.Status, page.problem)
	}
	release.Do(func() { close(wrapped.release) })
	deadline = time.Now().Add(time.Second)
	for time.Now().Before(deadline) {
		tester.Frame()
		if page.model.Status != view.Running || page.model.ActiveRunID == "" {
			t.Fatal("an earlier idle observation replaced the admitted running state")
		}
		if !page.polling {
			return
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("earlier observation never settled")
}

func TestWorkspaceRoutingMenuCanDisablePersistedFallback(t *testing.T) {
	service, _ := fixture(t)
	session, err := service.Create(t.Context(), "route owner", "")
	if err != nil {
		t.Fatal(err)
	}
	backup := service.DefaultTarget
	backup.ModelID = "backup"
	if err = service.Store.SetFallback(t.Context(), session.ID, []hub.Target{backup}); err != nil {
		t.Fatal(err)
	}
	page := NewNativePage(service)
	page.selected = session.ID
	page.detail, err = service.Store.Detail(t.Context(), session.ID)
	if err != nil {
		t.Fatal(err)
	}
	page.applyDetail()
	tester := ui.NewTester(workspaceStaticView(page), 1240, 850)
	if err = workspaceTestOpenMenu(tester); err != nil {
		t.Fatal(err)
	}
	if err = tester.ChooseMenuItem("路由与故障切换"); err != nil {
		t.Fatal(err)
	}
	if err = tester.Click("Disable automatic fallback"); err != nil {
		t.Fatal(err)
	}
	waitWorkspace(t, page, tester, func() bool { return !page.busy && !page.polling && len(page.detail.Fallback) == 0 })
	detail, err := service.Store.Detail(t.Context(), session.ID)
	if err != nil || len(detail.Fallback) != 0 || service.Runtime.HasActive() {
		t.Fatal("fallback removal did not persist cold", err)
	}
}
