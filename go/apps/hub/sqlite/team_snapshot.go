// INPUT: One actual product transaction and committed Team/Worker queue metadata
// OUTPUT: Lead/full-team or Worker/self-only handoff snapshots with bounded task/evidence indices
// POS: [L7 Hub product] Reads no native logs, attachment bytes, task prose or external engine state and starts no work
package sqlite

import (
	"context"
	"database/sql"
	"errors"

	"github.com/QuincySx/Alva/go/apps/hub"
)

func teamTaskIndex(ctx context.Context, tx *sql.Tx, worker string, terminal bool) (*hub.TeamTaskIndex, error) {
	query := `SELECT id,seq,status,coalesce(turn,''),coalesce(json_extract(data,'$.outcome.result_sequence'),0),coalesce(json_extract(data,'$.required_branch_id'),'') FROM hub_worker_tasks WHERE worker=? AND status IN ('queued','running') ORDER BY CASE status WHEN 'running' THEN 0 ELSE 1 END,seq LIMIT 1`
	if terminal {
		query = `SELECT id,seq,status,coalesce(turn,''),coalesce(json_extract(data,'$.outcome.result_sequence'),0),coalesce(json_extract(data,'$.required_branch_id'),'') FROM hub_worker_tasks WHERE worker=? AND status NOT IN ('queued','running') ORDER BY seq DESC LIMIT 1`
	}
	var value hub.TeamTaskIndex
	err := tx.QueryRowContext(ctx, query, worker).Scan(&value.TaskID, &value.Sequence, &value.Status, &value.TurnID, &value.ResultSequence, &value.RequiredBranchID)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, nil
	}
	return &value, err
}

func teamSnapshot(ctx context.Context, tx *sql.Tx, id string) (*hub.TeamSnapshot, error) {
	var teamID, onlyWorker string
	role := "lead"
	err := tx.QueryRowContext(ctx, `SELECT id FROM hub_teams WHERE lead=? ORDER BY rowid DESC LIMIT 1`, id).Scan(&teamID)
	if errors.Is(err, sql.ErrNoRows) {
		role = "worker"
		err = tx.QueryRowContext(ctx, `SELECT team,id FROM hub_workers WHERE session=?`, id).Scan(&teamID, &onlyWorker)
	}
	if errors.Is(err, sql.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	var lead string
	if err = tx.QueryRowContext(ctx, `SELECT lead FROM hub_teams WHERE id=?`, teamID).Scan(&lead); err != nil {
		return nil, err
	}
	team, err := teamDetail(ctx, tx, lead, teamID)
	if err != nil {
		return nil, err
	}
	value := &hub.TeamSnapshot{Role: role, TeamID: teamID, LeadSessionID: lead, Status: team.Team.Status, CapturedAt: now(), Processing: "Committed product metadata snapshot only; independently owned Workers are not cancelled, resumed or replayed by switching this engine. Task IDs/sequences identify bounded explicit retrieval, not transferred native-log permission. No Worker messages, task bodies, changed files or verification inferred from prose."}
	for _, worker := range team.Workers {
		if onlyWorker != "" && worker.ID != onlyWorker {
			continue
		}
		product, err := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, worker.SessionID)
		if err != nil {
			return nil, err
		}
		branch, err := read[hub.Branch](ctx, tx, `SELECT data FROM hub_branches WHERE id=? AND session=?`, product.ActiveBranchID, product.ID)
		if err != nil {
			return nil, err
		}
		entry := hub.TeamWorkerSnapshot{WorkerID: worker.ID, SessionID: product.ID, Lifecycle: worker.Lifecycle, BranchID: branch.ID, EngineID: branch.Target.EngineID, Status: product.Status}
		if err = tx.QueryRowContext(ctx, `SELECT coalesce(sum(status='queued'),0),coalesce(sum(status='running'),0) FROM hub_worker_tasks WHERE worker=?`, worker.ID).Scan(&entry.Queued, &entry.Running); err != nil {
			return nil, err
		}
		entry.Head, err = teamTaskIndex(ctx, tx, worker.ID, false)
		if err != nil {
			return nil, err
		}
		entry.LastOutcome, err = teamTaskIndex(ctx, tx, worker.ID, true)
		if err != nil {
			return nil, err
		}
		value.Workers = append(value.Workers, entry)
	}
	if onlyWorker != "" && len(value.Workers) != 1 {
		return nil, errors.New("Worker handoff membership is corrupt")
	}
	return value, nil
}

// beginTurn builds the handoff before ClaimWorkerTask links its real Turn.
// Refresh inside that SAME uncommitted transaction so the delivered and saved
// snapshot both describe the task actually admitted, not its previous queue state.
func refreshWorkerHandoff(ctx context.Context, tx *sql.Tx, claim *hub.Claim) error {
	if claim.Handoff == nil || claim.Handoff.Team == nil || claim.Handoff.Team.Role != "worker" {
		return nil
	}
	value, err := teamSnapshot(ctx, tx, claim.Session.ID)
	if err != nil {
		return err
	}
	claim.Handoff.Team = value
	claim.Handoff, err = protectHandoff(claim.Handoff)
	if err != nil {
		return err
	}
	data, err := body(claim.Handoff)
	if err != nil {
		return err
	}
	result, err := tx.ExecContext(ctx, `UPDATE hub_handoffs SET data=? WHERE id=? AND session=?`, data, claim.Handoff.ID, claim.Session.ID)
	if err != nil {
		return err
	}
	rows, err := result.RowsAffected()
	if err != nil {
		return err
	}
	if rows != 1 {
		return errors.New("admitted Worker handoff snapshot disappeared")
	}
	return nil
}
