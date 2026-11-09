// INPUT: The selected task, SettingsService.Load/Approve in application scope and native MyGo controls
// OUTPUT: One line above the composer when MCP servers or hooks of the effective settings wait for approval in this workspace, with a confirmed approve action
// POS: [L7 Hub UI] Only a view over the existing workspace approval flow; it never approves silently and never changes settings
//go:build desktop

package hubdesktop

import (
	"fmt"
	"strings"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/egoist/mygo/ui"
)

// mcpApprovalNotice is the window's read of which enabled MCP servers and
// hooks the selected task's workspace has not approved yet.
type mcpApprovalNotice struct {
	session   string
	epoch     uint64 // settingsForm.epoch at the last read, to notice edits
	loaded    bool
	loading   bool
	busy      bool
	confirm   bool
	workspace string
	pending   []product.SettingsItem
	problem   string
}

// pendingWorkspaceApprovalKinds are the host items this line speaks about.
func pendingWorkspaceApprovalKinds(kind string) bool {
	return kind == hub.CapabilityMCP || kind == hub.CapabilityHook
}

// observeMCPApprovalNotice reads the pending approvals again when the task or
// the settings changed. It runs from the page poll, never from a frame.
func (p *NativePage) observeMCPApprovalNotice() {
	n := &p.mcpNotice
	if p.Service == nil || p.Service.Settings == nil || p.selected == "" || p.selected != p.detail.Session.ID {
		if n.session != "" {
			*n = mcpApprovalNotice{}
		}
		return
	}
	if n.loading || n.busy {
		return
	}
	if n.session == p.selected && n.loaded && n.epoch == p.settingsForm.epoch {
		return
	}
	if n.session != p.selected {
		*n = mcpApprovalNotice{session: p.selected}
	}
	n.loading, n.epoch = true, p.settingsForm.epoch
	session, epoch, generation := p.selected, n.epoch, p.generation
	go func() {
		view, err := p.Service.Settings.Load(p.ctx, product.SettingsScope{App: true, SessionID: session})
		p.dispatch(func() {
			n.loading = false
			if p.selected != session || p.generation != generation {
				return
			}
			n.loaded, n.epoch = true, epoch
			n.pending, n.workspace, n.problem = nil, view.Workspace, ""
			if err != nil {
				n.problem = err.Error()
				return
			}
			// A task with its own settings does not run the application's items.
			if view.Override {
				return
			}
			for _, item := range view.Items {
				if item.NeedsApproval && pendingWorkspaceApprovalKinds(item.Kind) {
					n.pending = append(n.pending, item)
				}
			}
		})
	}()
}

// mcpApprovalSubject names the waiting items, e.g. "2 个 MCP 服务器、1 个 Hook".
func mcpApprovalSubject(pending []product.SettingsItem) string {
	var servers, hooks int
	for _, item := range pending {
		if item.Kind == hub.CapabilityMCP {
			servers++
		} else {
			hooks++
		}
	}
	var parts []string
	if servers > 0 {
		parts = append(parts, fmt.Sprintf("%d 个 MCP 服务器", servers))
	}
	if hooks > 0 {
		parts = append(parts, fmt.Sprintf("%d 个 Hook", hooks))
	}
	return strings.Join(parts, "、")
}

// mcpApprovalText is the line itself, e.g. "2 个 MCP 服务器在此工作区待批准".
func mcpApprovalText(pending []product.SettingsItem) string {
	return mcpApprovalSubject(pending) + "在此工作区待批准"
}

// approveMCPPending approves every listed item through SettingsService.ApproveExact,
// which refuses an item whose configuration is no longer the one the user confirmed.
func (p *NativePage) approveMCPPending() {
	n := &p.mcpNotice
	if n.busy || n.loading || len(n.pending) == 0 {
		return
	}
	n.busy, n.confirm, n.problem = true, false, ""
	session, generation := p.selected, p.generation
	shown := append([]product.SettingsItem(nil), n.pending...)
	go func() {
		err := func() error {
			for _, item := range shown {
				if _, err := p.Service.Settings.ApproveExact(p.ctx, session, item.Kind, item.Item, item.Fingerprint); err != nil {
					return err
				}
			}
			return nil
		}()
		p.dispatch(func() {
			n.busy = false
			if p.selected != session || p.generation != generation {
				return
			}
			if err != nil {
				n.problem = err.Error()
			}
			// Read again: the approvals now stand, or the problem shows what is left.
			n.loaded = false
			if p.settingsForm.session == session {
				p.settingsForm.epoch++
			}
		})
	}()
}

// buildMCPApprovalNotice is the line above the input box.
func (p *NativePage) buildMCPApprovalNotice(c *ui.Context) {
	n := &p.mcpNotice
	if n.session != p.selected || (len(n.pending) == 0 && n.problem == "") {
		return
	}
	t := c.Theme()
	ui.Column(c.Key("mcp-approval-notice")).Label("Pending workspace approvals").FillWidth().Gap(6).Children(func() {
		if len(n.pending) > 0 {
			ui.Row(c.Key("line")).FillWidth().AlignItems(ui.Center).Gap(8).Children(func() {
				ui.Text(c, mcpApprovalText(n.pending)+" · ").FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(t.TextMuted)
				workspaceControlButton(c, "mcp-approval-review", "批准", "ghost", n.busy || n.confirm).Label("Review workspace approvals").OnClick(func() { n.confirm = true })
			})
		}
		if n.confirm && len(n.pending) > 0 {
			ui.Column(c.Key("confirm")).Label("Workspace approval confirmation").FillWidth().Gap(8).Padding(12).Radius(8).Border(1, t.Border).Children(func() {
				ui.Text(c, "允许 "+mcpApprovalSubject(n.pending)+" 在工作区 "+n.workspace+" 中以你的权限运行当前配置？配置改变后需要重新批准，其他工作区仍要单独批准。").FontSize(workspaceFontBody).LineHeight(1.5).Wrap()
				ui.Row(c).Gap(8).Children(func() {
					workspaceControlButton(c, "mcp-approval-confirm", "批准", "primary", n.busy).Label("Confirm workspace approvals").OnClick(p.approveMCPPending)
					workspaceControlButton(c, "mcp-approval-cancel", "取消", "secondary").Label("Cancel workspace approvals").OnClick(func() { n.confirm = false })
				})
			})
		}
		if n.problem != "" {
			ui.Text(c, n.problem).Role(ui.RoleStatus).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(t.Danger).Wrap()
		}
	})
}
