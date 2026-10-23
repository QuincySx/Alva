// INPUT: MyGo native base controls, existing Hub draft/execution state and configured engine targets
// OUTPUT: Formal workspace composer with explicit send/stop, attachment and actual route/policy popovers
// POS: [L7 Hub UI] Owns presentation drafts and overlays; all durable actions remain with the existing product owner
//go:build desktop

package hubdesktop

import (
	"fmt"
	"strings"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	view "github.com/QuincySx/Alva/apps/ui/chat"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/egoist/mygo/ui"
)

type workspaceComposerState struct {
	addOpen, permissionOpen, targetOpen bool
	target                              hub.Target
}

func (p *NativePage) workspaceComposerTarget() hub.Target {
	if p.selected != "" && p.detail.Session.ID == p.selected {
		if branch, err := product.ActiveBranch(p.detail); err == nil {
			return branch.Target
		}
	}
	if p.composer.target.EngineID != "" {
		return p.composer.target
	}
	return p.Service.DefaultTarget
}

func (p *NativePage) workspaceComposerRunning() bool {
	return p.model.ActiveRunID != "" || p.selected != "" && p.detail.Session.ID == p.selected && p.detail.Session.Status == hub.Running
}

func (p *NativePage) workspaceComposerStatus() string {
	if p.model.Status == view.Stopping {
		return "正在停止…"
	}
	if len(p.approvals) != 0 && p.workspaceComposerRunning() {
		return "等待你的确认"
	}
	if p.selected != "" && p.detail.Session.ID == p.selected && p.detail.Session.Status == engine.Waiting {
		return "等待你的回应"
	}
	if p.workspaceComposerRunning() {
		return "正在工作…"
	}
	if p.busy {
		return "正在提交…"
	}
	if p.selected != "" && (p.detail.Session.ID != p.selected || p.chat.Busy) {
		return "正在加载…"
	}
	return ""
}

func (p *NativePage) buildWorkspaceComposer(c *ui.Context) {
	t := c.Theme()
	colors := workspacePalette(c)
	running := p.workspaceComposerRunning()
	waiting := p.selected != "" && p.detail.Session.ID == p.selected && p.detail.Session.Status == engine.Waiting
	loading := p.selected != "" && (p.detail.Session.ID != p.selected || p.chat.Busy && !p.busy && !running && !waiting)
	target := p.workspaceComposerTarget()
	ui.Column(c.Key("workspace-composer")).FillWidth().Gap(8).Children(func() {
		if status := p.workspaceComposerStatus(); status != "" {
			ui.Row(c).AlignItems(ui.Center).Gap(8).Children(func() {
				icon := "loader-circle"
				if waiting || len(p.approvals) != 0 {
					icon = "circle-question-mark"
				}
				ui.Icon(c, workspaceIcon(icon)).Size(16, 16).TextColor(t.TextMuted)
				ui.Text(c, status).Role(ui.RoleStatus).FontSize(14).LineHeight(1.5).TextColor(t.TextMuted)
			})
		}
		editor := ui.Column(c).Label("Task composer").FillWidth().Padding(12).Gap(8).
			Radius(16).Border(1, t.Border).Background(t.Surface)
		if editor.FocusWithin() {
			editor.BorderColor(t.Accent).DrawOver(func(painter *ui.Painter, bounds ui.Rect) {
				painter.Stroke(bounds, t.Accent, 16, 2)
			})
		}
		editor.Children(func() {
			if attachments := p.Service.DraftAttachments(p.selected); len(attachments) != 0 {
				ui.Row(c).FillWidth().Wrap().Gap(6).Children(func() {
					for _, attachment := range attachments {
						button := ui.ButtonBase(c.Key(attachment.Info.ID)).Label("Remove draft reference "+attachment.Info.Name).
							Padding(6, 8).Gap(6).Radius(8).Border(1, t.Border).Background(colors.subtle).
							Disabled(p.busy || running || waiting).Tooltip(fmt.Sprintf("%s · %d bytes · 原件已保存，发送时仅附上引用；点击移除此草稿引用", attachment.Info.Name, attachment.Info.Bytes)).
							OnClick(func() { p.Service.RemoveDraftAttachment(p.selected, attachment.Info.ID) })
						button.Children(func() {
							ui.Icon(c, workspaceIcon("paperclip")).Size(14, 14)
							ui.Text(c, attachment.Info.Name).FontSize(12).SingleLine().MaxWidth(180)
							ui.Icon(c, workspaceIcon("x")).Size(14, 14)
						})
					}
				})
			}
			placeholder := "告诉我你要完成什么…"
			if running {
				placeholder = "补充说明，或调整下一步…"
			}
			ui.TextAreaBase(c.Key("workspace-message"), &p.model.Draft).Label("Message").
				Placeholder(placeholder).FillWidth().MinWidth(0).FontSize(16).LineHeight(1.75).
				Lines(2, 5).Padding(0).Border(0, ui.Transparent).FocusRing(false).
				Disabled(p.busy || waiting || loading).OnPaste(func(text string) bool {
				if len(text) <= view.MaxDraftBytes {
					return false
				}
				p.problem = "粘贴内容超过 32 KiB，请通过添加菜单附上文件。草稿未发送。"
				return true
			})
			ui.Row(c).FillWidth().AlignItems(ui.Center).Gap(8).Children(func() {
				add := workspaceComposerIconButton(c.Key("add"), "plus", "Add attachments", "添加文件", 32).
					Disabled(p.busy || running || waiting || loading).Expanded(p.composer.addOpen).
					OnClick(func() {
						p.composer.addOpen = !p.composer.addOpen
						p.composer.permissionOpen = false
						p.composer.targetOpen = false
					})
				p.buildWorkspaceAttachmentMenu(c, add)
				policy, _ := workspacePermissionPolicy(target.EngineID)
				permission := workspaceComposerChip(c.Key("permission"), "shield-check", policy, "Permission policy").
					Expanded(p.composer.permissionOpen).OnClick(func() {
					p.composer.permissionOpen = !p.composer.permissionOpen
					p.composer.addOpen, p.composer.targetOpen = false, false
				})
				p.buildWorkspacePermissionMenu(c, permission, target)
				ui.Box(c).Grow(1).MinWidth(0)
				model := workspaceComposerChip(c.Key("target"), "bot", workspaceTargetLabel(target), "Agent and model").
					MinWidth(0).MaxWidth(240).Shrink(1).Disabled(p.busy || running || waiting || loading).
					Expanded(p.composer.targetOpen).OnClick(func() {
					p.composer.targetOpen = !p.composer.targetOpen
					p.composer.addOpen, p.composer.permissionOpen = false, false
				})
				model.Tooltip(workspaceTargetDetails(target))
				p.buildWorkspaceTargetMenu(c, model, target)
				if running {
					workspaceComposerIconButton(c.Key("stop"), "square", "Stop", "停止任务", 36).
						Background(t.Accent).TextColor(t.AccentText).
						Disabled(p.busy || p.model.Status == view.Stopping).OnClick(p.stop)
				} else {
					button := workspaceComposerIconButton(c.Key("send"), "arrow-up", "Send", "发送", 36).
						Disabled(p.busy || waiting || loading || !p.model.CanSend()).OnClick(p.sendWorkspaceDraft)
					if !p.busy && !waiting && !loading && p.model.CanSend() {
						button.Background(t.Accent).TextColor(t.AccentText)
					} else {
						button.Background(colors.subtle).TextColor(t.TextMuted)
					}
				}
			})
		})
		if len(p.model.Draft) > view.MaxDraftBytes {
			ui.Text(c, "消息超过 32 KiB，请缩短内容或添加文件。").Role(ui.RoleStatus).FontSize(12).TextColor(t.Danger)
		}
	})
}

func workspaceComposerIconButton(c *ui.Context, icon, label, hint string, size float32) ui.Element {
	b := ui.ButtonBase(c).Label(label).Size(size, size).Padding(0).Radius(8).
		AlignItems(ui.Center).Justify(ui.Center).TextColor(c.Theme().TextMuted).Tooltip(hint)
	if b.Hovered() {
		b.Background(workspacePalette(c).hover).TextColor(c.Theme().Text)
	}
	b.Children(func() { ui.Icon(c, workspaceIcon(icon)).Size(20, 20) })
	return b
}

func workspaceComposerChip(c *ui.Context, icon, text, label string) ui.Element {
	b := ui.ButtonBase(c).Label(label).Height(36).Padding(8).Gap(6).Radius(8).
		AlignItems(ui.Center).Justify(ui.Center).TextColor(c.Theme().TextMuted)
	if b.Hovered() {
		b.Background(workspacePalette(c).hover).TextColor(c.Theme().Text)
	}
	b.Children(func() {
		ui.Icon(c, workspaceIcon(icon)).Size(16, 16).Shrink(0)
		ui.Text(c, text).FontSize(14).LineHeight(1.5).SingleLine().MinWidth(0).Shrink(1)
		ui.Icon(c, workspaceIcon("chevron-down")).Size(14, 14).Shrink(0)
	})
	return b
}

func workspaceComposerPopover(c *ui.Context, anchor ui.Element, open *bool, width float32, content func()) {
	ui.PopoverBase(c, anchor, open, func(panel ui.Element) {
		panel.Width(width).MaxWidthPercent(95).Margin(6, 0, 0, 0).Padding(8).Gap(4).
			Radius(12).Background(c.Theme().Surface).Border(1, c.Theme().Border).
			Shadow(0, 6, 20, 0, ui.RGBA(0, 0, 0, 0.10))
		content()
	})
}

func (p *NativePage) buildWorkspaceAttachmentMenu(c *ui.Context, anchor ui.Element) {
	workspaceComposerPopover(c, anchor, &p.composer.addOpen, 320, func() {
		target := p.workspaceComposerTarget()
		enabled := p.selected != "" && target.EngineID == "alva" && p.window != nil && !p.busy && !p.inputBlocked()
		item := ui.ButtonBase(c).Label("Attach file").FillWidth().Padding(8, 12).Gap(8).Radius(8).
			Justify(ui.Start).Disabled(!enabled).OnClick(func() { p.composer.addOpen = false; p.upload() })
		if item.Hovered() {
			item.Background(workspacePalette(c).hover)
		}
		item.Children(func() {
			ui.Icon(c, workspaceIcon("paperclip")).Size(16, 16)
			ui.Text(c, "文件、图片或视频…").FontSize(14).LineHeight(1.5)
		})
		message := "原件保存后，仅在明确发送时附上引用。"
		if p.selected == "" {
			message = "发送首条消息创建会话后，即可添加文件。"
		} else if target.EngineID != "alva" {
			message = "文件引用暂仅支持 Alva 引擎。"
		}
		ui.Text(c, message).FontSize(12).LineHeight(1.5).TextColor(c.Theme().TextMuted).Padding(8, 12)
	})
}

func workspacePermissionPolicy(engineID string) (string, string) {
	switch engineID {
	case "alva":
		return "每次询问", "需要授权的操作会先暂停，等待你明确允许这一次操作。"
	case "codex", "pi":
		return "只读模式", "此引擎使用固定的只读限制，当前界面不提供提升权限。"
	case "claude":
		return "安全模式", "此引擎使用固定的安全模式，当前界面不提供提升权限。"
	default:
		return "引擎权限", "权限由此引擎的实际配置决定。当前界面不提供权限切换。"
	}
}

func (p *NativePage) buildWorkspacePermissionMenu(c *ui.Context, anchor ui.Element, target hub.Target) {
	workspaceComposerPopover(c, anchor, &p.composer.permissionOpen, 280, func() {
		label, description := workspacePermissionPolicy(target.EngineID)
		ui.Row(c).FillWidth().Padding(8, 12).AlignItems(ui.Center).Gap(8).Children(func() {
			ui.Icon(c, workspaceIcon("shield-check")).Size(16, 16).TextColor(c.Theme().Accent)
			ui.Text(c, label).FontSize(14).Grow(1)
			ui.Icon(c, workspaceIcon("check")).Size(16, 16).TextColor(c.Theme().Accent)
		})
		ui.Text(c, description).FontSize(12).LineHeight(1.5).TextColor(c.Theme().TextMuted).Padding(8, 12)
	})
}

func workspaceTargetLabel(target hub.Target) string {
	return target.ModelID + " · " + strings.ToUpper(target.EngineID)
}

func workspaceTargetDetails(target hub.Target) string {
	return fmt.Sprintf("%s · %s\n%s · %s", target.EngineID, target.ProviderID, target.AccountSource, target.BillingRoute)
}

func (p *NativePage) buildWorkspaceTargetMenu(c *ui.Context, anchor ui.Element, current hub.Target) {
	workspaceComposerPopover(c, anchor, &p.composer.targetOpen, 352, func() {
		ui.Text(c, "Agent 与模型").FontSize(12).TextColor(c.Theme().TextMuted).Padding(8, 12)
		for _, target := range p.targets {
			selected := hub.RouteKey(target) == hub.RouteKey(current)
			button := ui.ButtonBase(c.Key(hub.RouteKey(target))).Label("Use "+target.EngineID+" · "+target.ModelID).
				FillWidth().Padding(8, 12).Gap(8).Radius(8).Justify(ui.Start).
				Disabled(p.busy || p.inputBlocked()).OnClick(func() {
				p.composer.targetOpen = false
				if selected {
					return
				}
				if p.selected == "" {
					p.composer.target = target
				} else {
					p.switchBranch(target)
				}
			})
			if selected {
				button.Background(workspacePalette(c).selection)
			} else if button.Hovered() {
				button.Background(workspacePalette(c).hover)
			}
			button.Children(func() {
				ui.Icon(c, workspaceIcon("bot")).Size(16, 16).TextColor(c.Theme().TextMuted)
				ui.Column(c).Grow(1).MinWidth(0).Gap(2).Children(func() {
					ui.Text(c, workspaceTargetLabel(target)).FontSize(14).SingleLine()
					ui.Text(c, target.ProviderID+" · "+target.AccountSource+" · "+target.BillingRoute).
						FontSize(12).TextColor(c.Theme().TextMuted).SingleLine()
				})
				if selected {
					ui.Icon(c, workspaceIcon("check")).Size(16, 16).TextColor(c.Theme().Accent)
				}
			})
		}
	})
}
