// INPUT: Actual SQLite product transactions, Waiting engine receipts and Worker/automation ownership.
// OUTPUT: Same-Turn continuation, branch-scoped review, crash-edge recovery and no automatic fallback regressions.
// POS: [L7 product acceptance] Native checkpoint storage stays independent of product routing and task queues.
package sqlite

import (
	"context"
	"errors"
	"path/filepath"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

func waitingReceipt() *engine.WaitCheckpoint {
	return &engine.WaitCheckpoint{Revision: 3, Requests: 1, ExpiresAt: time.Now().Add(time.Hour).UnixMilli()}
}
func recordWaiting(t *testing.T, s *Store, c hub.Claim) {
	t.Helper()
	for _, e := range []engine.Event{
		{Kind: engine.SessionStarted, SessionID: "waiting-native"},
		{Kind: engine.Completed, SessionID: "waiting-native", Outcome: engine.Waiting, Wait: waitingReceipt()},
	} {
		if err := s.Record(t.Context(), c.Attempt.ID, e, false); err != nil {
			t.Fatal(err)
		}
	}
}
func TestWaitingCrashEdgePreservesTurnWorkerAndAutomation(t *testing.T) {
	for _, mode := range []string{"ordinary", "worker", "automation"} {
		t.Run(mode, func(t *testing.T) {
			path := filepath.Join(t.TempDir(), "private")
			s, err := Open(t.Context(), path)
			if err != nil {
				t.Fatal(err)
			}
			defer func() { _ = s.Close() }()
			var c hub.Claim
			var lead hub.Session
			var team hub.TeamDetail
			var worker hub.Worker
			var task hub.WorkerTask
			var automation hub.Automation
			if mode == "worker" {
				lead, team, worker, _ = teamFixture(t, s)
				if err = s.SetDurableWaits(t.Context(), worker.SessionID, true); err != nil {
					t.Fatal(err)
				}
				task = enqueue(t, s, lead.ID, team.Team.ID, worker.ID, "original worker input", []hub.ResourceClaim{{Path: "src", Write: true}})
				enqueue(t, s, lead.ID, team.Team.ID, worker.ID, "still queued", nil)
				claimed, e := s.ClaimWorkerTask(t.Context(), lead.ID, team.Team.ID, worker.ID)
				if e != nil || claimed.Claim == nil {
					t.Fatal(claimed, e)
				}
				c = *claimed.Claim
			} else if mode == "automation" {
				_, automation = schedule(t, s)
				if err = s.SetDurableWaits(t.Context(), automation.SessionID, true); err != nil {
					t.Fatal(err)
				}
				automation = enableSchedule(t, s, automation)
				c, err = s.ClaimAutomation(t.Context(), tick(automation), automation.NextRunAt)
			} else {
				var session hub.Session
				session, err = s.CreateSession(t.Context(), mode, t.TempDir(), target("a"))
				if err == nil {
					err = s.SetDurableWaits(t.Context(), session.ID, true)
				}
				if err == nil {
					c, err = s.BeginTurn(t.Context(), session.ID, "original input")
				}
			}
			if err != nil {
				t.Fatal(err)
			}
			recordWaiting(t, s, c)
			// Crash boundary: receipt committed, FinishTurn not yet called.
			if err = s.Close(); err != nil {
				t.Fatal(err)
			}
			s, err = Open(t.Context(), path)
			if err != nil {
				t.Fatal(err)
			}
			d, e := s.Detail(t.Context(), c.Session.ID)
			turn, e2 := s.BranchTurn(t.Context(), c.Session.ID, c.Branch.ID)
			if e != nil || e2 != nil || d.Session.Status != engine.Waiting || turn.Status != engine.Waiting || turn.EndedAt != 0 || turn.ID != c.Turn.ID {
				t.Fatal(d, turn, e, e2)
			}
			if _, err = s.BeginTurn(t.Context(), c.Session.ID, "bypass"); err == nil {
				t.Fatal("new input bypassed waiting")
			}
			if err = s.SetDurableWaits(t.Context(), c.Session.ID, false); err == nil {
				t.Fatal("disabled an unresolved checkpoint")
			}
			if mode == "worker" {
				tasks, e := s.WorkerTasks(t.Context(), lead.ID, team.Team.ID, worker.ID, 0, 32)
				if e != nil || len(tasks.Tasks) != 2 || tasks.Tasks[0].ID != task.ID || tasks.Tasks[0].Status != hub.Running || tasks.Tasks[0].Outcome != nil || tasks.Tasks[1].Status != hub.TaskQueued {
					t.Fatal(tasks, e)
				}
			} else if mode == "automation" {
				a, e := s.Automation(t.Context(), c.Session.ID, automation.ID)
				if e != nil || a.RunningTurnID != c.Turn.ID || a.LastStatus != "waiting" || a.NextRunAt != 0 {
					t.Fatal(a, e)
				}
				if due, e := s.DueAutomations(t.Context(), time.Now().Add(48*time.Hour).UnixMilli()); e != nil || len(due) != 0 {
					t.Fatal("waiting timer replayed", due, e)
				}
			}
			for _, bad := range []struct {
				session, branch, turn string
				revision              uint64
			}{
				{c.Session.ID, c.Branch.ID, c.Turn.ID, 2},
				{c.Session.ID, "foreign", c.Turn.ID, 4},
				{c.Session.ID, c.Branch.ID, "foreign", 4},
				{"foreign", c.Branch.ID, c.Turn.ID, 4},
			} {
				if _, err = s.BeginWaitResume(t.Context(), bad.session, bad.branch, bad.turn, bad.revision); err == nil {
					t.Fatal("foreign/stale wait claim admitted", bad)
				}
			}
			resumed, e := s.BeginWaitResume(t.Context(), c.Session.ID, c.Branch.ID, c.Turn.ID, 4)
			if e != nil || resumed.Turn.ID != c.Turn.ID || resumed.Turn.Sequence != c.Turn.Sequence || resumed.Attempt.ID == c.Attempt.ID || resumed.Branch.NativeSessionID != "waiting-native" || resumed.Handoff != nil || resumed.WaitRevision != 4 {
				t.Fatal(resumed, e)
			}
			complete(t, s, resumed, "waiting-native")
			if mode == "worker" {
				tasks, e := s.WorkerTasks(t.Context(), lead.ID, team.Team.ID, worker.ID, 0, 32)
				if e != nil || tasks.Tasks[0].Outcome == nil || tasks.Tasks[0].Outcome.Status != engine.Succeeded || tasks.Tasks[1].Status != hub.TaskQueued {
					t.Fatal(tasks, e)
				}
			} else if mode == "automation" {
				a, e := s.Automation(t.Context(), c.Session.ID, automation.ID)
				if e != nil || a.RunningTurnID != "" || a.LastStatus != "completed" || a.NextRunAt == 0 {
					t.Fatal(a, e)
				}
			}
		})
	}
}

type waitingEngine struct {
	runtimeFunc
	capable bool
}

func (e waitingEngine) Capabilities() engine.Capabilities {
	c := e.runtimeFunc.Capabilities()
	c.Resume, c.DurableWaits = true, e.capable
	return c
}
func TestWaitingOwnerCapabilityAndCleanupFailuresCannotSucceed(t *testing.T) {
	for _, mode := range []string{"unsupported", "missing-port", "owner-failed", "resume-failed"} {
		t.Run(mode, func(t *testing.T) {
			s := newStore(t)
			p, err := s.CreateSession(t.Context(), mode, t.TempDir(), target("a"))
			if err != nil {
				t.Fatal(err)
			}
			if err = s.SetDurableWaits(t.Context(), p.ID, true); err != nil {
				t.Fatal(err)
			}
			fallback := target("b")
			fallback.AutomaticAuthorized = true
			if err = s.SetFallback(t.Context(), p.ID, []hub.Target{fallback}); err != nil {
				t.Fatal(err)
			}
			r := hub.NewWithWaits(s)
			if mode == "missing-port" {
				r = hub.New(executionOnlyStore{s})
			}
			defer func() { _ = r.Close(context.Background()) }()
			fallbackCalls := 0
			if err = r.Register("b", runtimeFunc{run: func(context.Context, engine.Request, func(engine.Event) error) error {
				fallbackCalls++
				return errors.New("unexpected fallback")
			}}); err != nil {
				t.Fatal(err)
			}
			if err = r.Register("a", waitingEngine{capable: mode != "unsupported", runtimeFunc: runtimeFunc{control: true, run: func(_ context.Context, req engine.Request, emit func(engine.Event) error) error {
				if req.WaitRevision != 0 {
					if req.Prompt != "" || req.ResumeSession != "waiting-native" {
						t.Error("continuation lost native identity or replayed input", req)
					}
					if e := emit(engine.Event{Kind: engine.Error, Text: "rate limited", Failure: engine.FailureRateLimit}); e != nil {
						return e
					}
					return emit(engine.Event{Kind: engine.Completed, Outcome: engine.Failed})
				}
				if e := emit(engine.Event{Kind: engine.SessionStarted, SessionID: "waiting-native"}); e != nil {
					return e
				}
				_ = emit(engine.Event{Kind: engine.Completed, Outcome: engine.Waiting, Wait: waitingReceipt()}) // faulty engines may ignore rejection
				if mode == "owner-failed" {
					return errors.New("actual owner cleanup failed")
				}
				return nil
			}}}); err != nil {
				t.Fatal(err)
			}
			turn, done, err := r.Start(t.Context(), p.ID, "original input")
			if err != nil {
				t.Fatal(err)
			}
			err = <-done
			if mode == "resume-failed" {
				if err != nil {
					t.Fatal(err)
				}
				d, e := s.Detail(t.Context(), p.ID)
				if e != nil || d.Session.Status != engine.Waiting {
					t.Fatal(d, e)
				}
				_, done, err = r.ResumeWait(t.Context(), p.ID, p.ActiveBranchID, turn, 4)
				if err != nil {
					t.Fatal(err)
				}
				err = <-done
			}
			d, e := s.Detail(t.Context(), p.ID)
			if err == nil || e != nil || d.Session.Status != engine.Failed || fallbackCalls != 0 || r.SessionActive(p.ID) {
				t.Fatal(d, fallbackCalls, err, e)
			}
		})
	}
}

func TestWaitReviewCannotBorrowAnotherBranchLatestTurn(t *testing.T) {
	s := newStore(t)
	p, err := s.CreateSession(t.Context(), "branch review", t.TempDir(), target("a"))
	if err != nil {
		t.Fatal(err)
	}
	a, err := s.BeginTurn(t.Context(), p.ID, "a input")
	if err != nil {
		t.Fatal(err)
	}
	complete(t, s, a, "native-a")
	b, err := s.Switch(t.Context(), p.ID, target("b"), "explicit branch")
	if err != nil {
		t.Fatal(err)
	}
	bClaim, err := s.BeginTurn(t.Context(), p.ID, "b input")
	if err != nil {
		t.Fatal(err)
	}
	complete(t, s, bClaim, "native-b")
	if _, err = s.Switch(t.Context(), p.ID, target("a"), "return"); err != nil {
		t.Fatal(err)
	}
	turn, err := s.BranchTurn(t.Context(), p.ID, a.Branch.ID)
	if err != nil || turn.ID != a.Turn.ID {
		t.Fatal(turn, err)
	}
	if err = s.AbandonWaitingTurn(t.Context(), p.ID, a.Branch.ID, bClaim.Turn.ID); err == nil {
		t.Fatal("abandonment borrowed another branch Turn")
	}
	if _, err = s.BranchTurn(t.Context(), "foreign", b.ID); !errors.Is(err, hub.ErrNotFound) {
		t.Fatal(err)
	}
}
