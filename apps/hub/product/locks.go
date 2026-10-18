// INPUT: One SDK ToolLockRegistry owned by product process assembly
// OUTPUT: SharedLocksPlugin installs the same actual lock middleware in all in-product Alva agents
// POS: [L7 Hub product] Product recipe only; SDK retains resource algebra, Hub core remains engine-only
package product

import (
	"context"
	"errors"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/bus"
)

type SharedLocksPlugin struct{ Registry *core.ToolLockRegistry }

func (SharedLocksPlugin) Name() string { return "product-tool-locks" }
func (p SharedLocksPlugin) Register(_ context.Context, r *core.Registrar) error {
	if p.Registry == nil {
		return errors.New("shared product tool locks required")
	}
	if err := bus.Provide(r.BusWriter(), p.Registry); err != nil {
		return err
	}
	return r.Middleware(core.ToolLockMiddleware{Registry: p.Registry})
}
