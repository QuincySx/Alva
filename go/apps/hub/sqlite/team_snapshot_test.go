// INPUT: Actual product transactions, independent Worker queues and engine fixture terminal events
// OUTPUT: Bounded immutable Lead/self-Worker handoffs, typed evidence indices and credential redaction regressions
// POS: [L7 Hub product] Team handoff never executes/cancels Workers or ships their task bodies/native logs
package sqlite

import (
	"context"
	"encoding/json"
	"fmt"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/go/apps/hub"
	"github.com/QuincySx/Alva/go/integration/engine"
)

func TestTeamHandoffSnapshotsIndicesWithoutChangingWorkersAndRedactsCredentials(t *testing.T) {
	ctx := context.Background()
	dir := filepath.Join(t.TempDir(), "private")
	s, err := Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = s.Close() }()
	lead, err := s.CreateSession(ctx, "Lead", t.TempDir(), target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	team, err := s.CreateTeam(ctx, lead.ID)
	if err != nil {
		t.Fatal(err)
	}
	w1, err := s.AddWorker(ctx, lead.ID, team.Team.ID, "Worker1", target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	w2, err := s.AddWorker(ctx, lead.ID, team.Team.ID, "Worker2", target("codex"))
	if err != nil {
		t.Fatal(err)
	}
	workerDetail, err := s.Detail(ctx, w1.SessionID)
	if err != nil {
		t.Fatal(err)
	}
	first, err := s.EnqueueWorkerOnBranch(ctx, lead.ID, team.Team.ID, w1.ID, workerDetail.Session.ActiveBranchID, "PRIVATE WORKER TASK BODY must not become handoff text", nil)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.EnqueueWorker(ctx, lead.ID, team.Team.ID, w1.ID, "another raw worker body", nil); err != nil {
		t.Fatal(err)
	}
	if _, err = s.EnqueueWorker(ctx, lead.ID, team.Team.ID, w2.ID, "archived task still retained", nil); err != nil {
		t.Fatal(err)
	}
	if err = s.SetWorkerLifecycle(ctx, lead.ID, team.Team.ID, w2.ID, hub.WorkerArchived); err != nil {
		t.Fatal(err)
	}
	running, err := s.ClaimWorkerTask(ctx, lead.ID, team.Team.ID, w1.ID)
	if err != nil || running.Claim == nil {
		t.Fatal(err)
	}
	old, err := s.BeginTurn(ctx, lead.ID, "normal request api_key=alva-fixture-secret-value")
	if err != nil {
		t.Fatal(err)
	}
	if err = s.Record(ctx, old.Attempt.ID, engine.Event{Kind: engine.SessionStarted, SessionID: "fixture-native-lead"}, false); err != nil {
		t.Fatal(err)
	}
	if err = s.Record(ctx, old.Attempt.ID, engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded}, false); err != nil {
		t.Fatal(err)
	}
	if err = s.FinishTurn(ctx, old.Turn.ID, engine.Succeeded); err != nil {
		t.Fatal(err)
	}
	if _, err = s.Switch(ctx, lead.ID, target("codex"), "explicit Lead engine switch"); err != nil {
		t.Fatal(err)
	}
	claim, err := s.BeginTurn(ctx, lead.ID, "continue as Lead only")
	if err != nil {
		t.Fatal(err)
	}
	if claim.Handoff == nil || claim.Handoff.Team == nil {
		t.Fatal("Lead Team context missing")
	}
	snapshot := claim.Handoff.Team
	if snapshot.Role != "lead" || snapshot.TeamID != team.Team.ID || len(snapshot.Workers) != 2 {
		t.Fatal("bad actual roster", snapshot)
	}
	var one, two *hub.TeamWorkerSnapshot
	for i := range snapshot.Workers {
		if snapshot.Workers[i].WorkerID == w1.ID {
			one = &snapshot.Workers[i]
		} else {
			two = &snapshot.Workers[i]
		}
	}
	if one == nil || one.Running != 1 || one.Queued != 1 || one.Status != hub.Running || one.Head == nil || one.Head.TaskID != first.ID || two == nil || two.Queued != 1 || two.Lifecycle != hub.WorkerArchived {
		t.Fatal("snapshot changed or lost actual queue state", snapshot)
	}
	if one.Head.RequiredBranchID != first.RequiredBranchID {
		t.Fatal("handoff omitted actual draft branch pin")
	}
	encoded, err := hub.EncodeHandoff(claim.Handoff)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(encoded, "alva-fixture-secret-value") || !strings.Contains(encoded, "[REDACTED]") || strings.Contains(encoded, "PRIVATE WORKER") || strings.Contains(encoded, "archived task still retained") {
		t.Fatal("handoff leaked source prose/credential", encoded)
	}
	detail, err := s.Detail(ctx, w1.SessionID)
	if err != nil || detail.Session.Status != hub.Running {
		t.Fatal("Lead switch stopped Worker", err)
	}
	page, err := s.WorkerTasks(ctx, lead.ID, team.Team.ID, w1.ID, 0, 8)
	if err != nil || page.Tasks[0].Status != hub.Running || page.Tasks[1].Status != hub.TaskQueued {
		t.Fatal("handoff consumed/replayed queue", err)
	}
	var persisted []byte
	if err = s.db.QueryRowContext(ctx, `SELECT data FROM hub_handoffs WHERE id=?`, claim.Handoff.ID).Scan(&persisted); err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(persisted), "alva-fixture-secret-value") {
		t.Fatal("durable handoff was not redacted before persistence")
	}
	if err = s.Close(); err != nil {
		t.Fatal(err)
	}
	s, err = Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	var after []byte
	if err = s.db.QueryRowContext(ctx, `SELECT data FROM hub_handoffs WHERE id=?`, claim.Handoff.ID).Scan(&after); err != nil {
		t.Fatal(err)
	}
	if string(after) != string(persisted) {
		t.Fatal("recovery rewrote captured snapshot instead of preserving original evidence")
	}
}

func TestFullTeamRosterAndLongHistoryStayWithinOneHandoffBudget(t *testing.T) {
	ctx := context.Background()
	s := newStore(t)
	lead, err := s.CreateSession(ctx, "Lead", t.TempDir(), target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	team, err := s.CreateTeam(ctx, lead.ID)
	if err != nil {
		t.Fatal(err)
	}
	for i := 0; i < 16; i++ {
		w, err := s.AddWorker(ctx, lead.ID, team.Team.ID, fmt.Sprint("worker", i), target("alva"))
		if err != nil {
			t.Fatal(err)
		}
		claims := []hub.ResourceClaim{{Path: fmt.Sprint("worker-", i), Write: true}}
		if _, err = s.EnqueueWorker(ctx, lead.ID, team.Team.ID, w.ID, "first actual task", claims); err != nil {
			t.Fatal(err)
		}
		run, err := s.ClaimWorkerTask(ctx, lead.ID, team.Team.ID, w.ID)
		if err != nil || run.Claim == nil {
			t.Fatal(err)
		}
		complete(t, s, *run.Claim, fmt.Sprint("worker-native-", i))
		if _, err = s.EnqueueWorker(ctx, lead.ID, team.Team.ID, w.ID, "next task remains queued", claims); err != nil {
			t.Fatal(err)
		}
	}
	for i := 0; i < 32; i++ {
		claim, err := s.BeginTurn(ctx, lead.ID, strings.Repeat("long bounded prompt🙂", 30))
		if err != nil {
			t.Fatal(err)
		}
		complete(t, s, claim, "lead-native")
	}
	if _, err = s.Switch(ctx, lead.ID, target("codex"), "full team handoff"); err != nil {
		t.Fatal(err)
	}
	claim, err := s.BeginTurn(ctx, lead.ID, "continue")
	if err != nil {
		t.Fatal(err)
	}
	if claim.Handoff == nil || claim.Handoff.Team == nil || len(claim.Handoff.Team.Workers) != 16 {
		t.Fatal("roster silently omitted")
	}
	if _, err = hub.EncodeHandoff(claim.Handoff); err != nil {
		t.Fatal("full metadata envelope exceeded 16KiB", err)
	}
	if len(claim.Handoff.Entries) == 32 || !strings.Contains(claim.Handoff.Processing, "Additional older Turn excerpts omitted") {
		t.Fatal("Turn excerpt budget did not adapt to full Team snapshot")
	}
	for _, worker := range claim.Handoff.Team.Workers {
		if worker.Head == nil || worker.LastOutcome == nil || worker.Queued != 1 || worker.Running != 0 {
			t.Fatal("queue/evidence index missing", worker)
		}
	}
}

func TestWorkerHandoffContainsOnlyOwnTeamIndicesAndNoSiblingLogGrant(t *testing.T) {
	ctx := context.Background()
	s, err := Open(ctx, filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	defer s.Close()
	lead, err := s.CreateSession(ctx, "Lead", t.TempDir(), target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	team, err := s.CreateTeam(ctx, lead.ID)
	if err != nil {
		t.Fatal(err)
	}
	worker, err := s.AddWorker(ctx, lead.ID, team.Team.ID, "self", target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	sibling, err := s.AddWorker(ctx, lead.ID, team.Team.ID, "sibling", target("codex"))
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.EnqueueWorker(ctx, lead.ID, team.Team.ID, worker.ID, "self queue prose", nil); err != nil {
		t.Fatal(err)
	}
	if _, err = s.Switch(ctx, worker.SessionID, target("codex"), "worker route changed explicitly"); err != nil {
		t.Fatal(err)
	}
	claimed, err := s.ClaimWorkerTask(ctx, lead.ID, team.Team.ID, worker.ID)
	if err != nil || claimed.Claim == nil {
		t.Fatal(err)
	}
	claim := claimed.Claim
	if claim.Handoff == nil || claim.Handoff.Team == nil || claim.Handoff.Team.Role != "worker" || len(claim.Handoff.Team.Workers) != 1 || claim.Handoff.Team.Workers[0].WorkerID != worker.ID {
		t.Fatal("worker received whole-team scope", claim.Handoff)
	}
	self := claim.Handoff.Team.Workers[0]
	if self.Status != hub.Running || self.Queued != 0 || self.Running != 1 || self.Head == nil || self.Head.TurnID != claim.Turn.ID || self.Head.Status != hub.Running {
		t.Fatal("Worker snapshot described pre-admission state instead of same-transaction actual task", self)
	}
	raw, _ := json.Marshal(claim.Handoff)
	if strings.Contains(string(raw), sibling.SessionID) || strings.Contains(string(raw), "self queue prose") {
		t.Fatal("worker snapshot contains sibling/task text", string(raw))
	}
}
