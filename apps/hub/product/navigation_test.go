// INPUT: Real product/navigation stores and optional capability wrappers.
// OUTPUT: Shared facade location policy, unsupported ports and cold automation browse regressions.
// POS: [L7 product] Indexes reuse durable facts and never activate the scheduler or model.
package product

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

func TestNavigationUsesSavedProjectsAndLeavesExecutionCold(t *testing.T) {
	s, requests := fixture(t)
	ctx := t.Context()
	project := t.TempDir()
	for _, path := range []string{project, filepath.Join(s.DataDirectory, "workspaces", ".hidden"), filepath.Join(s.DataDirectory, "workspaces-extra")} {
		if err := os.MkdirAll(path, 0700); err != nil {
			t.Fatal(err)
		}
		session, err := s.Create(ctx, "saved workspace", path)
		if err != nil {
			t.Fatal(err)
		}
		if path == project {
			project = session.Workspace
		}
	}
	page, err := s.BrowseWorkspaces(ctx, "", hub.WorkspaceCursor{}, 8)
	if err != nil || len(page.Items) != 2 {
		t.Fatal(page, err)
	}
	rows, err := s.BrowseWorkspaceSessions(ctx, project, "", hub.SessionCursor{}, 8)
	if err != nil || len(rows.Sessions) != 1 || rows.Sessions[0].Workspace != project {
		t.Fatal(rows, err)
	}
	if s.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("project browsing invoked an engine")
	}
	items, err := s.BrowseAutomations(ctx)
	if err != nil || len(items) != 0 || s.Runtime.HasActive() {
		t.Fatal("empty automation overview executed or failed", items, err)
	}
}

type workspacePageReplyStore struct {
	ServiceStore
	page hub.SessionPage
}

func (s workspacePageReplyStore) PageWorkspaces(context.Context, hub.WorkspaceNavigationQuery, hub.WorkspaceCursor, int) (hub.WorkspacePage, error) {
	return hub.WorkspacePage{}, nil
}
func (s workspacePageReplyStore) PageWorkspaceSessions(context.Context, string, string, hub.SessionCursor, int) (hub.SessionPage, error) {
	return s.page, nil
}

func TestNavigationRejectsChangedOptionalProviderSessionPage(t *testing.T) {
	s, _ := fixture(t)
	store := s.Store
	workspace := t.TempDir()
	row := hub.Session{ID: "b", Workspace: workspace, UpdatedAt: 20}
	for name, page := range map[string]hub.SessionPage{
		"duplicate":  {Sessions: []hub.Session{row, row}, Next: hub.SessionCursor{Updated: 20, ID: "b"}},
		"wrong-next": {Sessions: []hub.Session{row}, Next: hub.SessionCursor{Updated: 20, ID: "other"}},
		"reordered":  {Sessions: []hub.Session{{ID: "a", Workspace: workspace, UpdatedAt: 10}, row}, Next: hub.SessionCursor{Updated: 20, ID: "b"}},
		"foreign":    {Sessions: []hub.Session{{ID: "a", Workspace: t.TempDir(), UpdatedAt: 20}}, Next: hub.SessionCursor{Updated: 20, ID: "a"}},
	} {
		t.Run(name, func(t *testing.T) {
			s.Store = workspacePageReplyStore{ServiceStore: store, page: page}
			if _, err := s.BrowseWorkspaceSessions(t.Context(), workspace, "", hub.SessionCursor{}, 4); err == nil {
				t.Fatal("invalid provider page reached project navigation")
			}
		})
	}
}

type navigationAbsentStore struct{ ServiceStore }

func TestNavigationCapabilityAbsenceNeverFallsBackToRecentSessions(t *testing.T) {
	s, _ := fixture(t)
	s.Store = navigationAbsentStore{s.Store}
	if _, err := s.BrowseWorkspaces(t.Context(), "", hub.WorkspaceCursor{}, 8); !errors.Is(err, engine.ErrUnsupported) {
		t.Fatal(err)
	}
	if _, err := s.BrowseWorkspaceSessions(t.Context(), t.TempDir(), "", hub.SessionCursor{}, 8); !errors.Is(err, engine.ErrUnsupported) {
		t.Fatal(err)
	}
	if _, err := s.BrowseAutomations(t.Context()); !errors.Is(err, engine.ErrUnsupported) {
		t.Fatal(err)
	}
}
