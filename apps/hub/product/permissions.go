// INPUT: Explicit native user policy choice and the shared session admission owner.
// OUTPUT: Validated next-Turn policy changes frozen while execution or a native checkpoint remains.
// POS: [L7 product service] One policy gate shared with Plan; no SDK mode mutation or execution.
package product

import (
	"context"
	"errors"

	"github.com/QuincySx/Alva/apps/hub"
)

func (s *HubService) SetPermissionPolicy(ctx context.Context, id string, policy hub.PermissionPolicy, confirmed bool) error {
	if err := policy.Validate(); err != nil {
		return err
	}
	if policy.Effective() == hub.PermissionFull && !confirmed {
		return errors.New("explicit Full scope confirmation required")
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if err := s.validateNativePolicyChangeLocked(ctx, id); err != nil {
		return err
	}
	return s.Store.SetPermissionPolicy(ctx, id, policy, confirmed)
}
