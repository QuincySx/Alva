// INPUT: Frozen native next-Turn settings and the real host/durable approval owner.
// OUTPUT: Product Ask/Auto/Full enforcement within the unchanged SDK Guard path boundary.
// POS: [L7 native composition] Auto recognizes native file tools only; no SDK policy or implicit plugin grants.
package bootstrap

import (
	"context"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

func nativePermissionManager(settings hub.Settings, fallback security.ApprovalHandler) *security.PermissionManager {
	if settings.PlanMode {
		return security.NewPermissionManager(security.Plan, fallback)
	}
	policy := settings.PermissionPolicy.Effective()
	approval := security.ApprovalFunc(func(ctx context.Context, request security.ApprovalRequest) (security.ApprovalDecision, error) {
		if err := ctx.Err(); err != nil {
			return security.ApprovalDecision{}, err
		}
		auto := false
		if policy == hub.PermissionAuto && request.Effect.Kind == abi.EffectWrite && len(request.Effect.Resources) != 0 {
			switch request.ToolName {
			case "write_file", "edit_file":
				auto = true
			}
		}
		// Guard checks actual resolved paths before calling this handler. Full
		// does not alter those roots or enable separately approved integrations.
		if auto || policy == hub.PermissionFull {
			return security.ApprovalDecision{Allowed: true, Scope: security.Once}, nil
		}
		if fallback == nil {
			return security.ApprovalDecision{}, security.ErrApprovalRequired
		}
		return fallback.Approve(ctx, request)
	})
	return security.NewPermissionManager(security.Ask, approval)
}
