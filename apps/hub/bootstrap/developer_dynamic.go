// INPUT: Committed connection snapshots and the exact selected developer target identities.
// OUTPUT: Cold revision-bound developer metadata and private immutable provider recipes.
// POS: [L7 composition] Adapts saved Connections to the shared developer owner without constructing another provider.
package bootstrap

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"

	"github.com/QuincySx/Alva/apps/developer"
	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/llm/eval"
)

type connectionDeveloperSource struct {
	connections *product.ConnectionsService
}

func connectionDeveloperTarget(target hub.Target) eval.Target {
	sum := sha256.Sum256([]byte(hub.RouteKey(target)))
	return eval.Target{ID: "connection-" + hex.EncodeToString(sum[:26]), Protocol: target.ProviderID, Model: target.ModelID}
}

func (s connectionDeveloperSource) Targets() []eval.Target {
	var result []eval.Target
	for _, target := range s.connections.Targets() {
		result = append(result, connectionDeveloperTarget(target))
	}
	return result
}

func (s connectionDeveloperSource) Resolve(ctx context.Context, selected []eval.Target) ([]developer.Route, error) {
	current := s.connections.Targets()
	routes := make([]developer.Route, 0, len(selected))
	for _, selection := range selected {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		found := false
		for _, target := range current {
			if connectionDeveloperTarget(target) != selection {
				continue
			}
			if found {
				return nil, errors.New("ambiguous saved developer target")
			}
			kind, config, _, err := s.connections.ResolveTarget(ctx, target)
			if err != nil {
				return nil, err
			}
			routes = append(routes, developer.Route{ID: selection.ID, Protocol: kind, Provider: config})
			found = true
		}
		if !found {
			return nil, errors.New("saved developer target changed or was disabled; select a current route")
		}
	}
	return routes, nil
}
