// INPUT: Real private SQLite product stores with ordinary sessions, Turns and a held write connection
// OUTPUT: Rename, pin and soft-delete invariants, restart persistence, continue-origin storage and the read-connection regression
// POS: [L7 Hub product] Proves a deleted session stays on disk but leaves every list; a blocked write connection never blocks list reads
package sqlite

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"path/filepath"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

func rowCount(t *testing.T, s *Store, query string, args ...any) int {
	t.Helper()
	var n int
	if err := s.db.QueryRowContext(context.Background(), query, args...).Scan(&n); err != nil {
		t.Fatal(err)
	}
	return n
}

func TestSoftDeleteHidesEverywhereButKeepsData(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	workspace := t.TempDir()
	gone, err := s.CreateSession(ctx, "gone", workspace, target("a"))
	if err != nil {
		t.Fatal(err)
	}
	kept, err := s.CreateSession(ctx, "kept", workspace, target("a"))
	if err != nil {
		t.Fatal(err)
	}
	claim, err := s.BeginTurn(ctx, gone.ID, "hello")
	if err != nil {
		t.Fatal(err)
	}
	if err = s.DeleteSession(ctx, gone.ID); !errors.Is(err, hub.ErrSessionRunning) {
		t.Fatal("a running session must refuse deletion", err)
	}
	complete(t, s, claim, "native-gone")
	if err = s.PinSession(ctx, gone.ID, true); err != nil {
		t.Fatal(err)
	}
	if err = s.DeleteSession(ctx, gone.ID); err != nil {
		t.Fatal(err)
	}
	page, err := s.PageSessions(ctx, "", hub.SessionCursor{}, 10)
	if err != nil || len(page.Sessions) != 1 || page.Sessions[0].ID != kept.ID {
		t.Fatal("deleted session still in the page", page, err)
	}
	if page, err = s.PageSessions(ctx, "gone", hub.SessionCursor{}, 10); err != nil || len(page.Sessions) != 0 {
		t.Fatal("deleted session still found by search", page, err)
	}
	if page, err = s.PageWorkspaceSessions(ctx, workspace, "", hub.SessionCursor{}, 10); err != nil || len(page.Sessions) != 1 {
		t.Fatal("deleted session still in workspace navigation", page, err)
	}
	if projects, err := s.PageWorkspaces(ctx, hub.WorkspaceNavigationQuery{}, hub.WorkspaceCursor{}, 10); err != nil || len(projects.Items) != 1 || projects.Items[0].SessionCount != 1 {
		t.Fatal("deleted session still counted by its project", projects, err)
	}
	if pinned, err := s.PinnedSessions(ctx); err != nil || len(pinned) != 0 {
		t.Fatal("deleting must also unpin", pinned, err)
	}
	if list, err := s.ListSessions(ctx, 10); err != nil || len(list) != 1 {
		t.Fatal("ListSessions shows a deleted session", list, err)
	}
	// Data retained: the product rows and the native binding are untouched.
	if rowCount(t, s, `SELECT count(*) FROM hub_sessions WHERE id=?`, gone.ID) != 1 ||
		rowCount(t, s, `SELECT count(*) FROM hub_turns WHERE session=?`, gone.ID) != 1 ||
		rowCount(t, s, `SELECT count(*) FROM hub_events WHERE session=?`, gone.ID) == 0 {
		t.Fatal("soft delete must keep product data")
	}
	if _, err = s.BeginTurn(ctx, gone.ID, "again"); !errors.Is(err, hub.ErrNotFound) {
		t.Fatal("a deleted session must not accept a Turn", err)
	}
	if err = s.RenameSession(ctx, gone.ID, "x"); !errors.Is(err, hub.ErrNotFound) {
		t.Fatal("a deleted session must not be renamed", err)
	}
	if err = s.DeleteSession(ctx, gone.ID); !errors.Is(err, hub.ErrNotFound) {
		t.Fatal("deleting twice must report not found", err)
	}
}

func TestRenamePinKeepOrderAndSurviveRestart(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "private")
	s, err := Open(context.Background(), dir)
	if err != nil {
		t.Fatal(err)
	}
	ctx := context.Background()
	workspace := t.TempDir()
	first, _ := s.CreateSession(ctx, "first", workspace, target("a"))
	time.Sleep(2 * time.Millisecond)
	second, _ := s.CreateSession(ctx, "second", workspace, target("a"))
	if err = s.RenameSession(ctx, first.ID, "renamed"); err != nil {
		t.Fatal(err)
	}
	if err = s.PinSession(ctx, first.ID, true); err != nil {
		t.Fatal(err)
	}
	for _, bad := range []string{"", " padded ", "two\nlines", "lone\rcr", "nul\x00", "bidi\u202eevil", string(make([]byte, 300))} {
		if err = s.RenameSession(ctx, first.ID, bad); err == nil {
			t.Fatalf("title %q accepted", bad)
		}
	}
	if err = s.Close(); err != nil {
		t.Fatal(err)
	}
	if s, err = Open(ctx, dir); err != nil {
		t.Fatal(err)
	}
	defer s.Close()
	page, err := s.PageSessions(ctx, "", hub.SessionCursor{}, 10)
	if err != nil || len(page.Sessions) != 2 || page.Sessions[0].ID != second.ID || page.Sessions[1].ID != first.ID {
		t.Fatal("rename or pin changed the recent order", page, err)
	}
	if page.Sessions[1].Title != "renamed" || !page.Sessions[1].Pinned || page.Sessions[1].UpdatedAt != first.UpdatedAt {
		t.Fatal("rename or pin lost after restart or bumped UpdatedAt", page.Sessions[1])
	}
	pinned, err := s.PinnedSessions(ctx)
	if err != nil || len(pinned) != 1 || pinned[0].ID != first.ID {
		t.Fatal("pinned group wrong", pinned, err)
	}
}

func TestPinnedGroupIsBounded(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	workspace := t.TempDir()
	var last hub.Session
	for i := 0; i <= hub.MaxPinnedSessions; i++ {
		var err error
		if last, err = s.CreateSession(ctx, fmt.Sprintf("s%d", i), workspace, target("a")); err != nil {
			t.Fatal(err)
		}
		if i < hub.MaxPinnedSessions {
			if err = s.PinSession(ctx, last.ID, true); err != nil {
				t.Fatal(err)
			}
		}
	}
	if err := s.PinSession(ctx, last.ID, true); !errors.Is(err, hub.ErrPinnedBudget) {
		t.Fatal("pinned budget not enforced", err)
	}
}

// The product database has one write connection. Before the dedicated read
// connection a list read queued behind any long operation on it and the
// sidebar/search stayed on "loading"; this holds the write connection open and
// requires the bounded list reads to answer anyway.
func TestListReadsDoNotQueueBehindTheWriteConnection(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	workspace := t.TempDir()
	session, err := s.CreateSession(ctx, "visible", workspace, target("a"))
	if err != nil {
		t.Fatal(err)
	}
	held, err := s.db.BeginTx(ctx, nil) // a long write/read owning the only connection
	if err != nil {
		t.Fatal(err)
	}
	defer held.Rollback()
	short, cancel := context.WithTimeout(ctx, 3*time.Second)
	defer cancel()
	page, err := s.PageSessions(short, "", hub.SessionCursor{}, 10)
	if err != nil || len(page.Sessions) != 1 || page.Sessions[0].ID != session.ID {
		t.Fatal("session page blocked behind the write connection", err)
	}
	if _, err = s.PageSessions(short, "visible", hub.SessionCursor{}, 10); err != nil {
		t.Fatal("search blocked behind the write connection", err)
	}
	if _, err = s.PageWorkspaces(short, hub.WorkspaceNavigationQuery{}, hub.WorkspaceCursor{}, 10); err != nil {
		t.Fatal("project navigation blocked behind the write connection", err)
	}
	if _, err = s.PinnedSessions(short); err != nil {
		t.Fatal("pinned read blocked behind the write connection", err)
	}
	if _, err = s.LatestTurn(short, session.ID); !errors.Is(err, hub.ErrNotFound) {
		t.Fatal("latest turn read blocked or wrong", err)
	}
}

func TestListConnectionIsReadOnly(t *testing.T) {
	s := newStore(t)
	if _, err := s.list.ExecContext(context.Background(), `DELETE FROM hub_sessions`); err == nil {
		t.Fatal("the list connection must refuse writes")
	}
}

func TestContinueAdmissionRequiresAStoppedLatestTurnInTheSameTransaction(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	session, _ := s.CreateSession(ctx, "c", t.TempDir(), target("a"))
	cont := hub.ComposerTurnInput{Origin: hub.TurnOriginContinue, Prompt: "go on"}
	if _, err := s.BeginComposerTurn(ctx, session.ID, "", cont); !errors.Is(err, hub.ErrNothingToContinue) {
		t.Fatal("continue with no turn admitted", err)
	}
	done, err := s.BeginTurn(ctx, session.ID, "first")
	if err != nil {
		t.Fatal(err)
	}
	complete(t, s, done, "native-c")
	if _, err = s.BeginComposerTurn(ctx, session.ID, "", cont); !errors.Is(err, hub.ErrNothingToContinue) {
		t.Fatal("continue after a completed reply admitted", err)
	}
	stopped, err := s.BeginTurn(ctx, session.ID, "second")
	if err != nil {
		t.Fatal(err)
	}
	if err = s.FinishTurn(ctx, stopped.Turn.ID, engine.Cancelled); err != nil {
		t.Fatal(err)
	}
	claim, err := s.BeginComposerTurn(ctx, session.ID, "", cont)
	if err != nil {
		t.Fatal(err)
	}
	if claim.Turn.Origin != hub.TurnOriginContinue {
		t.Fatal("turn lost its origin")
	}
	history, err := s.History(ctx, session.ID, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	var found bool
	for _, entry := range history {
		if entry.TurnID == claim.Turn.ID && entry.Origin == hub.TurnOriginContinue && entry.Input == "" {
			found = true
		}
	}
	if !found {
		t.Fatal("continue turn must record origin and no user text", history)
	}
	if err = s.FinishTurn(ctx, claim.Turn.ID, engine.Cancelled); err != nil {
		t.Fatal(err)
	}
	latest, err := s.LatestTurn(ctx, session.ID)
	if err != nil || latest.ID != claim.Turn.ID || latest.Status != engine.Cancelled {
		t.Fatal("latest turn wrong", latest, err)
	}
	if _, err = s.BeginComposerTurn(ctx, session.ID, "", hub.ComposerTurnInput{Origin: "bogus", Prompt: "x"}); err == nil {
		t.Fatal("unknown origin accepted")
	}
}

func TestDeletedSessionIsNotFoundForEveryProductAccess(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	gone, err := s.CreateSession(ctx, "gone", t.TempDir(), target("a"))
	if err != nil {
		t.Fatal(err)
	}
	if err = s.DeleteSession(ctx, gone.ID); err != nil {
		t.Fatal(err)
	}
	checks := map[string]error{
		"Detail":          func() error { _, e := s.Detail(ctx, gone.ID); return e }(),
		"Settings":        func() error { _, e := s.Settings(ctx, gone.ID); return e }(),
		"SetPlanMode":     s.SetPlanMode(ctx, gone.ID, true),
		"SetFallback":     s.SetFallback(ctx, gone.ID, []hub.Target{target("b")}),
		"History":         func() error { _, e := s.History(ctx, gone.ID, 0, 10); return e }(),
		"HistoryBefore":   func() error { _, e := s.HistoryBefore(ctx, gone.ID, 0, 10); return e }(),
		"Switch":          func() error { _, e := s.Switch(ctx, gone.ID, target("b"), "x"); return e }(),
		"BeginTurn":       func() error { _, e := s.BeginTurn(ctx, gone.ID, "x"); return e }(),
		"SetMediaEnabled": s.SetMediaEnabled(ctx, gone.ID, true),
	}
	for name, err := range checks {
		if !errors.Is(err, hub.ErrNotFound) {
			t.Errorf("%s on a deleted session = %v, want NotFound", name, err)
		}
	}
	raw, err := s.DetailIncludingDeleted(ctx, gone.ID)
	if err != nil || raw.Session.DeletedAt == 0 {
		t.Fatal("the admin read must still see the deleted session", err)
	}
}

func TestDeletePausesTheSessionsAutomationsWithAReason(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	p, a := schedule(t, s)
	a = enableSchedule(t, s, a)
	if !a.Enabled {
		t.Fatal("fixture automation not enabled")
	}
	if err := s.DeleteSession(ctx, p.ID); err != nil {
		t.Fatal(err)
	}
	got, err := s.Automation(ctx, p.ID, a.ID)
	if err != nil || got.Enabled || got.NextRunAt != 0 || got.LastError != hub.AutomationDeletedReason || got.Revision != a.Revision+1 {
		t.Fatal("automation of a deleted session keeps running or lacks the reason", got, err)
	}
	if due, err := s.DueAutomations(ctx, now()+1<<40); err != nil || len(due) != 0 {
		t.Fatal("deleted session's automation is still due", due, err)
	}
	index, err := s.ListAutomationIndex(ctx)
	if err != nil || len(index) != 1 || index[0].LastError != hub.AutomationDeletedReason {
		t.Fatal("the automation list must show why it was paused", index, err)
	}
}

func TestDeleteRefusesWhileATeamWorkerRuns(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	lead, _, worker, _ := teamFixture(t, s)
	if err := s.transaction(ctx, func(tx *sql.Tx) error {
		child, err := read[hub.Session](ctx, tx, liveSessionQuery, worker.SessionID)
		if err != nil {
			return err
		}
		child.Status = hub.Running
		return writeSession(ctx, tx, child)
	}); err != nil {
		t.Fatal(err)
	}
	if err := s.DeleteSession(ctx, lead.ID); !errors.Is(err, hub.ErrSessionHasWorkers) {
		t.Fatal("a Lead with a running Worker was deletable", err)
	}
	if _, err := s.Detail(ctx, lead.ID); err != nil {
		t.Fatal("a refused delete changed the session", err)
	}
}
