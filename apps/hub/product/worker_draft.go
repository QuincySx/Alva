// INPUT: Actual Lead/Team/Worker, explicit file/Skill/reference selections and original user task
// OUTPUT: Atomic branch-pinned FIFO task with exact logical-owner-checked frozen composer inputs
// POS: [L7 Hub product] Enqueue never executes or borrows Lead/sibling originals; failed admission preserves the exact draft.
package product

import (
	"context"
	"errors"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

func (s *HubService) EnqueueWorkerDraft(ctx context.Context, lead, team, worker, prompt string, claims []hub.ResourceClaim, references []DraftReferenceSelection) (hub.WorkerTask, error) {
	return s.enqueueWorkerDraft(ctx, lead, team, worker, prompt, claims, nil, references)
}
func (s *HubService) enqueueWorkerDraft(ctx context.Context, lead, team, worker, prompt string, claims []hub.ResourceClaim, selections []ComposerPreparation, references []DraftReferenceSelection) (hub.WorkerTask, error) {
	return s.EnqueueComposerWorkerFileDraft(ctx, lead, team, worker, prompt, claims, selections, references, nil)
}

func (s *HubService) EnqueueComposerWorkerFileDraft(ctx context.Context, lead, team, worker, prompt string, claims []hub.ResourceClaim, selections []ComposerPreparation, references []DraftReferenceSelection, files []ComposerFileSelection) (hub.WorkerTask, error) {
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
		selections, err = s.rebindComposerSkillsLocked(ctx, w.SessionID, selections)
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
		userText := prompt
		prompt, inputFiles, err := s.prepareComposerFileDraft(ctx, w.SessionID, prompt, references, files)
		if err != nil {
			return hub.WorkerTask{}, err
		}
		prompt, err = s.composerGoalPrompt(ctx, w.SessionID, selections, prompt)
		if err != nil {
			return hub.WorkerTask{}, err
		}
		if err = s.validatePlanTarget(ctx, w.SessionID, branch); err != nil {
			return hub.WorkerTask{}, err
		}
		if err = s.ValidateTarget(branch.Target); err != nil {
			return hub.WorkerTask{}, err
		}
		inputSkills, err := s.composerSkillInputs(ctx, current, selections)
		if err != nil {
			return hub.WorkerTask{}, err
		}
		if len(inputSkills) > 0 {
			descriptor, ok := s.Runtime.Descriptor(branch.Target.EngineID)
			if !ok || descriptor.Runtime == nil || !descriptor.Runtime.Capabilities().SkillInputs {
				return hub.WorkerTask{}, engine.ErrUnsupported
			}
		}
		prompt, err = s.draftPromptLocked(w.SessionID, branch, current.Session.Workspace, prompt, references)
		if err != nil {
			return hub.WorkerTask{}, err
		}
		if err = ctx.Err(); err != nil {
			return hub.WorkerTask{}, err
		}
		var task hub.WorkerTask
		if len(inputFiles) == 0 && len(inputSkills) == 0 {
			task, err = store.EnqueueWorkerOnBranch(ctx, lead, team, w.ID, branch.ID, prompt, claims)
		} else {
			typed, ok := s.Store.(hub.ComposerWorkerDraftStore)
			if !ok {
				return hub.WorkerTask{}, engine.ErrUnsupported
			}
			task, err = typed.EnqueueComposerWorkerOnBranch(ctx, lead, team, w.ID, branch.ID, hub.ComposerTurnInput{Prompt: prompt, UserText: userText, Files: inputFiles, Skills: inputSkills}, claims)
		}
		if err == nil {
			s.consumeDraftReferences(w.SessionID, references)
		}
		return task, err
	}
	return hub.WorkerTask{}, errors.New("Worker is not owned by this Lead/Team")
}
