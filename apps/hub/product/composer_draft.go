// INPUT: Explicit Send with frozen composer preparations and explicit branch-pinned DraftID/version selections.
// OUTPUT: Shared command/Skill revalidation, real next-turn Plan choice and normal Submit/Worker admission.
// POS: [L7 Hub product] Serializes selection validation with product draft admission; no second execution or storage path.
package product

import (
	"context"
	"errors"

	"github.com/QuincySx/Alva/apps/hub"
)

func (s *HubService) SubmitComposerDraft(ctx context.Context, id, prompt string, selections []ComposerPreparation, references []DraftReferenceSelection) (string, error) {
	return s.submitDraft(ctx, id, prompt, selections, references)
}

func (s *HubService) EnqueueComposerWorkerDraft(ctx context.Context, lead, team, worker, prompt string, claims []hub.ResourceClaim, selections []ComposerPreparation, references []DraftReferenceSelection) (hub.WorkerTask, error) {
	return s.enqueueWorkerDraft(ctx, lead, team, worker, prompt, claims, selections, references)
}

// Caller holds the same service mutex used by SetPlanMode and draft admission.
func (s *HubService) prepareComposerDraftLocked(ctx context.Context, id string, selections []ComposerPreparation) error {
	if len(selections) > 8 {
		return errors.New("at most 8 composer selections per draft")
	}
	seen := make(map[string]bool, len(selections))
	plan := false
	for _, selection := range selections {
		if seen[selection.EntryID] {
			return errors.New("duplicate composer selection")
		}
		seen[selection.EntryID] = true
		if err := s.RevalidateComposerPreparation(ctx, id, selection); err != nil {
			return err
		}
		plan = plan || selection.Intent == ComposerIntentPlan
	}
	if plan {
		return s.setPlanModeLocked(ctx, id, true)
	}
	return nil
}
