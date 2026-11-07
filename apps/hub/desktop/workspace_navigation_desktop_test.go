// INPUT: Actual SQLite workspace metadata, bounded navigation ports and headless native controls.
// OUTPUT: Cold project hierarchy, independent cursor paging and stale child receipt isolation regressions.
// POS: [L7 Hub UI testing] Exercises saved facts without OS windows, file-body reads, accounts or model execution.
//go:build desktop

package hubdesktop

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/egoist/mygo/ui"
)

type workspaceNavigationStoreProbe struct {
	product.ServiceStore
	port       hub.WorkspaceNavigationStore
	reads      atomic.Int32
	childReads atomic.Int32
	entered    chan struct{}
	release    chan struct{}
}

func (s *workspaceNavigationStoreProbe) PageWorkspaces(ctx context.Context, q hub.WorkspaceNavigationQuery, cursor hub.WorkspaceCursor, limit int) (hub.WorkspacePage, error) {
	s.reads.Add(1)
	return s.port.PageWorkspaces(ctx, q, cursor, limit)
}

func (s *workspaceNavigationStoreProbe) PageWorkspaceSessions(ctx context.Context, path, query string, cursor hub.SessionCursor, limit int) (hub.SessionPage, error) {
	page, err := s.port.PageWorkspaceSessions(ctx, path, query, cursor, limit)
	if s.childReads.Add(1) == 1 && s.entered != nil {
		close(s.entered)
		select {
		case <-s.release:
		case <-ctx.Done():
			return hub.SessionPage{}, ctx.Err()
		}
		if len(page.Sessions) > 0 {
			page.Sessions[0].Title = "stale child snapshot"
		}
	}
	return page, err
}

func settleWorkspaceNavigation(t *testing.T, p *NativePage, tester *ui.Tester) {
	t.Helper()
	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		tester.Frame()
		loading := p.navigation.loading || p.settingsScope.loading || p.polling || p.paneBusy || p.sessionLoading || p.busy
		for _, child := range p.navigation.projects {
			loading = loading || child.loading
		}
		if !loading {
			return
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("navigation reads did not settle")
}

func createNavigationProject(t *testing.T, service *product.HubService, path string, count int) []hub.Session {
	t.Helper()
	if err := os.MkdirAll(path, 0700); err != nil {
		t.Fatal(err)
	}
	var sessions []hub.Session
	for i := 0; i < count; i++ {
		s, err := service.Create(t.Context(), fmt.Sprintf("Saved task %02d", i), path)
		if err != nil {
			t.Fatal(err)
		}
		sessions = append(sessions, s)
	}
	return sessions
}

func TestWorkspaceNavigationProjectsAreIndependentColdPages(t *testing.T) {
	service, requests := fixture(t)
	root := t.TempDir()
	for i := 0; i < 10; i++ {
		count := 1
		if i == 0 {
			count = 17
		}
		createNavigationProject(t, service, filepath.Join(root, fmt.Sprintf("project-%02d", i)), count)
	}
	probe := &workspaceNavigationStoreProbe{ServiceStore: service.Store, port: service.Store.(hub.WorkspaceNavigationStore)}
	service.Store = probe
	p := NewNativePage(service)
	tester := ui.NewTester(workspaceStaticView(p), 1440, 960)
	if probe.reads.Load() != 0 || len(p.sessions) != 0 {
		t.Fatal("Build or constructor read the navigation store")
	}
	p.refreshWorkspaceProjects()
	settleWorkspaceNavigation(t, p, tester)
	if len(p.navigation.page.Items) != 8 || !p.navigation.page.HasMore || len(p.sessions) != 0 {
		t.Fatal("project index was derived from the recent page or lost its cursor", p.navigation.page)
	}
	path := p.navigation.page.Items[0].Path
	p.toggleWorkspaceProject(path)
	settleWorkspaceNavigation(t, p, tester)
	child := p.navigation.projects[path]
	if !child.open || len(child.page.Sessions) != 16 || !child.page.HasMore {
		t.Fatal("project tasks lost their independent bounded page", child)
	}
	for _, session := range child.page.Sessions {
		if session.Workspace != path {
			t.Fatal("project hierarchy borrowed another workspace", session)
		}
	}
	firstProjectPage, firstChildPage := p.navigation.page, child.page
	p.loadWorkspaceProjectSessions(path, child.page.Next)
	settleWorkspaceNavigation(t, p, tester)
	if len(child.page.Sessions) != 1 || child.page.HasMore || p.navigation.page.Next != firstProjectPage.Next || child.page.Sessions[0].ID == firstChildPage.Sessions[0].ID {
		t.Fatal("child paging replaced the project index or repeated its first page")
	}
	before := probe.reads.Load()
	for range 8 {
		tester.Frame()
	}
	if probe.reads.Load() != before || service.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("rendering caused reads or execution")
	}
}

func TestWorkspaceNavigationRejectsChildReceiptAfterProjectReload(t *testing.T) {
	service, _ := fixture(t)
	path := filepath.Join(t.TempDir(), "real-project")
	createNavigationProject(t, service, path, 1)
	probe := &workspaceNavigationStoreProbe{ServiceStore: service.Store, port: service.Store.(hub.WorkspaceNavigationStore), entered: make(chan struct{}), release: make(chan struct{})}
	t.Cleanup(func() {
		select {
		case <-probe.release:
		default:
			close(probe.release)
		}
	})
	service.Store = probe
	p := NewNativePage(service)
	tester := ui.NewTester(workspaceStaticView(p), 1440, 960)
	p.refreshWorkspaceProjects()
	settleWorkspaceNavigation(t, p, tester)
	path = p.navigation.page.Items[0].Path
	p.toggleWorkspaceProject(path)
	<-probe.entered
	p.refreshWorkspaceProjects()
	settleWorkspaceNavigation(t, p, tester)
	p.toggleWorkspaceProject(path)
	settleWorkspaceNavigation(t, p, tester)
	current := p.navigation.projects[path]
	close(probe.release)
	deadline := time.Now().Add(3 * time.Second)
	for len(p.updates) == 0 && time.Now().Before(deadline) {
		time.Sleep(time.Millisecond)
	}
	tester.Frame()
	if p.navigation.projects[path] != current || len(current.page.Sessions) != 1 || current.page.Sessions[0].Title == "stale child snapshot" {
		t.Fatal("a child receipt from the previous project page replaced the new scope", p.navigation.projects[path] == current, current, p.navigation.problem, probe.childReads.Load())
	}
}
