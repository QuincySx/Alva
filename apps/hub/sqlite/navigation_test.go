// INPUT: Real saved workspace/session metadata, literal paths and ordinary paused schedules.
// OUTPUT: Complete bounded project/session pages and non-executing global automation index regressions.
// POS: [L7 SQLite product] Navigation reads facts beyond the recent page without scanning directories or claiming jobs.
package sqlite

import (
	"fmt"
	"path/filepath"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
)

func TestWorkspaceNavigationPagesAllSavedProjectsAndExactSessions(t *testing.T) {
	s := newStore(t)
	ctx := t.Context()
	root := t.TempDir()
	for i := range 23 {
		path := filepath.Join(root, fmt.Sprintf("project-%02d", i))
		for j := range 3 {
			if _, err := s.CreateSession(ctx, fmt.Sprintf("task-%d-%d", i, j), path, target("a")); err != nil {
				t.Fatal(err)
			}
		}
	}
	managed := filepath.Join(root, "workspaces")
	for _, path := range []string{managed, filepath.Join(managed, "private"), filepath.Join(managed, ".hidden")} {
		if _, err := s.CreateSession(ctx, "private", path, target("a")); err != nil {
			t.Fatal(err)
		}
	}
	for _, path := range []string{filepath.Join(root, "project-00-extra"), filepath.Join(root, "workspaces-extra")} {
		if _, err := s.CreateSession(ctx, "sibling", path, target("a")); err != nil {
			t.Fatal(err)
		}
	}
	cursor := hub.WorkspaceCursor{}
	seen := map[string]bool{}
	for range 20 {
		page, err := s.PageWorkspaces(ctx, hub.WorkspaceNavigationQuery{ExcludeRoot: managed}, cursor, 4)
		if err != nil || len(page.Items) > 4 {
			t.Fatal(page, err)
		}
		for _, entry := range page.Items {
			if seen[entry.Path] || entry.Path <= cursor.Path || entry.SessionCount <= 0 || entry.Path == managed || filepath.Dir(entry.Path) == managed {
				t.Fatal("incorrect project page", entry)
			}
			seen[entry.Path] = true
		}
		if !page.HasMore {
			break
		}
		cursor = page.Next
	}
	if len(seen) != 25 {
		t.Fatal("project index was inferred from a recent page or excluded a sibling", len(seen))
	}
	workspace := filepath.Join(root, "project-00")
	page, err := s.PageWorkspaceSessions(ctx, workspace, "", hub.SessionCursor{}, 2)
	if err != nil || len(page.Sessions) != 2 || !page.HasMore {
		t.Fatal(page, err)
	}
	next, err := s.PageWorkspaceSessions(ctx, workspace, "", page.Next, 2)
	if err != nil || len(next.Sessions) != 1 || next.HasMore {
		t.Fatal(next, err)
	}
	for _, session := range append(page.Sessions, next.Sessions...) {
		if session.Workspace != workspace {
			t.Fatal("exact project navigation included a prefix sibling", session)
		}
	}
}

func TestWorkspaceNavigationLiteralPathsAndClosedStore(t *testing.T) {
	s := newStore(t)
	path := filepath.Join(t.TempDir(), "项目_%")
	if _, err := s.CreateSession(t.Context(), "literal_100%", path, target("a")); err != nil {
		t.Fatal(err)
	}
	page, err := s.PageWorkspaces(t.Context(), hub.WorkspaceNavigationQuery{Query: "_%"}, hub.WorkspaceCursor{}, 8)
	if err != nil || len(page.Items) != 1 || page.Items[0].Path != path {
		t.Fatal("path query became a wildcard", page, err)
	}
	rows, err := s.PageWorkspaceSessions(t.Context(), path, "_100%", hub.SessionCursor{}, 8)
	if err != nil || len(rows.Sessions) != 1 {
		t.Fatal("session query became a wildcard", rows, err)
	}
	if err := s.Close(); err != nil {
		t.Fatal(err)
	}
	if _, err := s.PageWorkspaces(t.Context(), hub.WorkspaceNavigationQuery{}, hub.WorkspaceCursor{}, 8); err == nil {
		t.Fatal("closed navigation silently returned an empty catalog")
	}
}

func TestAutomationIndexIncludesPausedPlansAcrossOwnersWithoutClaim(t *testing.T) {
	s := newStore(t)
	ctx := t.Context()
	want := map[string]string{}
	for i := range 3 {
		session, err := s.CreateSession(ctx, fmt.Sprintf("owner-%d", i), t.TempDir(), target("a"))
		if err != nil {
			t.Fatal(err)
		}
		item, err := s.CreateAutomation(ctx, session.ID, hub.AutomationDraft{Title: fmt.Sprintf("plan-%d", i), Prompt: "must not execute", IntervalMinutes: 5})
		if err != nil {
			t.Fatal(err)
		}
		want[item.ID] = session.ID
	}
	items, err := s.ListAutomationIndex(ctx)
	if err != nil || len(items) != 3 {
		t.Fatal(items, err)
	}
	for _, item := range items {
		if want[item.ID] != item.SessionID || item.Enabled || item.RunningTurnID != "" || item.LastRunAt != 0 {
			t.Fatal("browsing changed schedule execution facts", item)
		}
		if _, err := s.ListAutomations(ctx, ""); err == nil {
			t.Fatal("global browsing changed empty-owner semantics")
		}
	}
}
