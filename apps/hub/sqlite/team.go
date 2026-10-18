// INPUT: Shared product SQL transactions, Hub team contracts and durable session routing
// OUTPUT: Actual independent Worker sessions, FIFO task claims, lifecycle and terminal receipts
// POS: [L7 Hub product] Team facts share product.sqlite; native execution records stay in their own owner-scoped repository
package sqlite

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"path/filepath"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

func teamRecord(ctx context.Context, tx *sql.Tx, lead, id string) (hub.TeamRun, error) {
	return read[hub.TeamRun](ctx, tx, `SELECT data FROM hub_teams WHERE id=? AND lead=?`, id, lead)
}
func workerRecord(ctx context.Context, tx *sql.Tx, team, id string) (hub.Worker, error) {
	return read[hub.Worker](ctx, tx, `SELECT data FROM hub_workers WHERE id=? AND team=?`, id, team)
}
func writeWorker(ctx context.Context, tx *sql.Tx, w hub.Worker) error {
	data, err := body(w)
	if err != nil {
		return err
	}
	_, err = tx.ExecContext(ctx, `UPDATE hub_workers SET state=?,data=? WHERE id=? AND team=?`, w.Lifecycle, data, w.ID, w.TeamID)
	return err
}
func writeTask(ctx context.Context, tx *sql.Tx, t hub.WorkerTask) error {
	data, err := body(t)
	if err != nil {
		return err
	}
	var turn any
	if t.TurnID != "" {
		turn = t.TurnID
	}
	_, err = tx.ExecContext(ctx, `UPDATE hub_worker_tasks SET status=?,turn=?,data=? WHERE id=? AND worker=? AND team=?`, t.Status, turn, data, t.ID, t.WorkerID, t.TeamID)
	return err
}
func teamDetail(ctx context.Context, tx *sql.Tx, lead, id string) (hub.TeamDetail, error) {
	var d hub.TeamDetail
	var err error
	d.Team, err = teamRecord(ctx, tx, lead, id)
	if err != nil {
		return d, err
	}
	rows, err := tx.QueryContext(ctx, `SELECT data FROM hub_workers WHERE team=? ORDER BY id LIMIT 17`, id)
	if err != nil {
		return d, err
	}
	defer rows.Close()
	for rows.Next() {
		var data []byte
		var w hub.Worker
		if err = rows.Scan(&data); err != nil {
			return d, err
		}
		if len(data) > 128<<10 || json.Unmarshal(data, &w) != nil {
			return d, errors.New("worker record corrupt")
		}
		d.Workers = append(d.Workers, w)
	}
	if len(d.Workers) > 16 {
		return d, errors.New("team worker budget exceeded")
	}
	return d, rows.Err()
}
func (s *Store) CreateTeam(ctx context.Context, lead string) (hub.TeamDetail, error) {
	var d hub.TeamDetail
	err := s.transaction(ctx, func(tx *sql.Tx) error {
		if _, err := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, lead); err != nil {
			return err
		}
		var worker int
		if err := tx.QueryRowContext(ctx, `SELECT count(*) FROM hub_workers WHERE session=?`, lead).Scan(&worker); err != nil {
			return err
		}
		if worker != 0 {
			return errors.New("a Hub Worker cannot create a nested product team")
		}
		id, err := hub.NewID("team-")
		if err != nil {
			return err
		}
		d.Team = hub.TeamRun{ID: id, LeadSessionID: lead, Status: "active", CreatedAt: now()}
		data, err := body(d.Team)
		if err != nil {
			return err
		}
		_, err = tx.ExecContext(ctx, `INSERT INTO hub_teams VALUES(?,?,?,?)`, id, lead, "active", data)
		return err
	})
	return d, err
}
func (s *Store) Team(ctx context.Context, lead, id string) (hub.TeamDetail, error) {
	var d hub.TeamDetail
	err := s.transaction(ctx, func(tx *sql.Tx) error { var err error; d, err = teamDetail(ctx, tx, lead, id); return err })
	return d, err
}
func (s *Store) TeamForLead(ctx context.Context, lead string) (hub.TeamDetail, error) {
	var d hub.TeamDetail
	err := s.transaction(ctx, func(tx *sql.Tx) error {
		var id string
		if err := tx.QueryRowContext(ctx, `SELECT id FROM hub_teams WHERE lead=? ORDER BY rowid DESC LIMIT 1`, lead).Scan(&id); err != nil {
			return err
		}
		var err error
		d, err = teamDetail(ctx, tx, lead, id)
		return err
	})
	return d, err
}
func (s *Store) AddWorker(ctx context.Context, lead, team, name string, target hub.Target) (hub.Worker, error) {
	var w hub.Worker
	if err := target.Validate(); err != nil {
		return w, err
	}
	if strings.TrimSpace(name) == "" || len(name) > 80 || !utf8.ValidString(name) {
		return w, errors.New("worker name must be 1..80 bytes")
	}
	err := s.transaction(ctx, func(tx *sql.Tx) error {
		t, err := teamRecord(ctx, tx, lead, team)
		if err != nil {
			return err
		}
		if t.Status != "active" {
			return errors.New("team has ended")
		}
		var count int
		if err = tx.QueryRowContext(ctx, `SELECT count(*) FROM hub_workers WHERE team=?`, team).Scan(&count); err != nil {
			return err
		}
		if count >= 16 {
			return errors.New("team has reached 16 durable workers")
		}
		parent, err := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, lead)
		if err != nil {
			return err
		}
		id, err := hub.NewID("worker-")
		if err != nil {
			return err
		}
		sid, err := hub.NewID("hub-")
		if err != nil {
			return err
		}
		bid, err := hub.NewID("branch-")
		if err != nil {
			return err
		}
		child := hub.Session{ID: sid, Title: name, Workspace: parent.Workspace, ActiveBranchID: bid, Status: engine.Succeeded, CreatedAt: now(), UpdatedAt: now()}
		data, err := body(child)
		if err != nil {
			return err
		}
		if _, err = tx.ExecContext(ctx, `INSERT INTO hub_sessions VALUES(?,?,?,?)`, sid, child.Status, child.UpdatedAt, data); err != nil {
			return err
		}
		if err = insertBranch(ctx, tx, hub.Branch{ID: bid, SessionID: sid, Target: target}); err != nil {
			return err
		}
		w = hub.Worker{ID: id, TeamID: team, SessionID: sid, Name: name, Lifecycle: hub.WorkerActive, CreatedAt: now()}
		data, err = body(w)
		if err != nil {
			return err
		}
		_, err = tx.ExecContext(ctx, `INSERT INTO hub_workers VALUES(?,?,?,?,?,?)`, id, team, sid, w.Lifecycle, name, data)
		return err
	})
	return w, err
}
func (s *Store) SetWorkerLifecycle(ctx context.Context, lead, team, id string, state hub.WorkerLifecycle) error {
	if state != hub.WorkerActive && state != hub.WorkerArchived && state != hub.WorkerHibernated {
		return errors.New("unknown worker lifecycle")
	}
	return s.transaction(ctx, func(tx *sql.Tx) error {
		t, err := teamRecord(ctx, tx, lead, team)
		if err != nil {
			return err
		}
		if t.Status != "active" {
			return errors.New("team has ended")
		}
		w, err := workerRecord(ctx, tx, team, id)
		if err != nil {
			return err
		}
		session, err := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, w.SessionID)
		if err != nil {
			return err
		}
		if session.Status == hub.Running {
			return errors.New("stop actual Worker execution before changing visibility")
		}
		if state != hub.WorkerArchived && w.Worktree != nil && (w.Worktree.State == "removing" || w.Worktree.State == "removed" || w.Worktree.State == "removal_unknown") {
			return errors.New("removed/unknown worktree Worker is retired; create a new Worker without retargeting native history")
		}
		w.Lifecycle = state
		return writeWorker(ctx, tx, w)
	})
}
func (s *Store) EndTeam(ctx context.Context, lead, id string) error {
	return s.transaction(ctx, func(tx *sql.Tx) error {
		d, err := teamDetail(ctx, tx, lead, id)
		if err != nil {
			return err
		}
		if d.Team.Status != "active" {
			return errors.New("team already ended")
		}
		for _, w := range d.Workers {
			child, err := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, w.SessionID)
			if err != nil {
				return err
			}
			if child.Status == hub.Running {
				return errors.New("stop all actual Workers before ending Team")
			}
			w.Lifecycle = hub.WorkerArchived
			if err = writeWorker(ctx, tx, w); err != nil {
				return err
			}
		}
		d.Team.Status = "ended"
		data, err := body(d.Team)
		if err != nil {
			return err
		}
		_, err = tx.ExecContext(ctx, `UPDATE hub_teams SET status=?,data=? WHERE id=? AND lead=?`, d.Team.Status, data, id, lead)
		return err
	})
}
func (s *Store) EnqueueWorker(ctx context.Context, lead, team, id, scope string, claims []hub.ResourceClaim) (hub.WorkerTask, error) {
	return s.enqueueWorker(ctx, lead, team, id, "", scope, claims)
}
func (s *Store) EnqueueWorkerOnBranch(ctx context.Context, lead, team, id, branch, scope string, claims []hub.ResourceClaim) (hub.WorkerTask, error) {
	if branch == "" {
		return hub.WorkerTask{}, errors.New("exact saved Worker draft branch required")
	}
	return s.enqueueWorker(ctx, lead, team, id, branch, scope, claims)
}
func (s *Store) enqueueWorker(ctx context.Context, lead, team, id, requiredBranch, scope string, claims []hub.ResourceClaim) (hub.WorkerTask, error) {
	var task hub.WorkerTask
	if len(claims) == 0 {
		claims = []hub.ResourceClaim{{Path: ".", Write: true}}
	}
	if err := hub.ValidateTask(scope, claims); err != nil {
		return task, err
	}
	err := s.transaction(ctx, func(tx *sql.Tx) error {
		t, err := teamRecord(ctx, tx, lead, team)
		if err != nil {
			return err
		}
		if t.Status != "active" {
			return errors.New("team has ended")
		}
		w, err := workerRecord(ctx, tx, team, id)
		if err != nil {
			return err
		}
		if w.Lifecycle == hub.WorkerArchived {
			return errors.New("archived Worker requires explicit restore")
		}
		if requiredBranch != "" {
			current, err := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, w.SessionID)
			if err != nil {
				return err
			}
			if current.ActiveBranchID != requiredBranch {
				return errors.New("Worker draft branch changed before queue admission; draft retained")
			}
		}
		var local, global int
		if err = tx.QueryRowContext(ctx, `SELECT count(*) FROM hub_worker_tasks WHERE worker=? AND status=?`, id, hub.TaskQueued).Scan(&local); err != nil {
			return err
		}
		if err = tx.QueryRowContext(ctx, `SELECT count(*) FROM hub_worker_tasks WHERE status=?`, hub.TaskQueued).Scan(&global); err != nil {
			return err
		}
		if local >= 32 || global >= 256 {
			return errors.New("queued task budget reached: 32 per Worker, 256 globally")
		}
		tid, err := hub.NewID("task-")
		if err != nil {
			return err
		}
		task = hub.WorkerTask{ID: tid, TeamID: team, WorkerID: id, Scope: scope, Claims: append([]hub.ResourceClaim(nil), claims...), Status: hub.TaskQueued, CreatedAt: now(), RequiredBranchID: requiredBranch}
		data, err := body(task)
		if err != nil {
			return err
		}
		result, err := tx.ExecContext(ctx, `INSERT INTO hub_worker_tasks(id,team,worker,status,data) VALUES(?,?,?,?,?)`, tid, team, id, task.Status, data)
		if err != nil {
			return err
		}
		seq, err := result.LastInsertId()
		if err != nil {
			return err
		}
		task.Sequence = uint64(seq)
		if err = writeTask(ctx, tx, task); err != nil {
			return err
		}
		w.Lifecycle = hub.WorkerActive
		return writeWorker(ctx, tx, w)
	})
	return task, err
}
func queuedTask(ctx context.Context, tx *sql.Tx, lead, team, worker, id string) (hub.WorkerTask, error) {
	if _, err := teamRecord(ctx, tx, lead, team); err != nil {
		return hub.WorkerTask{}, err
	}
	if _, err := workerRecord(ctx, tx, team, worker); err != nil {
		return hub.WorkerTask{}, err
	}
	task, err := read[hub.WorkerTask](ctx, tx, `SELECT data FROM hub_worker_tasks WHERE id=? AND worker=? AND team=?`, id, worker, team)
	if err != nil {
		return task, err
	}
	if task.Status != hub.TaskQueued {
		return task, errors.New("only an unconsumed queued task can be edited/cancelled")
	}
	return task, nil
}
func (s *Store) EditQueuedTask(ctx context.Context, lead, team, worker, id, scope string, claims []hub.ResourceClaim) error {
	if len(claims) == 0 {
		claims = []hub.ResourceClaim{{Path: ".", Write: true}}
	}
	if err := hub.ValidateTask(scope, claims); err != nil {
		return err
	}
	return s.transaction(ctx, func(tx *sql.Tx) error {
		task, err := queuedTask(ctx, tx, lead, team, worker, id)
		if err != nil {
			return err
		}
		task.Scope = scope
		task.Claims = append([]hub.ResourceClaim(nil), claims...)
		return writeTask(ctx, tx, task)
	})
}
func (s *Store) CancelQueuedTask(ctx context.Context, lead, team, worker, id string) error {
	return s.transaction(ctx, func(tx *sql.Tx) error {
		task, err := queuedTask(ctx, tx, lead, team, worker, id)
		if err != nil {
			return err
		}
		task.Status = engine.Cancelled
		task.EndedAt = now()
		return writeTask(ctx, tx, task)
	})
}
func (s *Store) WorkerTasks(ctx context.Context, lead, team, worker string, after uint64, limit int) (hub.WorkerTaskPage, error) {
	var page hub.WorkerTaskPage
	if limit < 1 || limit > 32 {
		return page, errors.New("task page must be 1..32")
	}
	err := s.transaction(ctx, func(tx *sql.Tx) error {
		if _, err := teamRecord(ctx, tx, lead, team); err != nil {
			return err
		}
		if _, err := workerRecord(ctx, tx, team, worker); err != nil {
			return err
		}
		rows, err := tx.QueryContext(ctx, `SELECT data FROM hub_worker_tasks WHERE team=? AND worker=? AND seq>? ORDER BY seq LIMIT ?`, team, worker, after, limit+1)
		if err != nil {
			return err
		}
		defer rows.Close()
		for rows.Next() {
			var data []byte
			if err = rows.Scan(&data); err != nil {
				return err
			}
			if len(page.Tasks) == limit {
				page.HasMore = true
				break
			}
			var task hub.WorkerTask
			if len(data) > 128<<10 || json.Unmarshal(data, &task) != nil {
				return errors.New("task record corrupt")
			}
			page.Tasks = append(page.Tasks, task)
			page.Next = task.Sequence
		}
		return rows.Err()
	})
	return page, err
}
func absoluteClaims(workspace string, claims []hub.ResourceClaim) []hub.ResourceClaim {
	out := make([]hub.ResourceClaim, 0, len(claims))
	for _, claim := range claims {
		out = append(out, hub.ResourceClaim{Path: filepath.Join(workspace, claim.Path), Write: claim.Write})
	}
	return out
}
func taskClaimsConflict(wa string, a []hub.ResourceClaim, wb string, b []hub.ResourceClaim) bool {
	for _, l := range absoluteClaims(wa, a) {
		for _, r := range absoluteClaims(wb, b) {
			if !(l.Write || r.Write) {
				continue
			}
			rel, err := filepath.Rel(l.Path, r.Path)
			if err == nil && (rel == "." || filepath.IsLocal(rel)) {
				return true
			}
			rel, err = filepath.Rel(r.Path, l.Path)
			if err == nil && (rel == "." || filepath.IsLocal(rel)) {
				return true
			}
		}
	}
	return false
}
func (s *Store) ClaimWorkerTask(ctx context.Context, lead, team, worker string) (hub.WorkerClaim, error) {
	var result hub.WorkerClaim
	err := s.transaction(ctx, func(tx *sql.Tx) error {
		t, err := teamRecord(ctx, tx, lead, team)
		if err != nil {
			return err
		}
		w, err := workerRecord(ctx, tx, team, worker)
		if err != nil {
			return err
		}
		if t.Status != "active" || w.Lifecycle != hub.WorkerActive {
			result.Dormant = true
			return nil
		}
		task, err := read[hub.WorkerTask](ctx, tx, `SELECT data FROM hub_worker_tasks WHERE worker=? AND team=? AND status=? ORDER BY seq LIMIT 1`, worker, team, hub.TaskQueued)
		if errors.Is(err, sql.ErrNoRows) {
			return nil
		}
		if err != nil {
			return err
		}
		result.Task = task
		current, err := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, w.SessionID)
		if err != nil {
			return err
		}
		if current.Status == hub.Running {
			return errors.New("Worker already has actual execution")
		}
		if task.RequiredBranchID != "" && task.RequiredBranchID != current.ActiveBranchID {
			return errors.New("queued Worker draft belongs to a parked branch; return to its saved branch or explicitly cancel/requeue the task, no attachment replay on another route")
		}
		// A queued older head is a fair reservation, not just an active lock.
		rows, err := tx.QueryContext(ctx, `SELECT t.data,s.data FROM hub_worker_tasks t JOIN hub_workers w ON w.id=t.worker JOIN hub_teams g ON g.id=t.team JOIN hub_sessions s ON s.id=w.session WHERE t.id<>? AND (t.status=? OR (t.status=? AND t.seq<? AND w.state=? AND g.status='active' AND (coalesce(json_extract(t.data,'$.required_branch_id'),'')='' OR json_extract(t.data,'$.required_branch_id')=json_extract(s.data,'$.active_branch_id')) AND NOT EXISTS(SELECT 1 FROM hub_worker_tasks h WHERE h.worker=t.worker AND h.status=? AND h.seq<t.seq))) ORDER BY t.seq LIMIT 321`, task.ID, hub.Running, hub.TaskQueued, task.Sequence, hub.WorkerActive, hub.TaskQueued)
		if err != nil {
			return err
		}
		count := 0
		for rows.Next() {
			count++
			var td, sd []byte
			var other hub.WorkerTask
			var session hub.Session
			if err = rows.Scan(&td, &sd); err != nil {
				break
			}
			if len(td) > 128<<10 || len(sd) > 128<<10 || json.Unmarshal(td, &other) != nil || json.Unmarshal(sd, &session) != nil {
				err = errors.New("claim metadata corrupt")
				break
			}
			if taskClaimsConflict(current.Workspace, task.Claims, session.Workspace, other.Claims) {
				result.BlockedBy = append(result.BlockedBy, other.ID)
			}
		}
		if err == nil {
			err = rows.Err()
		}
		rows.Close()
		if err != nil {
			return err
		}
		if count > 320 {
			return errors.New("resource reservation budget exceeded")
		}
		if len(result.BlockedBy) > 0 {
			return nil
		}
		claim, err := beginTurn(ctx, tx, w.SessionID, task.Scope)
		if err != nil {
			return err
		}
		task.Status = hub.Running
		task.TurnID = claim.Turn.ID
		if err = writeTask(ctx, tx, task); err != nil {
			return err
		}
		if err = refreshWorkerHandoff(ctx, tx, &claim); err != nil {
			return err
		}
		result.Task = task
		result.Claim = &claim
		return nil
	})
	return result, err
}

// settleWorkerTask runs in the same transaction as the authoritative Turn edge.
// It does not infer changed files, tests or successful tool effects from prose.
func settleWorkerTask(ctx context.Context, tx *sql.Tx, turn hub.Turn) error {
	task, err := read[hub.WorkerTask](ctx, tx, `SELECT data FROM hub_worker_tasks WHERE turn=?`, turn.ID)
	if errors.Is(err, sql.ErrNoRows) {
		return nil
	}
	if err != nil {
		return err
	}
	if task.Status != hub.Running {
		return errors.New("worker task settlement is stale")
	}
	task.Status = turn.Status
	task.EndedAt = now()
	task.Outcome = &hub.WorkerOutcome{Status: turn.Status, TurnID: turn.ID, Summary: "Actual product Turn reached " + string(turn.Status), Processing: "Real terminal status only. Changed files and verification were not independently reported; no success evidence inferred from chat."}
	// Select only a genuine completed assistant message from this terminal
	// attempt. Reasoning, tool I/O, prior attempts and prose about tests are not
	// promoted into result or verification facts. The durable sequence retains
	// an exact route back to the original product evidence.
	rows, err := tx.QueryContext(ctx, `SELECT seq,data FROM hub_events WHERE turn=? AND attempt=? ORDER BY seq DESC LIMIT 64`, turn.ID, turn.ActiveAttemptID)
	if err != nil {
		return err
	}
	for rows.Next() {
		var sequence uint64
		var data []byte
		var entry hub.HistoryEntry
		if err = rows.Scan(&sequence, &data); err != nil {
			break
		}
		if len(data) > 128<<10 || json.Unmarshal(data, &entry) != nil {
			err = errors.New("Worker outcome history corrupt")
			break
		}
		if entry.Event.Kind == engine.Message && entry.Event.Text != "" {
			text := entry.Event.Text
			if len(text) > 4096 {
				text = text[:4096]
				for !utf8.ValidString(text) {
					text = text[:len(text)-1]
				}
				text += " [excerpt; retrieve result_sequence]"
			}
			task.Outcome.Summary = text
			task.Outcome.ResultSequence = sequence
			task.Outcome.Processing = "Selected actual terminal-attempt assistant answer, up to 4 KiB. Changed files and verification are not independently reported or inferred from this prose."
			break
		}
	}
	if err == nil {
		err = rows.Err()
	}
	rows.Close()
	if err != nil {
		return err
	}
	if err = writeTask(ctx, tx, task); err != nil {
		return err
	}
	team, err := read[hub.TeamRun](ctx, tx, `SELECT data FROM hub_teams WHERE id=?`, task.TeamID)
	if err != nil {
		return err
	}
	data, err := body(hub.HistoryEntry{SessionID: team.LeadSessionID, Time: now(), Timeline: &hub.TimelineEvent{Kind: "worker_outcome", Text: "Worker " + task.WorkerID + " task " + task.ID + " → " + string(task.Status) + "; actual Turn " + turn.ID + ". Inspect Worker evidence explicitly."}})
	if err != nil {
		return err
	}
	_, err = tx.ExecContext(ctx, `INSERT INTO hub_events(session,data) VALUES(?,?)`, team.LeadSessionID, data)
	return err
}

var _ hub.TeamStore = (*Store)(nil)
