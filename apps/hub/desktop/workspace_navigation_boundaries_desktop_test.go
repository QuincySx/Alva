// INPUT: Real saved-session indexes with deterministic read failures/barriers and native controls.
// OUTPUT: Pagination commit/retry, stale candidate, hidden-owner settlement and background refresh regressions.
// POS: [L7 Hub UI testing] Read-only fixtures preserve session/draft/execution identities without accounts or OS windows.
//go:build desktop

package hubdesktop

import (
	"context"
	"errors"
	"fmt"
	"path/filepath"
	"reflect"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/egoist/mygo/ui"
)

type workspaceNavigationReadGate struct {
	block, fail      atomic.Bool
	entered, release chan struct{}
}

func newWorkspaceNavigationReadGate(t *testing.T) *workspaceNavigationReadGate {
	t.Helper()
	g := &workspaceNavigationReadGate{entered: make(chan struct{}, 1), release: make(chan struct{}, 1)}
	t.Cleanup(g.unblock)
	return g
}

func (g *workspaceNavigationReadGate) read(ctx context.Context) error {
	if g == nil {
		return nil
	}
	if g.fail.Swap(false) {
		return errors.New("fixture navigation read failed")
	}
	if g.block.Swap(false) {
		g.entered <- struct{}{}
		select {
		case <-g.release:
		case <-ctx.Done():
			return ctx.Err()
		}
	}
	return nil
}

func (g *workspaceNavigationReadGate) unblock() {
	select {
	case g.release <- struct{}{}:
	default:
	}
}

type workspaceNavigationBoundaryStore struct {
	product.ServiceStore
	port                              hub.WorkspaceNavigationStore
	projects, children, scope, detail *workspaceNavigationReadGate
}

func (s *workspaceNavigationBoundaryStore) PageWorkspaces(ctx context.Context, query hub.WorkspaceNavigationQuery, cursor hub.WorkspaceCursor, limit int) (hub.WorkspacePage, error) {
	if err := s.projects.read(ctx); err != nil {
		return hub.WorkspacePage{}, err
	}
	return s.port.PageWorkspaces(ctx, query, cursor, limit)
}

func (s *workspaceNavigationBoundaryStore) PageWorkspaceSessions(ctx context.Context, path, query string, cursor hub.SessionCursor, limit int) (hub.SessionPage, error) {
	if err := s.children.read(ctx); err != nil {
		return hub.SessionPage{}, err
	}
	return s.port.PageWorkspaceSessions(ctx, path, query, cursor, limit)
}

func (s *workspaceNavigationBoundaryStore) PageSessions(ctx context.Context, query string, cursor hub.SessionCursor, limit int) (hub.SessionPage, error) {
	if err := s.scope.read(ctx); err != nil {
		return hub.SessionPage{}, err
	}
	return s.ServiceStore.PageSessions(ctx, query, cursor, limit)
}

func (s *workspaceNavigationBoundaryStore) Detail(ctx context.Context, id string) (hub.Detail, error) {
	if err := s.detail.read(ctx); err != nil {
		return hub.Detail{}, err
	}
	return s.ServiceStore.Detail(ctx, id)
}

func workspaceBoundaryPage(t *testing.T, service *product.HubService) (*NativePage, *ui.Tester, *workspaceNavigationBoundaryStore) {
	t.Helper()
	store := &workspaceNavigationBoundaryStore{ServiceStore: service.Store, port: service.Store.(hub.WorkspaceNavigationStore),
		projects: newWorkspaceNavigationReadGate(t), children: newWorkspaceNavigationReadGate(t), scope: newWorkspaceNavigationReadGate(t), detail: newWorkspaceNavigationReadGate(t)}
	service.Store = store
	p := NewNativePage(service)
	p.ctx = t.Context()
	return p, ui.NewTester(workspaceStaticView(p), 1440, 960), store
}

func awaitWorkspaceBoundary(t *testing.T, tester *ui.Tester, ready func() bool) {
	t.Helper()
	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		tester.Frame()
		if ready() {
			return
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("navigation boundary did not settle")
}

func awaitWorkspaceRead(t *testing.T, g *workspaceNavigationReadGate) {
	t.Helper()
	select {
	case <-g.entered:
	case <-time.After(3 * time.Second):
		t.Fatal("navigation read did not reach its barrier")
	}
}

func TestWorkspaceScopeCandidateCannotReenterDuringReplacementPage(t *testing.T) {
	service := preferenceFixture(t)
	a, err := service.Create(t.Context(), "Owner A", "")
	if err != nil {
		t.Fatal(err)
	}
	b, err := service.Create(t.Context(), "Owner B", "")
	if err != nil {
		t.Fatal(err)
	}
	p, tester, store := workspaceBoundaryPage(t, service)
	p.openWorkspaceSessionChooserForSession("resources", a.ID)
	settleWorkspaceNavigation(t, p, tester)
	p.settingsScope.query = "Owner B"
	store.scope.block.Store(true)
	p.refreshWorkspaceScopeSessions()
	awaitWorkspaceRead(t, store.scope)
	tester.Frame()
	connectionClick(t, tester, "Select scope session "+a.ID)
	p.selectWorkspaceScopeSession(a.ID) // A late old-row gesture must share the same guard.
	if p.selected != "" || p.settingsScope.selected != "" || p.workspaceScopeReady() {
		t.Fatal("old candidate was accepted while the new result page was loading")
	}
	store.scope.unblock()
	settleWorkspaceNavigation(t, p, tester)
	p.selectWorkspaceScopeSession(a.ID)
	if p.selected != "" || p.settingsScope.candidate.ID != "" || len(p.settingsScope.page.Sessions) != 1 || p.settingsScope.page.Sessions[0].ID != b.ID {
		t.Fatal("stale candidate bypassed membership validation after replacement")
	}
	connectionClick(t, tester, "Select scope session "+b.ID)
	settleWorkspaceNavigation(t, p, tester)
	if !p.workspaceScopeReady() || p.selected != b.ID || service.Runtime.HasActive() {
		t.Fatal("actual replacement candidate did not bind a ready, nonexecuting scope")
	}
}

type workspacePaginationSnapshot struct {
	cursor, back, page any
	query              string
}

func TestWorkspaceNavigationPagingFailureRetriesOriginalTarget(t *testing.T) {
	for _, owner := range []string{"projects", "children", "scope"} {
		t.Run(owner, func(t *testing.T) {
			service := preferenceFixture(t)
			root := t.TempDir()
			for i := range 10 {
				count := 1
				if i == 0 {
					count = 17
				}
				createNavigationProject(t, service, filepath.Join(root, fmt.Sprintf("project-%02d", i)), count)
			}
			p, tester, store := workspaceBoundaryPage(t, service)
			var gate *workspaceNavigationReadGate
			var next, previous, retry, refresh func()
			var snapshot func() workspacePaginationSnapshot
			var requested, nextCursor func() any
			var historyLength func() int
			var problem func() string
			if owner == "scope" {
				p.openWorkspaceSessionChooser("plugins")
				settleWorkspaceNavigation(t, p, tester)
				v := &p.settingsScope
				gate, next, previous, retry, refresh = store.scope, p.nextWorkspaceScopeSessions, p.previousWorkspaceScopeSessions, p.retryWorkspaceScopeRead, p.refreshWorkspaceScopeSessions
				snapshot = func() workspacePaginationSnapshot {
					return workspacePaginationSnapshot{v.cursor, v.back, v.page, v.loadedQuery}
				}
				requested, nextCursor = func() any { return v.request.cursor }, func() any { return v.page.Next }
				historyLength, problem = func() int { return len(v.back) }, func() string { return v.problem }
			} else {
				p.refreshWorkspaceProjects()
				settleWorkspaceNavigation(t, p, tester)
				if owner == "projects" {
					v := &p.navigation
					gate, next, previous, retry, refresh = store.projects, p.nextWorkspaceProjects, p.previousWorkspaceProjects, p.retryWorkspaceProjects, p.refreshWorkspaceProjects
					snapshot = func() workspacePaginationSnapshot {
						return workspacePaginationSnapshot{v.cursor, v.back, v.page, v.query}
					}
					requested, nextCursor = func() any { return v.request.cursor }, func() any { return v.page.Next }
					historyLength, problem = func() int { return len(v.back) }, func() string { return v.problem }
				} else {
					path := p.navigation.page.Items[0].Path
					p.toggleWorkspaceProject(path)
					settleWorkspaceNavigation(t, p, tester)
					v := p.navigation.projects[path]
					gate, next, previous, retry = store.children, func() { p.nextWorkspaceProjectSessions(path) }, func() { p.previousWorkspaceProjectSessions(path) }, func() { p.retryWorkspaceProjectSessions(path) }
					refresh = func() {
						p.loadWorkspaceProjectSessionsTarget(path, workspacePageTarget(hub.SessionCursor{}, nil, p.navigation.query))
					}
					snapshot = func() workspacePaginationSnapshot {
						return workspacePaginationSnapshot{v.cursor, v.back, v.page, p.navigation.query}
					}
					requested, nextCursor = func() any { return v.request.cursor }, func() any { return v.page.Next }
					historyLength, problem = func() int { return len(v.back) }, func() string { return v.problem }
				}
			}
			initial, target := snapshot(), nextCursor()
			gate.fail.Store(true)
			next()
			settleWorkspaceNavigation(t, p, tester)
			if problem() == "" || !reflect.DeepEqual(snapshot(), initial) || !reflect.DeepEqual(requested(), target) {
				t.Fatal("failed Next changed committed page/history or lost the retry cursor", snapshot(), requested())
			}
			retry()
			settleWorkspaceNavigation(t, p, tester)
			if !reflect.DeepEqual(snapshot().cursor, target) || historyLength() != 1 || problem() != "" {
				t.Fatal("Next retry did not commit the requested cursor/history")
			}
			second := snapshot()
			gate.fail.Store(true)
			previous()
			settleWorkspaceNavigation(t, p, tester)
			if problem() == "" || !reflect.DeepEqual(snapshot(), second) || !reflect.DeepEqual(requested(), initial.cursor) {
				t.Fatal("failed Previous popped history or lost its destination")
			}
			retry()
			settleWorkspaceNavigation(t, p, tester)
			if !reflect.DeepEqual(snapshot().cursor, initial.cursor) || historyLength() != 0 {
				t.Fatal("Previous retry did not return to the exact previous cursor")
			}
			next()
			settleWorkspaceNavigation(t, p, tester)
			second = snapshot()
			gate.fail.Store(true)
			refresh()
			settleWorkspaceNavigation(t, p, tester)
			if problem() == "" || !reflect.DeepEqual(snapshot(), second) {
				t.Fatal("failed same-query refresh reset committed history before success")
			}
			retry()
			settleWorkspaceNavigation(t, p, tester)
			if !reflect.DeepEqual(snapshot().cursor, initial.cursor) || historyLength() != 0 || service.Runtime.HasActive() {
				t.Fatal("refresh retry did not atomically reset history without execution")
			}
		})
	}
}

func TestWorkspaceNavigationChildSettlesAfterFailedParentRefresh(t *testing.T) {
	service := preferenceFixture(t)
	createNavigationProject(t, service, filepath.Join(t.TempDir(), "real-project"), 17)
	p, tester, store := workspaceBoundaryPage(t, service)
	p.refreshWorkspaceProjects()
	settleWorkspaceNavigation(t, p, tester)
	path := p.navigation.page.Items[0].Path
	p.toggleWorkspaceProject(path)
	settleWorkspaceNavigation(t, p, tester)
	child := p.navigation.projects[path]
	committed := workspacePaginationSnapshot{child.cursor, child.back, child.page, p.navigation.query}
	target := child.page.Next
	store.children.block.Store(true)
	p.nextWorkspaceProjectSessions(path)
	awaitWorkspaceRead(t, store.children)
	store.projects.fail.Store(true)
	p.refreshWorkspaceProjects()
	awaitWorkspaceBoundary(t, tester, func() bool { return !p.navigation.loading })
	if p.navigation.problem == "" || p.navigation.projects[path] != child || !child.loading {
		t.Fatal("failed parent refresh did not retain the pending child owner")
	}
	store.children.unblock()
	settleWorkspaceNavigation(t, p, tester)
	after := workspacePaginationSnapshot{child.cursor, child.back, child.page, p.navigation.query}
	if !reflect.DeepEqual(after, committed) || child.problem == "" || child.request.cursor != target {
		t.Fatal("late child either accepted an invalidated page or lost its exact retry target")
	}
	p.retryWorkspaceProjectSessions(path)
	settleWorkspaceNavigation(t, p, tester)
	if child.problem != "" || child.cursor != target || len(child.page.Sessions) != 1 || len(child.back) != 1 || service.Runtime.HasActive() {
		t.Fatal("settled child owner could not retry its requested page without executing")
	}
}

func TestWorkspaceScopeReadSettlesWhileSettingsTemporarilyOwnsView(t *testing.T) {
	for _, kind := range []string{"page", "detail"} {
		t.Run(kind, func(t *testing.T) {
			service := preferenceFixture(t)
			owner, err := service.Create(t.Context(), "Existing hidden owner", "")
			if err != nil {
				t.Fatal(err)
			}
			p, tester, store := workspaceBoundaryPage(t, service)
			gate := store.scope
			if kind == "detail" {
				gate = store.detail
			}
			gate.block.Store(true)
			if kind == "page" {
				p.openWorkspaceSessionChooser("resources")
			} else {
				p.openWorkspaceSessionChooserForSession("resources", owner.ID)
			}
			awaitWorkspaceRead(t, gate)
			tester.Frame()
			openPreferencePage(t, p, tester)
			gate.unblock()
			awaitWorkspaceBoundary(t, tester, func() bool { return !p.settingsScope.loading })
			if p.pane != "preferences" || p.selected != "" || p.settingsScope.cancel != nil || p.problem != "" {
				t.Fatal("hidden scope result either retained its owner or changed navigation/global state")
			}
			connectionClick(t, tester, "Back to workspace")
			if p.pane != "workspace-scope" || p.settingsScope.loading {
				t.Fatal("return restored a permanently loading chooser")
			}
			if kind == "detail" && p.settingsScope.candidate.ID != owner.ID || kind == "page" && len(p.settingsScope.page.Sessions) != 1 {
				t.Fatal("matching hidden read was not retained as the correct scope projection")
			}
			p.refreshWorkspaceScopeSessions()
			settleWorkspaceNavigation(t, p, tester)
			if len(p.settingsScope.page.Sessions) != 1 || service.Runtime.HasActive() {
				t.Fatal("returned chooser cannot issue a new cold read")
			}
		})
	}
}

func TestWorkspaceSettingsAndChooserKeepVerifiedScopeDuringBackgroundPoll(t *testing.T) {
	service := preferenceFixture(t)
	owner, err := service.Create(t.Context(), "Stable refresh owner", "")
	if err != nil {
		t.Fatal(err)
	}
	p, tester, store := workspaceBoundaryPage(t, service)
	p.selectSession(owner.ID)
	settleWorkspaceNavigation(t, p, tester)
	p.openWorkspaceSessionChooserForSession("resources", owner.ID)
	settleWorkspaceNavigation(t, p, tester)
	store.detail.block.Store(true)
	p.selectWorkspaceScopeSession(owner.ID)
	awaitWorkspaceRead(t, store.detail)
	tester.Frame()
	if p.workspaceScopeReady() || !p.chat.Busy || p.detail.Session.ID != owner.ID {
		t.Fatal("a matching cached ID was accepted before the new selection's first Detail receipt")
	}
	p.chat.Busy = false // An unrelated operation's cleanup is not a Detail receipt.
	if p.workspaceScopeReady() {
		t.Fatal("shared Busy cleanup bypassed the missing generation-bound Detail receipt")
	}
	store.detail.unblock()
	settleWorkspaceNavigation(t, p, tester)
	if !p.workspaceScopeReady() {
		t.Fatal("matching Detail success did not validate the new selected generation")
	}
	p.openWorkspaceSettingsCategory("security")
	tester.Frame()
	connectionClick(t, tester, "Settings permission policy")
	store.detail.block.Store(true)
	p.poll()
	awaitWorkspaceRead(t, store.detail)
	tester.Frame()
	if !p.polling || !p.workspaceScopeReady() || !p.composer.permissionOpen || !tester.HasText("自动审批") {
		t.Fatal("background polling revoked a verified scope or its open permission menu")
	}
	if _, ok := tester.Find("Settings permission policy"); !ok {
		t.Fatal("background refresh removed the Settings operation")
	}
	store.detail.unblock()
	settleWorkspaceNavigation(t, p, tester)
	if p.selected != owner.ID || service.Runtime.HasActive() {
		t.Fatal("background refresh changed the owner or execution")
	}
}
