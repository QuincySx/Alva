// INPUT: Claimed Lead/Team/Worker, idle session facts and trusted managed binding/hash evidence
// OUTPUT: Durable one-way provisioning, optimistic evidence revision and explicit adoption/unknown transitions
// POS: [L7 Hub product] Product facts only; no Git execution, automatic retry, filesystem rollback or native history rewrite
package sqlite

import (
	"context"
	"database/sql"
	"encoding/hex"
	"errors"
	"path/filepath"

	"github.com/QuincySx/Alva/go/apps/hub"
)

func worktreeWorker(ctx context.Context, tx *sql.Tx, lead, team, worker string) (hub.Worker, hub.Session, hub.Session, error) {
	t, err := teamRecord(ctx, tx, lead, team)
	if err != nil {
		return hub.Worker{}, hub.Session{}, hub.Session{}, err
	}
	if t.Status != "active" {
		return hub.Worker{}, hub.Session{}, hub.Session{}, errors.New("active Team required")
	}
	w, err := workerRecord(ctx, tx, team, worker)
	if err != nil {
		return w, hub.Session{}, hub.Session{}, err
	}
	parent, err := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, lead)
	if err != nil {
		return w, parent, hub.Session{}, err
	}
	child, err := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, w.SessionID)
	if err != nil {
		return w, parent, child, err
	}
	if parent.Status == hub.Running || child.Status == hub.Running {
		return w, parent, child, errors.New("finish or Stop actual Lead and Worker execution before worktree changes")
	}
	return w, parent, child, nil
}
func (s *Store) BeginWorkerWorktree(ctx context.Context, lead, team, worker string) (hub.Worker, error) {
	var w hub.Worker
	err := s.transaction(ctx, func(tx *sql.Tx) error {
		var parent, child hub.Session
		var err error
		w, parent, child, err = worktreeWorker(ctx, tx, lead, team, worker)
		if err != nil {
			return err
		}
		if w.Worktree != nil {
			return errors.New("Worker already has a worktree decision; failed/unknown provisioning is not replayed")
		}
		var count int
		if err = tx.QueryRowContext(ctx, `SELECT count(*) FROM hub_turns WHERE session=?`, child.ID).Scan(&count); err != nil {
			return err
		}
		if count != 0 {
			return errors.New("isolation must be selected before this Worker's first Turn; existing native cwd/history is immutable")
		}
		if err = tx.QueryRowContext(ctx, `SELECT count(*) FROM hub_branches WHERE session=? AND coalesce(json_extract(data,'$.native_session_id'),'')<>''`, child.ID).Scan(&count); err != nil {
			return err
		}
		if count != 0 {
			return errors.New("Worker already has native identity; cannot retarget its workspace")
		}
		w.Worktree = &hub.WorkerWorktree{State: "provisioning", Binding: hub.WorktreeBinding{WorkerID: w.ID, Workspace: parent.Workspace}, Revision: 1}
		return writeWorker(ctx, tx, w)
	})
	return w, err
}
func (s *Store) FinishWorkerWorktree(ctx context.Context, lead, team, worker string, revision uint64, binding hub.WorktreeBinding, problem string) (hub.Worker, error) {
	var w hub.Worker
	err := s.transaction(ctx, func(tx *sql.Tx) error {
		var parent, child hub.Session
		var err error
		w, parent, child, err = worktreeWorker(ctx, tx, lead, team, worker)
		if err != nil {
			return err
		}
		if w.Worktree == nil || w.Worktree.State != "provisioning" || w.Worktree.Revision != revision {
			return errors.New("worktree provisioning revision changed")
		}
		if binding.WorkerID != "" && (binding.WorkerID != w.ID || binding.Workspace != parent.Workspace) {
			return errors.New("managed worktree belongs to a different Worker/workspace")
		}
		w.Worktree.Revision++
		w.Worktree.Problem = boundedWorktreeProblem(problem)
		if problem != "" {
			w.Worktree.State = "provision_failed"
			if binding.WorkerID != "" {
				w.Worktree.Binding = binding
			}
		} else {
			if binding.Version != 1 || !filepath.IsAbs(binding.Path) || binding.Path == parent.Workspace || binding.BaseCommit == "" || binding.Branch == "" {
				return errors.New("actual managed worktree identity required")
			}
			w.Worktree.State = "ready"
			w.Worktree.Binding = binding
			child.Workspace = binding.Path
			child.UpdatedAt = now()
			if err = writeSession(ctx, tx, child); err != nil {
				return err
			}
		}
		if err = writeWorker(ctx, tx, w); err != nil {
			return err
		}
		return timeline(ctx, tx, lead, "", "", hub.TimelineEvent{Kind: "worker_worktree", Text: w.ID + ": " + w.Worktree.State})
	})
	return w, err
}
func boundedWorktreeProblem(problem string) string {
	if problem != "" {
		return "Managed worktree operation did not prove success. Inspect private target/evidence; no automatic retry, rollback or replay."
	}
	return ""
}
func (s *Store) SaveWorkerEvidence(ctx context.Context, lead, team, worker string, revision uint64, evidence hub.WorktreeEvidence) (hub.Worker, error) {
	var w hub.Worker
	err := s.transaction(ctx, func(tx *sql.Tx) error {
		var err error
		w, _, _, err = worktreeWorker(ctx, tx, lead, team, worker)
		if err != nil {
			return err
		}
		if w.Worktree == nil || w.Worktree.State != "ready" || w.Worktree.Revision != revision || w.Worktree.Adoption != "" {
			return errors.New("worktree revision/adoption changed; explicit evidence refresh unavailable")
		}
		if evidence.WorkerID != w.ID || evidence.BaseCommit != w.Worktree.Binding.BaseCommit || evidence.ID == "" || len(evidence.Changes) == 0 || len(evidence.Changes) > 128 {
			return errors.New("bounded evidence must belong to this managed Worker")
		}
		w.Worktree.Evidence = &evidence
		w.Worktree.Revision++
		w.Worktree.Problem = ""
		return writeWorker(ctx, tx, w)
	})
	return w, err
}
func (s *Store) BeginWorkerAdoption(ctx context.Context, lead, team, worker string, revision uint64, evidenceID string) (hub.Worker, error) {
	var w hub.Worker
	err := s.transaction(ctx, func(tx *sql.Tx) error {
		var err error
		w, _, _, err = worktreeWorker(ctx, tx, lead, team, worker)
		if err != nil {
			return err
		}
		if w.Worktree == nil || w.Worktree.State != "ready" || w.Worktree.Revision != revision || w.Worktree.Adoption != "" || w.Worktree.Evidence == nil || w.Worktree.Evidence.ID != evidenceID {
			return errors.New("exact reviewed evidence/revision required; adoption is never replayed")
		}
		w.Worktree.Adoption = "adopting"
		w.Worktree.Revision++
		if err = writeWorker(ctx, tx, w); err != nil {
			return err
		}
		return timeline(ctx, tx, lead, "", "", hub.TimelineEvent{Kind: "worker_adoption", Text: w.ID + ": adopting reviewed evidence " + evidenceID})
	})
	return w, err
}
func (s *Store) FinishWorkerAdoption(ctx context.Context, lead, team, worker string, revision uint64, problem string) (hub.Worker, error) {
	var w hub.Worker
	err := s.transaction(ctx, func(tx *sql.Tx) error {
		if _, err := teamRecord(ctx, tx, lead, team); err != nil {
			return err
		}
		var err error
		w, err = workerRecord(ctx, tx, team, worker)
		if err != nil {
			return err
		}
		if w.Worktree == nil || w.Worktree.Revision != revision || w.Worktree.Adoption != "adopting" {
			return errors.New("adoption revision changed")
		}
		w.Worktree.Revision++
		w.Worktree.Adoption = "applied"
		w.Worktree.Problem = boundedWorktreeProblem(problem)
		if problem != "" {
			w.Worktree.Adoption = "unknown"
		}
		if err = writeWorker(ctx, tx, w); err != nil {
			return err
		}
		return timeline(ctx, tx, lead, "", "", hub.TimelineEvent{Kind: "worker_adoption", Text: w.ID + ": " + w.Worktree.Adoption + "; no commit/staging/test success implied"})
	})
	return w, err
}
func checkWorkerWorkspaceAdmission(ctx context.Context, tx *sql.Tx, id string) error {
	w, err := read[hub.Worker](ctx, tx, `SELECT data FROM hub_workers WHERE session=?`, id)
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return err
	}
	if err == nil && w.Worktree != nil && (w.Worktree.State != "ready" || w.Worktree.Adoption == "adopting" || w.Worktree.Adoption == "unknown") {
		return errors.New("Worker worktree is not ready or adoption outcome is unknown; inspect, do not replay")
	}
	var adopting int
	if err = tx.QueryRowContext(ctx, `SELECT count(*) FROM hub_workers WHERE json_extract(data,'$.worktree.adoption')='adopting' AND json_extract(data,'$.worktree.binding.shared_workspace')=(SELECT json_extract(data,'$.workspace') FROM hub_sessions WHERE id=?)`, id).Scan(&adopting); err != nil {
		return err
	}
	if adopting != 0 {
		return errors.New("workspace is in explicitly owned adoption; wait for its actual result")
	}
	return nil
}
func (s *Store) RecoverWorktrees(ctx context.Context) error {
	return s.transaction(ctx, func(tx *sql.Tx) error {
		if _, err := tx.ExecContext(ctx, `UPDATE hub_workers SET data=json_set(data,'$.worktree.state','removal_unknown','$.worktree.problem','Application exited during explicitly approved worktree removal; inspect retained branch/history/evidence, no automatic retry.','$.worktree.revision',coalesce(json_extract(data,'$.worktree.revision'),0)+1) WHERE json_extract(data,'$.worktree.state')='removing'`); err != nil {
			return err
		}
		_, err := tx.ExecContext(ctx, `UPDATE hub_workers SET data=json_set(data,'$.worktree.state',CASE WHEN json_extract(data,'$.worktree.state')='provisioning' THEN 'provision_unknown' ELSE json_extract(data,'$.worktree.state') END,'$.worktree.adoption',CASE WHEN json_extract(data,'$.worktree.adoption')='adopting' THEN 'unknown' ELSE coalesce(json_extract(data,'$.worktree.adoption'),'') END,'$.worktree.problem','Application exited during worktree mutation; outcome unknown, no automatic replay or rollback.','$.worktree.revision',coalesce(json_extract(data,'$.worktree.revision'),0)+1) WHERE json_extract(data,'$.worktree.state')='provisioning' OR json_extract(data,'$.worktree.adoption')='adopting'`)
		return err
	})
}

func validRemovalReview(review hub.WorktreeRemovalReview) bool {
	for _, digest := range []string{review.WorkerSHA256, review.IndexSHA256} {
		decoded, err := hex.DecodeString(digest)
		if err != nil || len(decoded) != 32 {
			return false
		}
	}
	decoded, err := hex.DecodeString(review.Head)
	return err == nil && (len(decoded) == 20 || len(decoded) == 32) && review.Files >= 0 && review.Files <= 4096 && review.Bytes >= 0 && review.Bytes <= 128<<20 && review.CreatedAt > 0
}
func (s *Store) SaveWorkerRemovalReview(ctx context.Context, lead, team, worker string, revision uint64, review hub.WorktreeRemovalReview) (hub.Worker, error) {
	var w hub.Worker
	err := s.transaction(ctx, func(tx *sql.Tx) error {
		var err error
		w, _, _, err = worktreeWorker(ctx, tx, lead, team, worker)
		if err != nil {
			return err
		}
		if w.Worktree == nil || w.Worktree.State != "ready" || w.Worktree.Revision != revision || w.Worktree.Adoption == "adopting" || w.Worktree.Adoption == "unknown" || !validRemovalReview(review) {
			return errors.New("idle ready Worker, exact revision and complete removal review required")
		}
		w.Worktree.Removal = &review
		w.Worktree.Revision++
		return writeWorker(ctx, tx, w)
	})
	return w, err
}
func (s *Store) BeginWorkerRemoval(ctx context.Context, lead, team, worker string, revision uint64, snapshot string) (hub.Worker, error) {
	var w hub.Worker
	err := s.transaction(ctx, func(tx *sql.Tx) error {
		var err error
		w, _, _, err = worktreeWorker(ctx, tx, lead, team, worker)
		if err != nil {
			return err
		}
		if w.Worktree == nil || w.Worktree.State != "ready" || w.Worktree.Revision != revision || w.Worktree.Removal == nil || !validRemovalReview(*w.Worktree.Removal) || snapshot != w.Worktree.Removal.WorkerSHA256 || w.Worktree.Adoption == "adopting" || w.Worktree.Adoption == "unknown" {
			return errors.New("exact reviewed removal snapshot/revision required; unknown operations are never replayed")
		}
		w.Lifecycle = hub.WorkerArchived
		w.Worktree.State = "removing"
		w.Worktree.Revision++
		if err = writeWorker(ctx, tx, w); err != nil {
			return err
		}
		return timeline(ctx, tx, lead, "", "", hub.TimelineEvent{Kind: "worker_worktree", Text: w.ID + ": explicitly approved removal; Worker retired, native history and evidence retained"})
	})
	return w, err
}
func (s *Store) FinishWorkerRemoval(ctx context.Context, lead, team, worker string, revision uint64, success bool) (hub.Worker, error) {
	var w hub.Worker
	err := s.transaction(ctx, func(tx *sql.Tx) error {
		if _, err := teamRecord(ctx, tx, lead, team); err != nil {
			return err
		}
		var err error
		w, err = workerRecord(ctx, tx, team, worker)
		if err != nil {
			return err
		}
		if w.Worktree == nil || w.Worktree.Revision != revision || w.Worktree.State != "removing" {
			return errors.New("worktree removal revision changed")
		}
		w.Worktree.State = "removed"
		w.Worktree.Problem = ""
		if !success {
			w.Worktree.State = "removal_unknown"
			w.Worktree.Problem = "Removal did not prove success. Inspect private target, Git registration and retained evidence; no automatic retry or native cwd retargeting."
		}
		w.Worktree.Revision++
		if err = writeWorker(ctx, tx, w); err != nil {
			return err
		}
		return timeline(ctx, tx, lead, "", "", hub.TimelineEvent{Kind: "worker_worktree", Text: w.ID + ": " + w.Worktree.State + "; branch/history/evidence retained"})
	})
	return w, err
}

var _ hub.WorktreeStore = (*Store)(nil)
