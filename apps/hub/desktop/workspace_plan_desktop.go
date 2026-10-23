// INPUT: Page-local home intent, asynchronously loaded native settings and the actual saved branch/Turn state.
// OUTPUT: Explicit Plan menu choice, truthful next-turn policy descriptions and successful-store-only UI snapshots.
// POS: [L7 Hub UI] Home selection is a draft; saved-session policy changes go through the shared product service.
//go:build desktop

package hubdesktop

import (
	"github.com/QuincySx/Alva/apps/hub"
	"github.com/egoist/mygo/ui"
)

func workspacePlanCacheKey(id, branchID string) string {
	if id == "" {
		return ""
	}
	return id + "\x00" + branchID
}

func (p *NativePage) workspacePlanKey() string {
	return workspacePlanCacheKey(p.selected, p.detail.Session.ActiveBranchID)
}

func (p *NativePage) workspacePlanEnabled() bool {
	return p.planModes[p.workspacePlanKey()]
}

func (p *NativePage) acceptWorkspacePlanSettings(id, branchID string, enabled bool) {
	if p.busy || p.selected != id || p.detail.Session.ID != id || p.detail.Session.ActiveBranchID != branchID {
		return
	}
	if p.planModes == nil {
		p.planModes = make(map[string]bool)
	}
	if p.planLoaded == nil {
		p.planLoaded = make(map[string]bool)
	}
	key := workspacePlanCacheKey(id, branchID)
	p.planModes[key], p.planLoaded[key] = enabled, true
}

func (p *NativePage) workspacePlanDescription() string {
	if p.workspaceComposerTarget().EngineID != "alva" {
		return "此引擎未提供等价的计划策略，不能切换计划模式。"
	}
	if p.selected != "" && !p.planLoaded[p.workspacePlanKey()] {
		return "正在读取已保存的下一轮策略；读取完成后可选择计划模式。"
	}
	if p.workspacePlanEnabled() {
		return "只读计划：下一轮只允许读取；写文件、执行、网络和未知效果直接拒绝。Hooks、诊断、MCP/AEP、Shell/Web、浏览器、媒体与子 Agent 本轮不装配。关闭后恢复逐次询问，仍须明确发送。"
	}
	return "计划模式限制下一轮为只读规划；只保存选择，明确发送后才执行。关闭后恢复逐次询问，不会批准任何操作。"
}

func (p *NativePage) workspacePlanCanChange() bool {
	return p.workspaceComposerTarget().EngineID == "alva" && !p.busy && !p.workspaceComposerRunning() && !p.inputBlocked() && (p.selected == "" || p.detail.Session.ID == p.selected && !p.chat.Busy && p.planLoaded[p.workspacePlanKey()] && !hub.BlocksInput(p.detail.Session.Status))
}

func (p *NativePage) buildWorkspacePlanMenuItem(c *ui.Context) {
	enabled := p.workspacePlanEnabled()
	item := ui.ButtonBase(c.Key("plan-mode")).Role(ui.RoleMenuItemRadio).Label("Plan mode").Checked(enabled).
		FillWidth().Padding(8, 12).Gap(8).Radius(8).Justify(ui.Start).
		Disabled(!p.workspacePlanCanChange()).Tooltip(p.workspacePlanDescription()).
		OnClick(func() { p.composer.addOpen = false; p.setWorkspacePlanMode(!enabled) })
	if item.Hovered() {
		item.Background(workspacePalette(c).hover)
	}
	item.Children(func() {
		ui.Icon(c, workspaceIcon("book-open")).Size(16, 16)
		ui.Text(c, "计划模式").FontSize(14).LineHeight(1.5).Grow(1)
		if enabled {
			ui.Icon(c, workspaceIcon("check")).Size(16, 16)
		}
	})
}

func (p *NativePage) setWorkspacePlanMode(enabled bool) {
	if !p.workspacePlanCanChange() {
		return
	}
	if p.planModes == nil {
		p.planModes = make(map[string]bool)
	}
	if p.selected == "" {
		p.planModes[""] = enabled
		return
	}
	id, branchID := p.selected, p.detail.Session.ActiveBranchID
	p.generation++ // Reject reads captured before this explicit policy edit.
	p.busy = true
	go func() {
		err := p.Service.SetPlanMode(p.ctx, id, enabled)
		p.dispatch(func() {
			p.busy = false
			if p.selected != id || p.detail.Session.ActiveBranchID != branchID {
				return
			}
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.acceptWorkspacePlanSettings(id, branchID, enabled)
			p.problem = ""
		})
	}()
}
