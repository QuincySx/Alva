// INPUT: Actual Lead/Team/Worker, saved native-branch attachment drafts and explicit user prompt
// OUTPUT: Atomic branch-pinned FIFO task containing only attachment metadata/references
// POS: [L7 Hub product] Enqueue never executes or transfers Lead/sibling artifacts; failed admission preserves the exact draft.
package product

import (
	"context"
	"errors"

	"github.com/QuincySx/Alva/go/apps/hub"
)

func (s *HubService) EnqueueWorkerDraft(ctx context.Context, lead, team, worker, prompt string, claims []hub.ResourceClaim) (hub.WorkerTask, error) {
	if s.Teams == nil {
		return hub.WorkerTask{}, errors.New("Worker queue unavailable")
	}
	store, ok := s.Store.(hub.WorkerDraftStore)
	if !ok {
		return hub.WorkerTask{}, errors.New("branch-pinned Worker draft queue unavailable")
	}
	detail, err := s.Teams.Store.Team(ctx, lead, team)
	if err != nil {
		return hub.WorkerTask{}, err
	}
	for _, w := range detail.Workers {
		if w.ID != worker {
			continue
		}
		current, err := s.Store.Detail(ctx, w.SessionID)
		if err != nil {
			return hub.WorkerTask{}, err
		}
		branch, err := ActiveBranch(current)
		if err != nil {
			return hub.WorkerTask{}, err
		}
		s.mu.Lock()
		defer s.mu.Unlock()
		prompt, err = s.draftPromptLocked(w.SessionID, branch, prompt)
		if err != nil {
			return hub.WorkerTask{}, err
		}
		task, err := store.EnqueueWorkerOnBranch(ctx, lead, team, w.ID, branch.ID, prompt, claims)
		if err == nil {
			delete(s.attachments, w.SessionID)
		}
		return task, err
	}
	return hub.WorkerTask{}, errors.New("Worker is not owned by this Lead/Team")
}
