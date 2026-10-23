// INPUT: Saved branches, configured route snapshots and existing explicit fallback service actions.
// OUTPUT: Reachable advanced route/history selection and account/billing-aware fallback approval or removal.
// POS: [L7 Hub UI] Keeps prior execution controls available while the everyday workspace stays focused.
//go:build desktop

package hubdesktop

import (
	"github.com/egoist/mygo/ui"
)

func (p *NativePage) buildWorkspaceRouting(c *ui.Context) {
	ui.Text(c, "执行路由").FontSize(24).FontWeight(600)
	ui.Scroll(c).Grow(1).FillWidth().Children(func() {
		ui.Column(c).FillWidth().Gap(16).Children(func() {
			ui.Text(c, "会话分支").FontWeight(600)
			for _, branch := range p.detail.Branches {
				branch := branch
				label := "Use " + branch.Target.EngineID + " · " + branch.Target.ModelID
				if branch.ID == p.detail.Session.ActiveBranchID {
					label = "Active: " + branch.Target.EngineID + " · " + branch.Target.ModelID
				}
				ui.Button(c, label).Disabled(p.busy || p.inputBlocked() || branch.ID == p.detail.Session.ActiveBranchID).OnClick(func() { p.switchBranch(branch.Target) })
			}
			ui.Text(c, "故障切换").FontWeight(600)
			ui.Text(c, "仅在明确批准的启动失败或限流条件下，使用下方指定账户与计费路由。可能已经产生副作用时不会自动切换。").TextColor(c.Theme().TextMuted)
			if len(p.detail.Fallback) == 0 {
				ui.Text(c, "未启用自动故障切换").TextColor(c.Theme().TextMuted)
			}
			for _, target := range p.detail.Fallback {
				ui.Text(c, "已批准："+workspaceTargetLabel(target)+" · "+target.AccountSource+" · "+target.BillingRoute)
			}
			for _, target := range p.targets {
				target := target
				ui.Column(c).Gap(8).Children(func() {
					ui.Text(c, workspaceTargetLabel(target)+" · "+target.AccountSource+" · "+target.BillingRoute).FontSize(14)
					ui.Button(c, "Approve fallback: "+target.EngineID+" · "+target.AccountSource+" · "+target.BillingRoute).Disabled(p.busy || p.inputBlocked()).OnClick(func() { p.setFallback(&target) })
				})
			}
			ui.Button(c, "Disable automatic fallback").Disabled(p.busy || p.inputBlocked() || len(p.detail.Fallback) == 0).OnClick(func() { p.setFallback(nil) })
		})
	})
}
