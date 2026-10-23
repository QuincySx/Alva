// INPUT: An explicit bootstrap recipe, committed connection service and exact saved target.
// OUTPUT: An immutable per-attempt provider recipe, including declared image capability.
// POS: [L7 composition] Resolves current connection credentials without changing a process default or discovering accounts.
package bootstrap

import (
	"context"
	"errors"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
)

func resolveConnectionConfig(ctx context.Context, base Config, service *product.HubService, target hub.Target) (Config, error) {
	if !product.IsConnectionTarget(target) {
		return base, nil
	}
	if service == nil || service.Connections == nil {
		return Config{}, errors.New("model connection service is unavailable")
	}
	kind, config, images, err := service.Connections.ResolveTarget(ctx, target)
	if err != nil {
		return Config{}, err
	}
	base.Demo = false
	base.ProviderKind = kind
	base.Provider = config
	base.ImageCapable = images
	return base, nil
}
