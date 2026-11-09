// INPUT: Frozen native next-Turn settings and the real host/durable approval owner.
// OUTPUT: Product Ask/Auto/Full enforcement within the unchanged SDK Guard path boundary.
// POS: [L7 native composition] Auto judges by Write effect inside Guard-confined resources; billable effects always ask; no tool names.
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
		// Guard checks actual resolved paths before calling this handler, so a
		// Write effect with claimed resources has already been confined to the
		// workspace. Auto trusts that effect, never the tool's name. Full does
		// not alter those roots or enable separately approved integrations, and
		// a tool-declared billable effect is asked at every tier.
		auto := policy == hub.PermissionAuto && request.Effect.Kind == abi.EffectWrite && len(request.Effect.Resources) != 0
		if !request.Effect.Billable && (auto || policy == hub.PermissionFull) {
			return security.ApprovalDecision{Allowed: true, Scope: security.Once}, nil
		}
		if fallback == nil {
			return security.ApprovalDecision{}, security.ErrApprovalRequired
		}
		return fallback.Approve(ctx, request)
	})
	return security.NewPermissionManager(security.Ask, approval)
}
