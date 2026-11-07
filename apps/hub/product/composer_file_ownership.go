// INPUT: Actual product session identity, immutable captured-file catalog and trusted persisted adoption grants.
// OUTPUT: Shared session ownership validation for composer admission and native reference construction.
// POS: [L7 Hub product] Scope/URI/SHA DTOs never substitute for a real home-to-session transition or an original session owner.
package product

import (
	"context"
	"errors"

	"github.com/QuincySx/Alva/apps/hub"
)

func (s *HubService) ValidateComposerFileOwner(ctx context.Context, id string, file hub.ComposerFile) error {
	actual, err := s.selectedComposerFile(ctx, SelectedComposerFile(file))
	if err != nil {
		return err
	}
	if id == "" {
		return hub.ErrComposerFileChanged
	}
	if _, err = s.Store.Detail(ctx, id); err != nil {
		return err
	}
	if actual.Scope.Kind == "session" {
		if actual.Scope.ID != id {
			return hub.ErrComposerFileChanged
		}
		return nil
	}
	ownership, ok := s.Store.(hub.ComposerFileOwnershipStore)
	if !ok {
		return errors.New("captured file ownership storage is unavailable")
	}
	allowed, err := ownership.ComposerFileSession(ctx, id, actual.ID, actual.SHA256)
	if err != nil {
		return err
	}
	if !allowed {
		return hub.ErrComposerFileChanged
	}
	return nil
}
