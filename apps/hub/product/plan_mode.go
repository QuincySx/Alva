// INPUT: Explicit saved-session next-turn planning choice and the actual product execution owner.
// OUTPUT: SetPlanMode persists only a native Alva planning choice without starting or changing an execution.
// POS: [L7 product] Trusted user configuration; native Guard enforcement belongs to bootstrap, never to a model mode tool.
package product

import (
	"context"
	"errors"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/harness/durable"
	"github.com/QuincySx/Alva/integration/engine"
)

// SetPlanMode applies to the next admitted Turn. A running or waiting recipe is
// immutable; disabling returns to Ask and does not approve an operation.
func (s *HubService) SetPlanMode(ctx context.Context, id string, enabled bool) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.setPlanModeLocked(ctx, id, enabled)
}

func (s *HubService) setPlanModeLocked(ctx context.Context, id string, enabled bool) error {
	detail, err := s.Store.Detail(ctx, id)
	if err != nil {
		return err
	}
	branch, err := ActiveBranch(detail)
	if err != nil {
		return err
	}
	if branch.Target.EngineID != "alva" {
		return engine.ErrUnsupported
	}
	if hub.BlocksInput(detail.Session.Status) || s.Runtime.SessionActive(id) {
		return errors.New("finish the current Turn or review and abandon its wait before changing plan mode")
	}
	view, _, _, err := s.waitView(ctx, id)
	if err != nil {
		return err
	}
	if view.Snapshot.State.Status != durable.Idle {
		return errors.New("review and abandon the existing native checkpoint before changing plan mode")
	}
	return s.Store.SetPlanMode(ctx, id, enabled)
}
