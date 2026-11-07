// INPUT: Actual Lead/Team/Worker, saved native-branch file/project drafts and explicit user prompt
// OUTPUT: Atomic branch-pinned FIFO task containing only checked file/project metadata and references
// POS: [L7 Hub product] Enqueue never executes or transfers Lead/sibling artifacts; failed admission preserves the exact draft.
package product

import (
	"context"
	"errors"

	"github.com/QuincySx/Alva/apps/hub"
)

func (s *HubService) EnqueueWorkerDraft(ctx context.Context, lead, team, worker, prompt string, claims []hub.ResourceClaim) (hub.WorkerTask, error) {
	return s.enqueueWorkerDraft(ctx, lead, team, worker, prompt, claims, nil)
}
func (s *HubService) enqueueWorkerDraft(ctx context.Context, lead, team, worker, prompt string, claims []hub.ResourceClaim, selections []ComposerPreparation) (hub.WorkerTask, error) {
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
		selections, err = s.rebindComposerGoalsLocked(ctx, w.SessionID, selections)
		if err != nil {
			return hub.WorkerTask{}, err
		}
		// Goal preparation may establish the first native identity. Read the
		// branch again before applying existing attachment ownership checks.
		current, err = s.Store.Detail(ctx, w.SessionID)
		if err != nil {
			return hub.WorkerTask{}, err
		}
		branch, err = ActiveBranch(current)
		if err != nil {
			return hub.WorkerTask{}, err
		}
		if err = s.prepareComposerDraftLocked(ctx, w.SessionID, selections); err != nil {
			return hub.WorkerTask{}, err
		}
		prompt, err = s.composerGoalPrompt(ctx, w.SessionID, selections, prompt)
		if err != nil {
			return hub.WorkerTask{}, err
		}
		if err = s.validatePlanTarget(ctx, w.SessionID, branch); err != nil {
			return hub.WorkerTask{}, err
		}
		prompt, err = s.draftPromptLocked(w.SessionID, branch, current.Session.Workspace, prompt)
		if err != nil {
			return hub.WorkerTask{}, err
		}
		task, err := store.EnqueueWorkerOnBranch(ctx, lead, team, w.ID, branch.ID, prompt, claims)
		if err == nil {
			s.draftMu.Lock()
			delete(s.attachments, w.SessionID)
			delete(s.workspaceReferences, w.SessionID)
			s.draftMu.Unlock()
		}
		return task, err
	}
	return hub.WorkerTask{}, errors.New("Worker is not owned by this Lead/Team")
}
