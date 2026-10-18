// INPUT: Actual Lead/Team/Worker, explicit host-approved Git manager and owner-scoped durable store
// OUTPUT: Runtime-owned isolated provisioning, exact diff pages and two-workspace confirmed literal adoption
// POS: [L7 Hub product] Shared native/HTTP product operations; no arbitrary client path/native ID, model delivery or automatic rollback
package product

import (
	"context"
	"errors"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
)

type WorktreePatchPage struct {
	Text       string `json:"text"`
	Next       int    `json:"next"`
	HasMore    bool   `json:"has_more"`
	Processing string `json:"processing"`
}

func (s *HubService) worktree(ctx context.Context, lead, team, worker string) (hub.Worker, hub.WorktreeStore, error) {
	if s.Worktrees == nil || s.Teams == nil {
		return hub.Worker{}, nil, errors.New("managed worktrees require explicit operator Git executable and host approval")
	}
	store, ok := s.Store.(hub.WorktreeStore)
	if !ok {
		return hub.Worker{}, nil, errors.New("durable worktree product store unavailable")
	}
	detail, err := s.Teams.Store.Team(ctx, lead, team)
	if err != nil {
		return hub.Worker{}, nil, err
	}
	for _, w := range detail.Workers {
		if w.ID == worker {
			return w, store, nil
		}
	}
	return hub.Worker{}, nil, errors.New("Worker is not owned by this Lead/Team")
}
func (s *HubService) ProvisionWorkerWorktree(ctx context.Context, lead, team, worker string, approved bool) (result hub.Worker, err error) {
	if !approved {
		return result, errors.New("explicit new private worktree/Git host access confirmation required")
	}
	_, store, err := s.worktree(ctx, lead, team, worker)
	if err != nil {
		return result, err
	}
	detail, err := s.Store.Detail(ctx, lead)
	if err != nil {
		return result, err
	}
	err = s.Runtime.WithExclusiveWorkspaces(ctx, []string{detail.Session.Workspace}, func(owned context.Context) error {
		w, err := store.BeginWorkerWorktree(owned, lead, team, worker)
		if err != nil {
			return err
		}
		binding, createErr := s.Worktrees.Create(owned, w.Worktree.Binding.Workspace, w.ID)
		problem := ""
		if createErr != nil {
			problem = "failed"
		}
		finish, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		result, err = store.FinishWorkerWorktree(finish, lead, team, worker, w.Worktree.Revision, binding, problem)
		return errors.Join(createErr, err)
	})
	return result, err
}
func (s *HubService) PreviewWorkerChanges(ctx context.Context, lead, team, worker string) (result hub.Worker, err error) {
	w, store, err := s.worktree(ctx, lead, team, worker)
	if err != nil {
		return result, err
	}
	if w.Worktree == nil || w.Worktree.State != "ready" {
		return result, errors.New("ready isolated Worker required")
	}
	bind := w.Worktree.Binding
	err = s.Runtime.WithExclusiveWorkspaces(ctx, []string{bind.Workspace, bind.Path}, func(owned context.Context) error {
		evidence, err := s.Worktrees.Preview(owned, bind)
		if err != nil {
			return err
		}
		result, err = store.SaveWorkerEvidence(owned, lead, team, worker, w.Worktree.Revision, evidence)
		return err
	})
	return result, err
}
func (s *HubService) WorkerPatch(ctx context.Context, lead, team, worker, evidenceID string, offset int) (WorktreePatchPage, error) {
	w, _, err := s.worktree(ctx, lead, team, worker)
	if err != nil {
		return WorktreePatchPage{}, err
	}
	if w.Worktree == nil || w.Worktree.Evidence == nil || w.Worktree.Evidence.ID != evidenceID {
		return WorktreePatchPage{}, errors.New("exact saved Worker evidence required")
	}
	data, more, err := s.Worktrees.ReadPatch(ctx, *w.Worktree.Evidence, offset, 8<<10)
	return WorktreePatchPage{Text: string(data), Next: offset + len(data), HasMore: more, Processing: "Explicit raw private patch page; may contain project secrets. Not model context, test success, adoption or a commit."}, err
}
func (s *HubService) AdoptWorkerChanges(ctx context.Context, lead, team, worker, evidenceID string, revision uint64, approved bool) (result hub.Worker, err error) {
	if !approved {
		return result, errors.New("explicit reviewed path/SHA evidence approval required; adoption changes the shared workspace")
	}
	w, store, err := s.worktree(ctx, lead, team, worker)
	if err != nil {
		return result, err
	}
	if w.Worktree == nil || w.Worktree.Evidence == nil || w.Worktree.Evidence.ID != evidenceID || w.Worktree.Revision != revision {
		return result, errors.New("exact saved evidence and revision required")
	}
	bind := w.Worktree.Binding
	err = s.Runtime.WithExclusiveWorkspaces(ctx, []string{bind.Workspace, bind.Path}, func(owned context.Context) error {
		admitted, err := store.BeginWorkerAdoption(owned, lead, team, worker, revision, evidenceID)
		if err != nil {
			return err
		}
		applyErr := s.Worktrees.Apply(owned, bind, *admitted.Worktree.Evidence)
		problem := ""
		if applyErr != nil {
			problem = "unknown"
		}
		finish, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		result, err = store.FinishWorkerAdoption(finish, lead, team, worker, admitted.Worktree.Revision, problem)
		return errors.Join(applyErr, err)
	})
	return result, err
}

func (s *HubService) ReviewWorkerRemoval(ctx context.Context, lead, team, worker string) (result hub.Worker, err error) {
	w, store, err := s.worktree(ctx, lead, team, worker)
	if err != nil {
		return result, err
	}
	if w.Worktree == nil || w.Worktree.State != "ready" {
		return result, errors.New("ready isolated Worker required for removal review")
	}
	bind := w.Worktree.Binding
	err = s.Runtime.WithExclusiveWorkspaces(ctx, []string{bind.Workspace, bind.Path}, func(owned context.Context) error {
		review, err := s.Worktrees.ReviewRemoval(owned, bind)
		if err != nil {
			return err
		}
		result, err = store.SaveWorkerRemovalReview(owned, lead, team, worker, w.Worktree.Revision, review)
		return err
	})
	return result, err
}

func (s *HubService) RemoveWorkerWorktree(ctx context.Context, lead, team, worker string, revision uint64, snapshot string, approved bool) (result hub.Worker, err error) {
	if !approved {
		return result, errors.New("explicit confirmation required: remove all reviewed managed Worker files, including unadopted changes, and retire this Worker")
	}
	w, store, err := s.worktree(ctx, lead, team, worker)
	if err != nil {
		return result, err
	}
	if w.Worktree == nil || w.Worktree.Removal == nil || w.Worktree.Revision != revision || w.Worktree.Removal.WorkerSHA256 != snapshot {
		return result, errors.New("exact saved removal review/revision required")
	}
	bind := w.Worktree.Binding
	err = s.Runtime.WithExclusiveWorkspaces(ctx, []string{bind.Workspace, bind.Path}, func(owned context.Context) error {
		admitted, err := store.BeginWorkerRemoval(owned, lead, team, worker, revision, snapshot)
		if err != nil {
			return err
		}
		removeErr := s.Worktrees.Remove(owned, bind, *admitted.Worktree.Removal)
		finish, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		result, err = store.FinishWorkerRemoval(finish, lead, team, worker, admitted.Worktree.Revision, removeErr == nil)
		return errors.Join(removeErr, err)
	})
	return result, err
}
