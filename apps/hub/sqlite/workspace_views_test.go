// INPUT: Temporary real SQLite, owned session branches and resource view CAS.
// OUTPUT: Reopen/position isolation, competing window and failed-commit regressions.
// POS: [L7 SQLite tests] Proves view persistence without execution or native fact changes.
package sqlite

import (
	"context"
	"errors"
	"path/filepath"
	"reflect"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
)

func TestWorkspaceViewsReopenAndSessionIsolation(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "product")
	s, err := Open(t.Context(), dir)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = s.Close() }()
	a, err := s.CreateSession(t.Context(), "A", t.TempDir(), target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	b, err := s.CreateSession(t.Context(), "B", t.TempDir(), target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	view, err := s.WorkspaceView(t.Context(), a.ID)
	if err != nil || view.Revision != 0 || len(view.Tabs) != 0 {
		t.Fatal(view, err)
	}
	view.Tabs = []hub.WorkspaceTab{{ID: "files-tab", Kind: "files", Title: "Saved position", BranchID: a.ActiveBranchID, Query: "notes", After: "scoped-metadata-cursor", ScrollY: 24}, {ID: "artifact-tab", Kind: "artifact", Title: "Original", BranchID: a.ActiveBranchID, URI: "alva-artifact://owned-source", Offset: 4096, ScrollY: 120}}
	view.Active, view.Focused, view.Collapsed = "artifact-tab", true, true
	view.Width = 640
	saved, err := s.SaveWorkspaceView(t.Context(), view, 0)
	if err != nil || saved.Revision != 1 {
		t.Fatal(saved, err)
	}
	if _, err = s.SaveWorkspaceView(t.Context(), view, 0); !errors.Is(err, hub.ErrWorkspaceViewConflict) {
		t.Fatal("stale window overwrote saved view", err)
	}
	other, err := s.WorkspaceView(t.Context(), b.ID)
	if err != nil || other.Revision != 0 || len(other.Tabs) != 0 {
		t.Fatal("scope leaked", other, err)
	}
	foreign := hub.CloneWorkspaceView(saved)
	foreign.SessionID = b.ID
	if _, err = s.SaveWorkspaceView(t.Context(), foreign, 0); err == nil {
		t.Fatal("foreign branch location accepted")
	}
	if err = s.Close(); err != nil {
		t.Fatal(err)
	}
	s, err = Open(t.Context(), dir)
	if err != nil {
		t.Fatal(err)
	}
	got, err := s.WorkspaceView(t.Context(), a.ID)
	if err != nil || !reflect.DeepEqual(got, saved) {
		t.Fatal("reopen lost exact locations", got, saved, err)
	}
	if _, err = s.WorkspaceView(t.Context(), "missing-session"); !errors.Is(err, hub.ErrNotFound) {
		t.Fatal("missing session semantics", err)
	}
}

func TestWorkspaceViewsCanceledFailedCommitAndCorruptionRemainExplicit(t *testing.T) {
	s := newStore(t)
	a, err := s.CreateSession(t.Context(), "A", t.TempDir(), target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	view := hub.WorkspaceView{SessionID: a.ID, Active: "one", Tabs: []hub.WorkspaceTab{{ID: "one", Kind: "files", Title: "Files", BranchID: a.ActiveBranchID}}}
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	if _, err = s.SaveWorkspaceView(ctx, view, 0); !errors.Is(err, context.Canceled) {
		t.Fatal("cancelled save committed", err)
	}
	if _, err = s.db.ExecContext(t.Context(), `CREATE TRIGGER reject_workspace_view BEFORE INSERT ON hub_workspace_views BEGIN SELECT RAISE(ABORT,'view fixture failure'); END`); err != nil {
		t.Fatal(err)
	}
	if got, err := s.SaveWorkspaceView(t.Context(), view, 0); err == nil || got.Revision != 0 {
		t.Fatal("failed save returned success", got, err)
	}
	got, err := s.WorkspaceView(t.Context(), a.ID)
	if err != nil || got.Revision != 0 {
		t.Fatal("failed save left partial fact", got, err)
	}
	if _, err = s.db.ExecContext(t.Context(), `DROP TRIGGER reject_workspace_view`); err != nil {
		t.Fatal(err)
	}
	if _, err = s.SaveWorkspaceView(t.Context(), view, 0); err != nil {
		t.Fatal(err)
	}
	if _, err = s.db.ExecContext(t.Context(), `UPDATE hub_workspace_views SET revision=revision+1 WHERE session=?`, a.ID); err != nil {
		t.Fatal(err)
	}
	if _, err = s.WorkspaceView(t.Context(), a.ID); err == nil {
		t.Fatal("corrupt revision silently reset")
	}
}
