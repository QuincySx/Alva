// INPUT: Real private product SQLite, queued Worker intents and deterministic engine events
// OUTPUT: Complete Worker routing, FIFO claims, lifecycle and crash recovery regressions
// POS: [L7 Hub product] No native subprocess, model billing, credentials or in-engine subagent stand-ins
package sqlite

import (
	"context"
	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	"path/filepath"
	"testing"
)

func teamFixture(t *testing.T, s *Store) (hub.Session, hub.TeamDetail, hub.Worker, hub.Worker) {
	t.Helper()
	ctx := context.Background()
	lead, err := s.CreateSession(ctx, "Lead", t.TempDir(), target("a"))
	if err != nil {
		t.Fatal(err)
	}
	team, err := s.CreateTeam(ctx, lead.ID)
	if err != nil {
		t.Fatal(err)
	}
	w1, err := s.AddWorker(ctx, lead.ID, team.Team.ID, "one", target("a"))
	if err != nil {
		t.Fatal(err)
	}
	w2, err := s.AddWorker(ctx, lead.ID, team.Team.ID, "two", target("b"))
	if err != nil {
		t.Fatal(err)
	}
	return lead, team, w1, w2
}
func enqueue(t *testing.T, s *Store, lead, team, worker, scope string, claims []hub.ResourceClaim) hub.WorkerTask {
	t.Helper()
	v, err := s.EnqueueWorker(context.Background(), lead, team, worker, scope, claims)
	if err != nil {
		t.Fatal(err)
	}
	return v
}
func TestTeamWorkerOwnSessionFIFOAndReversibleBranches(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	lead, team, w1, w2 := teamFixture(t, s)
	if w1.SessionID == w2.SessionID || w1.SessionID == lead.ID {
		t.Fatal("Workers are not separate products")
	}
	if _, err := s.CreateTeam(ctx, lead.ID); err == nil {
		t.Fatal("two active Teams on Lead")
	}
	if _, err := s.CreateTeam(ctx, w1.SessionID); err == nil {
		t.Fatal("nested Worker product team")
	}
	if _, err := s.Team(ctx, w1.SessionID, team.Team.ID); err == nil {
		t.Fatal("foreign Lead read Team")
	}
	page, err := s.PageSessions(ctx, "", hub.SessionCursor{}, 128)
	if err != nil || len(page.Sessions) != 1 || page.Sessions[0].ID != lead.ID {
		t.Fatal("Workers leaked into home", err)
	}
	if _, err = s.BeginTurn(ctx, w1.SessionID, "bypass"); err == nil {
		t.Fatal("plain Turn bypassed FIFO")
	}
	a := enqueue(t, s, lead.ID, team.Team.ID, w1.ID, "first", []hub.ResourceClaim{{Path: "src", Write: true}})
	b := enqueue(t, s, lead.ID, team.Team.ID, w1.ID, "second", nil)
	if err = s.EditQueuedTask(ctx, lead.ID, team.Team.ID, w1.ID, b.ID, "edited second", nil); err != nil {
		t.Fatal(err)
	}
	claim, err := s.ClaimWorkerTask(ctx, lead.ID, team.Team.ID, w1.ID)
	if err != nil || claim.Claim == nil || claim.Task.ID != a.ID || claim.Claim.Session.ID != w1.SessionID {
		t.Fatal("bad FIFO admission", err)
	}
	if err = s.CancelQueuedTask(ctx, lead.ID, team.Team.ID, w1.ID, a.ID); err == nil {
		t.Fatal("queued cancel rewrote running task")
	}
	complete(t, s, *claim.Claim, "worker-native-a")
	tasks, err := s.WorkerTasks(ctx, lead.ID, team.Team.ID, w1.ID, 0, 32)
	if err != nil || tasks.Tasks[0].Outcome == nil || tasks.Tasks[0].Outcome.Status != engine.Succeeded || len(tasks.Tasks[0].Outcome.Verification) != 0 {
		t.Fatal("lost terminal/invented evidence", err)
	}
	if _, err = s.Switch(ctx, w1.SessionID, target("b"), "trial"); err != nil {
		t.Fatal(err)
	}
	returned, err := s.Switch(ctx, w1.SessionID, target("a"), "return")
	if err != nil || returned.ID != claim.Claim.Branch.ID || returned.NativeSessionID != "worker-native-a" {
		t.Fatal("Worker native identity changed", err)
	}
	second, err := s.ClaimWorkerTask(ctx, lead.ID, team.Team.ID, w1.ID)
	if err != nil || second.Task.ID != b.ID || second.Claim == nil || second.Claim.Turn.Prompt != "edited second" {
		t.Fatal("FIFO edit lost", err)
	}
	complete(t, s, *second.Claim, "worker-native-a")
	entries, err := s.History(ctx, lead.ID, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	count := 0
	for _, e := range entries {
		if e.Timeline != nil && e.Timeline.Kind == "worker_outcome" {
			count++
		}
	}
	if count != 2 {
		t.Fatal("missing Lead terminal receipts")
	}
}
func TestResourceFairnessLifecycleAndQueuedCancellation(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	lead, team, w1, w2 := teamFixture(t, s)
	older := enqueue(t, s, lead.ID, team.Team.ID, w1.ID, "older writer", []hub.ResourceClaim{{Path: "src", Write: true}})
	enqueue(t, s, lead.ID, team.Team.ID, w2.ID, "younger reader", []hub.ResourceClaim{{Path: "src/a.go"}})
	blocked, err := s.ClaimWorkerTask(ctx, lead.ID, team.Team.ID, w2.ID)
	if err != nil || blocked.Claim != nil || len(blocked.BlockedBy) != 1 || blocked.BlockedBy[0] != older.ID {
		t.Fatal("younger conflicting head jumped queue", err)
	}
	if err = s.SetWorkerLifecycle(ctx, lead.ID, team.Team.ID, w1.ID, hub.WorkerHibernated); err != nil {
		t.Fatal(err)
	}
	dormant, err := s.ClaimWorkerTask(ctx, lead.ID, team.Team.ID, w1.ID)
	if err != nil || !dormant.Dormant {
		t.Fatal("hibernated queue consumed", err)
	}
	reader, err := s.ClaimWorkerTask(ctx, lead.ID, team.Team.ID, w2.ID)
	if err != nil || reader.Claim == nil {
		t.Fatal("dormant reservation blocked live work", err)
	}
	if err = s.EndTeam(ctx, lead.ID, team.Team.ID); err == nil {
		t.Fatal("Team ended over running Worker")
	}
	complete(t, s, *reader.Claim, "reader-native")
	if err = s.CancelQueuedTask(ctx, lead.ID, team.Team.ID, w1.ID, older.ID); err != nil {
		t.Fatal(err)
	}
	if err = s.SetWorkerLifecycle(ctx, lead.ID, team.Team.ID, w1.ID, hub.WorkerArchived); err != nil {
		t.Fatal(err)
	}
	if _, err = s.EnqueueWorker(ctx, lead.ID, team.Team.ID, w1.ID, "must restore", nil); err == nil {
		t.Fatal("archived Worker silently woke")
	}
	if err = s.SetWorkerLifecycle(ctx, lead.ID, team.Team.ID, w1.ID, hub.WorkerHibernated); err != nil {
		t.Fatal(err)
	}
	enqueue(t, s, lead.ID, team.Team.ID, w1.ID, "wake", nil)
	d, err := s.Team(ctx, lead.ID, team.Team.ID)
	if err != nil {
		t.Fatal(err)
	}
	for _, w := range d.Workers {
		if w.ID == w1.ID && w.Lifecycle != hub.WorkerActive {
			t.Fatal("enqueue did not wake same hibernated identity")
		}
	}
	if err = s.EndTeam(ctx, lead.ID, team.Team.ID); err != nil {
		t.Fatal(err)
	}
	d, err = s.Team(ctx, lead.ID, team.Team.ID)
	if err != nil {
		t.Fatal(err)
	}
	for _, w := range d.Workers {
		if w.Lifecycle != hub.WorkerArchived {
			t.Fatal("Team end did not preserve archived Worker")
		}
	}
}

func TestPinnedWorkerDraftWaitsForItsExactBranchWithoutReservingOtherWorkers(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	lead, team, w1, w2 := teamFixture(t, s)
	detail, err := s.Detail(ctx, w1.SessionID)
	if err != nil {
		t.Fatal(err)
	}
	branch := detail.Session.ActiveBranchID
	if _, err := s.EnqueueWorkerOnBranch(ctx, lead.ID, team.Team.ID, w1.ID, "foreign-branch", "draft", nil); err == nil {
		t.Fatal("foreign draft branch admitted")
	}
	task, err := s.EnqueueWorkerOnBranch(ctx, lead.ID, team.Team.ID, w1.ID, branch, "explicit attachment reference draft", nil)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := s.Switch(ctx, w1.SessionID, target("b"), "park original draft route"); err != nil {
		t.Fatal(err)
	}
	if _, err := s.ClaimWorkerTask(ctx, lead.ID, team.Team.ID, w1.ID); err == nil {
		t.Fatal("draft followed another branch")
	}
	enqueue(t, s, lead.ID, team.Team.ID, w2.ID, "unrelated actual Worker", nil)
	other, err := s.ClaimWorkerTask(ctx, lead.ID, team.Team.ID, w2.ID)
	if err != nil || other.Claim == nil {
		t.Fatal("parked draft reserved shared resources", err, other)
	}
	complete(t, s, *other.Claim, "other-native")
	if _, err := s.Switch(ctx, w1.SessionID, target("a"), "explicit route return"); err != nil {
		t.Fatal(err)
	}
	actual, err := s.ClaimWorkerTask(ctx, lead.ID, team.Team.ID, w1.ID)
	if err != nil || actual.Claim == nil || actual.Task.ID != task.ID || actual.Claim.Branch.ID != branch {
		t.Fatal("original FIFO/native branch lost", actual, err)
	}
	complete(t, s, *actual.Claim, "draft-native")
}
func TestTeamCrashRecoveryUnknownNotReplayAndPreservesQueued(t *testing.T) {
	ctx := context.Background()
	dir := filepath.Join(t.TempDir(), "private")
	s, err := Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	lead, team, w, _ := teamFixture(t, s)
	enqueue(t, s, lead.ID, team.Team.ID, w.ID, "unknown active", nil)
	enqueue(t, s, lead.ID, team.Team.ID, w.ID, "never started", nil)
	claim, err := s.ClaimWorkerTask(ctx, lead.ID, team.Team.ID, w.ID)
	if err != nil || claim.Claim == nil {
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
	page, err := s.WorkerTasks(ctx, lead.ID, team.Team.ID, w.ID, 0, 32)
	if err != nil || len(page.Tasks) != 2 || page.Tasks[0].Status != engine.Interrupted || page.Tasks[0].Outcome == nil || page.Tasks[1].Status != hub.TaskQueued {
		t.Fatal("recovery replayed/lost unknown or pending task", err)
	}
	d, err := s.TeamForLead(ctx, lead.ID)
	if err != nil || d.Team.ID != team.Team.ID {
		t.Fatal("Team identity did not survive", err)
	}
}
