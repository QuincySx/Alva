// INPUT: Configured route snapshots, committed connections and the existing session store.
// OUTPUT: Shared route validation, explicit target/fallback changes and retained-branch selection.
// POS: [L7 Hub product] Execution routes require current configuration; historical branch navigation never grants execution.
package product

import (
	"context"
	"errors"

	"github.com/QuincySx/Alva/apps/hub"
)

var ErrRouteUnavailable = errors.New("route changed or is unavailable; select a current configured route")

// ValidateConfiguredTarget compares the entire account/billing/capability route.
// A transport may supply its frozen program catalog plus committed connections.
func ValidateConfiguredTarget(target hub.Target, configured []hub.Target) error {
	if target.Validate() != nil {
		return ErrRouteUnavailable
	}
	key := hub.RouteKey(target)
	for _, candidate := range configured {
		// A connection route survives cosmetic edits and endpoint moves, but
		// never a protocol change.
		if hub.RouteKey(candidate) == key && (candidate.ProviderID == target.ProviderID || !IsConnectionTarget(target)) {
			return nil
		}
	}
	return ErrRouteUnavailable
}

func (s *HubService) ValidateTarget(target hub.Target) error {
	return ValidateConfiguredTarget(target, s.AvailableTargets())
}

func (s *HubService) SwitchTarget(ctx context.Context, id string, target hub.Target, reason string) (hub.Branch, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if err := s.ValidateTarget(target); err != nil {
		return hub.Branch{}, err
	}
	return s.Store.Switch(ctx, id, target, reason)
}

// SelectBranch only selects an existing product branch. Its route may no longer
// be configured, so callers can still inspect old history after an account edit.
// Submit independently validates the current execution route.
func (s *HubService) SelectBranch(ctx context.Context, id, branchID string, reason string) (hub.Branch, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	detail, err := s.Store.Detail(ctx, id)
	if err != nil {
		return hub.Branch{}, err
	}
	for _, branch := range detail.Branches {
		if branch.ID == branchID && branch.SessionID == id {
			return s.Store.Switch(ctx, id, branch.Target, reason)
		}
	}
	return hub.Branch{}, hub.ErrNotFound
}

func (s *HubService) SetFallback(ctx context.Context, id string, targets []hub.Target) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if len(targets) > 7 {
		return errors.New("at most seven explicitly approved fallback routes")
	}
	seen := make(map[string]bool, len(targets))
	for _, target := range targets {
		if err := s.ValidateTarget(target); err != nil {
			return err
		}
		key := hub.RouteKey(target)
		if !target.AutomaticAuthorized || seen[key] {
			return errors.New("fallback routes require unique, explicit account and billing approval")
		}
		seen[key] = true
	}
	return s.Store.SetFallback(ctx, id, targets)
}
