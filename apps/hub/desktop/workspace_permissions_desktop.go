// INPUT: Page-local home policy intent and committed native session settings.
// OUTPUT: Real Ask/Auto/Full menu, explicit Full scope confirmation and successful-save-only UI updates.
// POS: [L7 Hub UI] Shared product service owns policy persistence and next-Turn enforcement.
//go:build desktop

package hubdesktop

import (
	"github.com/QuincySx/Alva/apps/hub"
	"github.com/egoist/mygo/ui"
)

func (p *NativePage) workspacePermissionPolicy() hub.PermissionPolicy {
	if p.selected == "" && p.homePermissionScope != p.workspacePermissionScope() {
		return hub.PermissionAsk
	}
	return p.permissionPolicies[p.workspacePlanKey()].Effective()
}

func (p *NativePage) workspacePermissionScope() string {
	return p.workspace + "\x00" + hub.RouteKey(p.workspaceComposerTarget())
}

func (p *NativePage) acceptWorkspaceHomePermission(policy hub.PermissionPolicy) {
	if p.permissionPolicies == nil {
		p.permissionPolicies = make(map[string]hub.PermissionPolicy)
	}
	p.permissionPolicies[""] = policy.Effective()
	p.homePermissionScope = p.workspacePermissionScope()
}

func (p *NativePage) acceptWorkspacePermissionSettings(id, branchID string, policy hub.PermissionPolicy) {
	if p.busy || p.selected != id || p.detail.Session.ID != id || p.detail.Session.ActiveBranchID != branchID {
		return
	}
	if p.permissionPolicies == nil {
		p.permissionPolicies = make(map[string]hub.PermissionPolicy)
	}
	p.permissionPolicies[workspacePlanCacheKey(id, branchID)] = policy.Effective()
}

func (p *NativePage) workspacePermissionDescription(engineID string) (string, string) {
	if engineID != "alva" {
		return workspacePermissionPolicy(engineID)
	}
	switch p.workspacePermissionPolicy() {
	case hub.PermissionAuto:
		return "自动审批", "工作区文件写入自动批准；命令、网络、MCP 和未知效果仍逐次询问。仅影响下一轮。"
	case hub.PermissionFull:
		return "完全访问", "已启用能力在现有工作区范围内自动批准；敏感路径、越界限制及独立宿主批准仍有效。仅影响下一轮。"
	default:
		return workspacePermissionPolicy(engineID)
	}
}

func (p *NativePage) setWorkspacePermission(policy hub.PermissionPolicy, confirmed bool) {
	if !p.workspacePlanCanChange() || policy.Validate() != nil || policy == hub.PermissionFull && !confirmed {
		return
	}
	p.composer.permissionOpen, p.composer.confirmFull = false, false
	if p.selected == "" {
		p.acceptWorkspaceHomePermission(policy)
		if p.planModes != nil {
			p.planModes[""] = false
		}
		return
	}
	id, branchID := p.selected, p.detail.Session.ActiveBranchID
	p.generation++
	p.busy = true
	go func() {
		err := p.Service.SetPermissionPolicy(p.ctx, id, policy, confirmed)
		p.dispatch(func() {
			p.busy = false
			if p.selected != id || p.detail.Session.ActiveBranchID != branchID {
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.acceptWorkspacePlanSettings(id, branchID, false)
			p.acceptWorkspacePermissionSettings(id, branchID, policy)
			p.problem = ""
		})
	}()
}

func (p *NativePage) buildNativePermissionMenu(c *ui.Context, anchor ui.Element) {
	workspaceComposerPopover(c, anchor, &p.composer.permissionOpen, 320, func() {
		if p.composer.confirmFull {
			ui.Text(c, "允许已启用能力在当前工作区内自动执行？敏感路径、越界限制和独立宿主批准仍有效。选择只保存下一轮策略。").Label("Full permission scope").FontSize(14).LineHeight(1.5).Padding(8, 12)
			ui.Row(c).FillWidth().Gap(8).Justify(ui.End).Children(func() {
				ui.Button(c, "取消").Label("Cancel Full permission").OnClick(func() { p.composer.confirmFull = false })
				ui.Button(c, "确认完全访问").Label("Confirm Full permission").Disabled(!p.workspacePlanCanChange()).OnClick(func() { p.setWorkspacePermission(hub.PermissionFull, true) })
			})
			return
		}
		ui.Column(c).FillWidth().Gap(4).FocusGroup(ui.Vertical).Children(func() {
			for _, choice := range []struct {
				policy           hub.PermissionPolicy
				text, icon, hint string
			}{
				{hub.PermissionAsk, "每次询问", "shield-check", "读取无需确认；其他操作逐次询问。"},
				{hub.PermissionAuto, "自动审批", "sparkles", "原生工作区文件写入自动批准；命令、网络、MCP 和未知效果仍询问。"},
				{hub.PermissionFull, "完全访问", "triangle-alert", "确认范围后，已启用能力自动批准；不会放宽工作区或启用插件。"},
			} {
				selected := !p.workspacePlanEnabled() && p.workspacePermissionPolicy() == choice.policy
				item := ui.ButtonBase(c.Key(string(choice.policy))).Label(choice.text).Role(ui.RoleMenuItemRadio).Checked(selected).
					FillWidth().Padding(8, 12).Gap(8).AlignItems(ui.Center).Radius(8).Justify(ui.Start).
					Tooltip(choice.hint).Disabled(!p.workspacePlanCanChange()).OnClick(func() {
					if choice.policy == hub.PermissionFull {
						p.composer.confirmFull = true
					} else {
						p.setWorkspacePermission(choice.policy, false)
					}
				})
				if item.Hovered() {
					item.Background(workspacePalette(c).hover)
				}
				item.Children(func() {
					ui.Icon(c, workspaceIcon(choice.icon)).Size(16, 16)
					ui.Text(c, choice.text).FontSize(14).LineHeight(1.5).Grow(1)
					if selected {
						ui.Icon(c, workspaceIcon("check")).Size(16, 16).TextColor(c.Theme().Accent)
					}
				})
			}
		})
	})
}
