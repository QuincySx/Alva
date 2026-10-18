// INPUT: Real private SQLite, product engine fixtures and controlled due-time observations
// OUTPUT: Paused CRUD, exact atomic claims, no replay/fallback, Stop and crash settlement regressions
// POS: [L7 Hub product] Automation acceptance uses normal durable Turns rather than a second scheduling transcript
package sqlite

import (
	"context"
	"database/sql"
	"errors"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"

	"github.com/QuincySx/Alva/go/apps/hub"
	"github.com/QuincySx/Alva/go/integration/engine"
)

func schedule(t *testing.T, s *Store) (hub.Session, hub.Automation) {
	t.Helper()
	ctx := context.Background()
	p, err := s.CreateSession(ctx, "scheduled", t.TempDir(), target("a"))
	if err != nil {
		t.Fatal(err)
	}
	a, err := s.CreateAutomation(ctx, p.ID, hub.AutomationDraft{Title: " Review ", Prompt: "inspect explicitly", IntervalMinutes: 5})
	if err != nil {
		t.Fatal(err)
	}
	return p, a
}

func enableSchedule(t *testing.T, s *Store, a hub.Automation) hub.Automation {
	t.Helper()
	a, err := s.SetAutomationEnabled(context.Background(), a.SessionID, a.ID, a.Revision, true, true)
	if err != nil {
		t.Fatal(err)
	}
	return a
}

func tick(a hub.Automation) hub.AutomationTick {
	return hub.AutomationTick{ID: a.ID, SessionID: a.SessionID, Revision: a.Revision}
}

func TestAutomationPausedCRUDRevisionScopeAndBounds(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	p, a := schedule(t, s)
	if a.Enabled || a.NextRunAt != 0 || a.Title != "Review" || a.BranchID != p.ActiveBranchID {
		t.Fatal("not paused/pinned", a)
	}
	if due, err := s.DueAutomations(ctx, now()); err != nil || len(due) != 0 {
		t.Fatal("paused schedule is due", err)
	}
	if _, err := s.Automation(ctx, "foreign", a.ID); !errors.Is(err, sql.ErrNoRows) {
		t.Fatal("cross-owner lookup", err)
	}
	if _, err := s.SetAutomationEnabled(ctx, p.ID, a.ID, a.Revision, true, false); err == nil {
		t.Fatal("billing silently approved")
	}
	if _, err := s.UpdateAutomation(ctx, p.ID, a.ID, a.Revision+1, a.AutomationDraft); !errors.Is(err, hub.ErrAutomationStale) {
		t.Fatal("stale edit accepted", err)
	}
	for _, draft := range []hub.AutomationDraft{
		{Title: " ", Prompt: "x", IntervalMinutes: 5}, {Title: strings.Repeat("界", 121), Prompt: "x", IntervalMinutes: 5},
		{Title: "x", Prompt: strings.Repeat("x", 16385), IntervalMinutes: 5}, {Title: "x", Prompt: "x", IntervalMinutes: 4}, {Title: "x", Prompt: "x", IntervalMinutes: 10081},
	} {
		if _, err := s.CreateAutomation(ctx, p.ID, draft); err == nil {
			t.Fatal("invalid schedule accepted")
		}
	}
	a = enableSchedule(t, s, a)
	if err := s.DeleteAutomation(ctx, p.ID, a.ID, a.Revision-1); !errors.Is(err, hub.ErrAutomationStale) {
		t.Fatal(err)
	}
	a, err := s.UpdateAutomation(ctx, p.ID, a.ID, a.Revision, hub.AutomationDraft{Title: strings.Repeat("界", 120), Prompt: "new", IntervalMinutes: 10})
	if err != nil || a.Enabled || a.NextRunAt != 0 {
		t.Fatal("edit did not pause", err)
	}
	if err = s.DeleteAutomation(ctx, p.ID, a.ID, a.Revision); err != nil {
		t.Fatal(err)
	}
	if _, err = s.Detail(ctx, p.ID); err != nil {
		t.Fatal("config delete destroyed session", err)
	}
}

func TestAutomationClaimAtomicExactAndPauseNotUndone(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	p, a := schedule(t, s)
	if _, err := s.ClaimAutomation(ctx, tick(a), now()); !errors.Is(err, hub.ErrAutomationStale) {
		t.Fatal("paused claimed", err)
	}
	a = enableSchedule(t, s, a)
	claim, err := s.ClaimAutomation(ctx, tick(a), a.NextRunAt)
	if err != nil || claim.Turn.AutomationID != a.ID || claim.Turn.Prompt != a.Prompt {
		t.Fatal("not normal exact Turn", err)
	}
	if _, err = s.ClaimAutomation(ctx, tick(a), a.NextRunAt); !errors.Is(err, hub.ErrAutomationStale) {
		t.Fatal("claim replay", err)
	}
	running, err := s.Automation(ctx, p.ID, a.ID)
	if err != nil || running.RunningTurnID != claim.Turn.ID || running.NextRunAt != 0 {
		t.Fatal("not atomically linked", err)
	}
	if err = s.DeleteAutomation(ctx, p.ID, a.ID, running.Revision); err == nil {
		t.Fatal("running config deleted")
	}
	paused, err := s.SetAutomationEnabled(ctx, p.ID, a.ID, running.Revision, false, false)
	if err != nil || paused.RunningTurnID != claim.Turn.ID {
		t.Fatal("pause cancelled current Turn", err)
	}
	complete(t, s, claim, "scheduled-native")
	settled, err := s.Automation(ctx, p.ID, a.ID)
	if err != nil || settled.Enabled || settled.RunningTurnID != "" || settled.NextRunAt != 0 || settled.LastStatus != "completed" {
		t.Fatal("completion undid pause", settled, err)
	}
	history, err := s.History(ctx, p.ID, 0, 32)
	if err != nil || len(history) != 3 || history[0].Input != a.Prompt || history[0].TurnID != claim.Turn.ID {
		t.Fatal("not normal history", history, err)
	}
}

func TestAutomationBusyMissedRouteAndTeamChangesDoNotQueue(t *testing.T) {
	for _, mode := range []string{"busy", "missed", "route", "team"} {
		t.Run(mode, func(t *testing.T) {
			s := newStore(t)
			ctx := context.Background()
			p, a := schedule(t, s)
			a = enableSchedule(t, s, a)
			at := a.NextRunAt
			switch mode {
			case "busy":
				if _, err := s.BeginTurn(ctx, p.ID, "manual"); err != nil {
					t.Fatal(err)
				}
			case "missed":
				at += a.IntervalMinutes * 60_000
			case "route":
				if _, err := s.Switch(ctx, p.ID, target("b"), "explicit"); err != nil {
					t.Fatal(err)
				}
			case "team":
				if _, err := s.CreateTeam(ctx, p.ID); err != nil {
					t.Fatal(err)
				}
			}
			_, err := s.ClaimAutomation(ctx, tick(a), at)
			if mode == "busy" || mode == "missed" {
				if !errors.Is(err, hub.ErrAutomationSkipped) {
					t.Fatal("not skipped", err)
				}
			} else if !errors.Is(err, hub.ErrAutomationRouteChanged) {
				t.Fatal("not paused", err)
			}
			a, err = s.Automation(ctx, p.ID, a.ID)
			if err != nil || a.RunningTurnID != "" || a.LastRunAt != 0 {
				t.Fatal("unexpected run", a, err)
			}
			if mode == "busy" || mode == "missed" {
				if !a.Enabled || a.NextRunAt <= at {
					t.Fatal("tick queued/caught up", a)
				}
			} else if a.Enabled || a.NextRunAt != 0 {
				t.Fatal("route veto rolled back", a)
			}
		})
	}
}

func TestAutomationRuntimeFailureNeverFallsBackAndStopPauses(t *testing.T) {
	for _, stop := range []bool{false, true} {
		t.Run(map[bool]string{false: "startup", true: "stop"}[stop], func(t *testing.T) {
			s := newStore(t)
			ctx := context.Background()
			p, a := schedule(t, s)
			a = enableSchedule(t, s, a)
			fallback := target("b")
			fallback.AutomaticAuthorized = true
			if err := s.SetFallback(ctx, p.ID, []hub.Target{fallback}); err != nil {
				t.Fatal(err)
			}
			r := hub.New(s)
			var calls atomic.Int32
			entered := make(chan struct{})
			if err := r.Register("a", runtimeFunc{control: true, run: func(ctx context.Context, _ engine.Request, emit func(engine.Event) error) error {
				calls.Add(1)
				close(entered)
				if stop {
					<-ctx.Done()
					return ctx.Err()
				}
				if err := emit(engine.Event{Kind: engine.Error, Text: "startup fixture", Failure: engine.FailureStartup}); err != nil {
					return err
				}
				return emit(engine.Event{Kind: engine.Completed, Outcome: engine.Failed})
			}}); err != nil {
				t.Fatal(err)
			}
			if err := r.Register("b", runtimeFunc{run: func(context.Context, engine.Request, func(engine.Event) error) error { calls.Add(100); return nil }}); err != nil {
				t.Fatal(err)
			}
			_, done, err := r.StartAutomation(ctx, ctx, tick(a), a.NextRunAt)
			if err != nil {
				t.Fatal(err)
			}
			<-entered
			if stop {
				if err = r.Cancel(ctx, p.ID); err != nil {
					t.Fatal(err)
				}
			}
			<-done
			settled, err := s.Automation(ctx, p.ID, a.ID)
			if err != nil || settled.Enabled || settled.NextRunAt != 0 || settled.RunningTurnID != "" || calls.Load() != 1 {
				t.Fatal("fallback/replay/orphan", settled, calls.Load(), err)
			}
			if stop && settled.LastStatus != "interrupted" {
				t.Fatal("Stop not represented", settled)
			}
		})
	}
}

func TestAutomationTerminalSettlementIsOneTransactionAndRecoveryNeverReplays(t *testing.T) {
	ctx := context.Background()
	dir := filepath.Join(t.TempDir(), "store")
	s, err := Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	p, idle := schedule(t, s)
	idle = enableSchedule(t, s, idle)
	running, err := s.CreateAutomation(ctx, p.ID, hub.AutomationDraft{Title: "second", Prompt: "exact", IntervalMinutes: 5})
	if err != nil {
		t.Fatal(err)
	}
	running = enableSchedule(t, s, running)
	claim, err := s.ClaimAutomation(ctx, tick(running), running.NextRunAt)
	if err != nil {
		t.Fatal(err)
	}
	if err = s.Record(ctx, claim.Attempt.ID, engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded}, false); err != nil {
		t.Fatal(err)
	}
	if _, err = s.db.ExecContext(ctx, `CREATE TRIGGER reject_automation_settle BEFORE UPDATE ON hub_automations BEGIN SELECT RAISE(ABORT,'fixture'); END`); err != nil {
		t.Fatal(err)
	}
	if err = s.FinishTurn(ctx, claim.Turn.ID, engine.Succeeded); err == nil {
		t.Fatal("terminal committed without schedule settlement")
	}
	turn, err := read[hub.Turn](ctx, s.db, `SELECT data FROM hub_turns WHERE id=?`, claim.Turn.ID)
	if err != nil || turn.Status != hub.Running {
		t.Fatal("partial terminal transaction", turn, err)
	}
	if _, err = s.db.ExecContext(ctx, `DROP TRIGGER reject_automation_settle`); err != nil {
		t.Fatal(err)
	}
	if err = s.Close(); err != nil {
		t.Fatal(err)
	}
	s, err = Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	defer s.Close()
	recovered, err := s.Automation(ctx, p.ID, running.ID)
	if err != nil || recovered.Enabled || recovered.RunningTurnID != "" || recovered.LastStatus != "interrupted" {
		t.Fatal("interrupted tick replayable", recovered, err)
	}
	idle, err = s.Automation(ctx, p.ID, idle.ID)
	if err != nil || !idle.Enabled || idle.NextRunAt <= now() {
		t.Fatal("idle restart catch-up", idle, err)
	}
	if due, err := s.DueAutomations(ctx, now()); err != nil || len(due) != 0 {
		t.Fatal("startup due replay", due, err)
	}
	turn, err = read[hub.Turn](ctx, s.db, `SELECT data FROM hub_turns WHERE id=?`, claim.Turn.ID)
	if err != nil || turn.Status != engine.Succeeded {
		t.Fatal("known completed attempt rewritten by recovery", turn, err)
	}
}
