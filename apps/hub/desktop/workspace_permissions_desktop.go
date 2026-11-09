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
		return p.workspaceEnginePermission(engineID)
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
	p.busy, p.composer.settingsSaving = true, true
	go func() {
		err := p.Service.SetPermissionPolicy(p.ctx, id, policy, confirmed)
		p.dispatch(func() {
			p.busy, p.composer.settingsSaving = false, false
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
	workspaceComposerPopover(c, anchor, &p.composer.permissionOpen, 240, func() {
		if p.composer.confirmFull {
			ui.Text(c, "允许已启用能力在当前工作区内自动执行？敏感路径、越界限制和独立宿主批准仍有效。选择只保存下一轮策略。").Label("Full permission scope").FontSize(workspaceFontBody).LineHeight(1.5).Padding(8, 12)
			ui.Row(c).FillWidth().Gap(8).Justify(ui.End).Children(func() {
				workspaceControlButton(c, "cancel-full", "取消", "secondary").Label("Cancel Full permission").OnClick(func() { p.composer.confirmFull = false })
				workspaceControlButton(c, "confirm-full", "确认完全访问", "primary", !p.workspacePlanCanChange()).Label("Confirm Full permission").OnClick(func() { p.setWorkspacePermission(hub.PermissionFull, true) })
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
				item := workspaceComposerMenuItem(c.Key(string(choice.policy)), choice.text).Role(ui.RoleMenuItemRadio).Checked(selected).
					Tooltip(choice.hint).Disabled(!p.workspacePlanCanChange()).OnClick(func() {
					if choice.policy == hub.PermissionFull {
						p.composer.confirmFull = true
					} else {
						p.setWorkspacePermission(choice.policy, false)
					}
				})
				iconColor := c.Theme().TextMuted
				if selected {
					item.Background(workspacePalette(c).selection).TextColor(c.Theme().Accent)
					iconColor = c.Theme().Accent
				}
				if !p.workspacePlanCanChange() {
					item.TextColor(c.Theme().TextMuted)
					iconColor = c.Theme().TextMuted
				}
				item.Children(func() {
					ui.Icon(c, workspaceIcon(choice.icon)).Size(16, 16).Shrink(0).TextColor(iconColor)
					ui.Text(c, choice.text).FontSize(workspaceFontBody).LineHeight(1.5).SingleLine().Grow(1).MinWidth(0)
					if selected {
						ui.Icon(c, workspaceIcon("check")).Size(16, 16).Shrink(0).TextColor(iconColor)
					}
				})
			}
		})
	})
}
