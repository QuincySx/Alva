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
	targetFocus                         ui.Handle
	targetActive, context               string
}

func (s *workspaceComposerState) closePopovers() {
	s.addOpen, s.permissionOpen, s.targetOpen = false, false, false
	s.targetActive = ""
	s.targetFocus.CancelFocus()
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
	if p.modelStopping() {
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
	status := p.model.Status
	if p.selected != "" && p.detail.Session.ID == p.selected {
		status = view.Status(p.detail.Session.Status)
	}
	switch status {
	case view.Completed:
		return "已完成"
	case view.Cancelled:
		return "已停止，已保留部分输出"
	case view.Failed, view.Status(engine.Interrupted):
		return "执行失败"
	}
	return ""
}

func (p *NativePage) buildWorkspaceComposer(c *ui.Context) {
	t := c.Theme()
	running := p.workspaceComposerRunning()
	waiting := p.selected != "" && p.detail.Session.ID == p.selected && p.detail.Session.Status == engine.Waiting
	loading := p.selected != "" && (p.detail.Session.ID != p.selected || p.chat.Busy && !p.busy && !running && !waiting)
	target := p.workspaceComposerTarget()
	composerContext := p.pane + "\x00" + p.workspace + "\x00" + p.selected + "\x00" + p.detail.Session.ActiveBranchID + "\x00" + hub.RouteKey(target)
	if p.composer.context != composerContext {
		p.composer.closePopovers()
		p.composer.context = composerContext
	}
	attachmentProblem := p.workspaceAttachmentProblem()
	if attachmentProblem == "" {
		attachmentProblem = p.workspaceTargetProblem()
	}
	ui.Column(c.Key("workspace-composer")).FillWidth().Gap(8).Children(func() {
		if status := p.workspaceComposerStatus(); status != "" {
			ui.Row(c.Key("status")).AlignItems(ui.Center).Gap(8).Children(func() {
				icon := "loader-circle"
				switch {
				case waiting || len(p.approvals) != 0:
					icon = "circle-question-mark"
				case status == "已完成":
					icon = "check"
				case status == "已停止，已保留部分输出":
					icon = "square"
				case status == "执行失败":
					icon = "triangle-alert"
				}
				ui.Icon(c, workspaceIcon(icon)).Size(16, 16).TextColor(t.TextMuted)
				ui.Text(c, status).Role(ui.RoleStatus).FontSize(14).LineHeight(1.5).TextColor(t.TextMuted)
			})
		}
		editor := ui.Column(c.Key("editor")).Label("Task composer").FillWidth().Padding(8).Gap(8).
			Radius(16).Border(1, t.Border).Background(t.Surface)
		if editor.FocusWithin() {
			editor.BorderColor(t.Accent).DrawOver(func(painter *ui.Painter, bounds ui.Rect) {
				painter.Stroke(bounds, t.Accent, 16, 2)
			})
		}
		editor.Children(func() {
			p.buildWorkspaceAttachments(c)
			placeholder := "告诉我你要完成什么…"
			if running {
				placeholder = "补充说明，或调整下一步…"
			}
			messageEditor := ui.TextAreaBase(c.Key("workspace-message"), &p.model.Draft).Label("Message").
				Placeholder(placeholder).FillWidth().MinWidth(0).FontSize(16).LineHeight(1.75).
				Lines(2, 5).Padding(0).Border(0, ui.Transparent).FocusRing(false).
				Disabled(waiting || loading).OnPaste(func(text string) bool {
				if len(text) <= view.MaxDraftBytes {
					return false
				}
				p.problem = "粘贴内容超过 32 KiB，请通过添加菜单附上文件。草稿未发送。"
				return true
			})
			p.buildWorkspacePicker(c, messageEditor, editor)
			ui.Row(c.Key("controls")).FillWidth().AlignItems(ui.Center).Gap(8).Children(func() {
				add := workspaceComposerIconButton(c.Key("add"), "plus", "Add attachments", "添加文件", 32).
					Disabled(p.busy || running || waiting || loading).Expanded(p.composer.addOpen).
					OnClick(func() {
						p.closeWorkspacePicker()
						p.composer.addOpen = !p.composer.addOpen
						p.composer.permissionOpen = false
						p.composer.targetOpen = false
					})
				p.buildWorkspaceAttachmentMenu(c, add)
				policy, policyDescription := workspacePermissionPolicy(target.EngineID)
				if p.workspacePlanEnabled() {
					policy, policyDescription = "只读计划", p.workspacePlanDescription()
				}
				permission := workspaceComposerChip(c.Key("permission"), "shield-check", policy, "Permission policy").
					Tooltip(policyDescription).Expanded(p.composer.permissionOpen).OnClick(func() {
					p.closeWorkspacePicker()
					p.composer.permissionOpen = !p.composer.permissionOpen
					p.composer.addOpen, p.composer.targetOpen = false, false
				})
				p.buildWorkspacePermissionMenu(c, permission, target)
				ui.Box(c).Grow(1).MinWidth(0)
				model := workspaceComposerChip(c.Key("target"), "bot", workspaceTargetLabel(target), "Agent and model").
					MinWidth(0).MaxWidth(240).Shrink(1).Disabled(p.busy || running || waiting || loading).
					Expanded(p.composer.targetOpen).OnClick(func() {
					p.closeWorkspacePicker()
					p.composer.targetOpen = !p.composer.targetOpen
					p.composer.addOpen, p.composer.permissionOpen = false, false
					if p.composer.targetOpen {
						p.composer.targetActive = ""
						p.composer.targetFocus.RequestFocus(c)
					}
				})
				model.Tooltip("选择下一次提交使用的模型与 Agent。\n" + workspaceTargetDetails(target))
				p.buildWorkspaceTargetMenu(c, model, target)
				if running {
					hint := "停止任务；等待执行进程和回调清理完成。"
					if p.stopRetry[p.selected] {
						hint = "停止请求失败，可重试；执行仍在收尾。"
					} else if p.modelStopping() {
						hint = "正在停止，等待执行进程和回调清理完成。"
					} else if p.busy {
						hint = "正在提交，等待真实接收回执后可停止。"
					}
					workspaceComposerAction(c.Key("stop"), "square", "Stop", hint, p.busy || p.modelStopping() && !p.stopRetry[p.selected], true, p.stop)
				} else {
					hint := "发送消息；Enter 换行。"
					switch {
					case p.busy:
						hint = "正在提交或导入附件，等待真实接收回执。"
					case waiting:
						hint = "任务正在等待回应，请先处理等待中的问题或审批。"
					case loading:
						hint = "正在加载当前会话，完成后可发送。"
					case attachmentProblem != "":
						hint = attachmentProblem
					case len(p.model.Draft) > view.MaxDraftBytes:
						hint = "消息超过 32 KiB，请缩短内容或添加文件。"
					case !p.model.CanSend():
						hint = "请输入消息后发送；Enter 换行。"
					}
					enabled := !p.busy && !waiting && !loading && attachmentProblem == "" && p.model.CanSend()
					workspaceComposerAction(c.Key("send"), "arrow-up", "Send", hint, !enabled, enabled, p.sendWorkspaceDraft)
				}
			})
		})
		if attachmentProblem != "" {
			ui.Text(c, attachmentProblem).Role(ui.RoleStatus).FontSize(12).TextColor(t.Danger)
		}
		if len(p.model.Draft) > view.MaxDraftBytes {
			ui.Text(c, "消息超过 32 KiB，请缩短内容或添加文件。").Role(ui.RoleStatus).FontSize(12).TextColor(t.Danger)
		}
	})
}

// Disabled native controls do not receive tooltips. Their fixed-size parent
// exposes the reason to pointer and keyboard users while the action stays disabled.
func workspaceComposerAction(c *ui.Context, icon, label, hint string, disabled, primary bool, action func()) {
	slot := ui.Box(c).Size(32, 32)
	if disabled {
		slot.Label(label + " unavailable").Focusable().Tooltip(hint)
	}
	slot.Children(func() {
		button := workspaceComposerIconButton(c.Key("control"), icon, label, hint, 32).Radius(16).
			Disabled(disabled).OnClick(action)
		if primary {
			button.Background(c.Theme().Accent).TextColor(c.Theme().AccentText)
		} else {
			button.Background(workspacePalette(c).subtle).TextColor(c.Theme().TextMuted)
		}
	})
}

func workspaceComposerIconButton(c *ui.Context, icon, label, hint string, size float32) ui.Element {
	b := ui.ButtonBase(c).Label(label).Size(size, size).Padding(0).Radius(8).
		AlignItems(ui.Center).Justify(ui.Center).TextColor(c.Theme().TextMuted).Tooltip(hint)
	if b.Hovered() || b.FocusVisible() {
		b.Background(workspacePalette(c).hover).TextColor(c.Theme().Text)
	}
	b.Children(func() { ui.Icon(c, workspaceIcon(icon)).Size(18, 18) })
	return b
}

func workspaceComposerChip(c *ui.Context, icon, text, label string) ui.Element {
	b := ui.ButtonBase(c).Label(label).Height(32).Padding(4, 12).Gap(6).Radius(8).
		AlignItems(ui.Center).Justify(ui.Center).TextColor(c.Theme().TextMuted)
	if b.Hovered() || b.FocusVisible() {
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
	_, height := c.Size()
	bounds := anchor.Bounds()
	available := max(0, max(bounds.Y, height-bounds.Y-bounds.H)-10)
	ui.PopoverBase(c, anchor, open, func(panel ui.Element) {
		panel.Label("Composer menu").AttachTo(anchor, ui.AnchorTopLeft, ui.AnchorBottomLeft).Width(width).MaxWidthPercent(95).MaxHeight(available).Margin(0, 0, 6, 0).Padding(8).Gap(4).
			Radius(12).Background(c.Theme().Surface).Border(1, c.Theme().Border).
			Shadow(0, 6, 20, 0, ui.RGBA(0, 0, 0, 0.10))
		ui.Scroll(c).Label("Composer menu contents").FillWidth().MaxHeight(max(0, available-18)).Children(content)
	})
}

func (p *NativePage) buildWorkspaceAttachmentMenu(c *ui.Context, anchor ui.Element) {
	workspaceComposerPopover(c, anchor, &p.composer.addOpen, 320, func() {
		target := p.workspaceComposerTarget()
		enabled := target.EngineID == "alva" && p.window != nil && !p.busy && !p.inputBlocked()
		item := ui.ButtonBase(c).Label("Attach file").FillWidth().Padding(8, 12).Gap(8).Radius(8).
			Justify(ui.Start).Disabled(!enabled).OnClick(func() { p.composer.addOpen = false; p.chooseWorkspaceFiles() })
		if item.Hovered() {
			item.Background(workspacePalette(c).hover)
		}
		item.Children(func() {
			ui.Icon(c, workspaceIcon("paperclip")).Size(16, 16)
			ui.Text(c, "文件、图片或视频…").FontSize(14).LineHeight(1.5)
		})
		for _, entry := range []struct{ kind, icon, label string }{
			{"references", "folder", "项目文件或目录…"},
			{"commands", "code-xml", "命令与 Skills…"},
		} {
			disabled := p.busy || p.inputBlocked() || entry.kind == "references" && (p.selected == "" || target.EngineID != "alva")
			button := ui.ButtonBase(c.Key(entry.kind)).Label(entry.label).FillWidth().Padding(8, 12).Gap(8).Radius(8).
				AlignItems(ui.Center).Justify(ui.Start).Disabled(disabled).OnClick(func() {
				p.composer.addOpen = false
				p.openWorkspacePicker(entry.kind)
			})
			if button.Hovered() {
				button.Background(workspacePalette(c).hover)
			}
			button.Children(func() {
				ui.Icon(c, workspaceIcon(entry.icon)).Size(16, 16)
				ui.Text(c, entry.label).FontSize(14).LineHeight(1.5)
			})
		}
		p.buildWorkspacePlanMenuItem(c)
		if target.EngineID != "alva" {
			ui.Text(c, "文件引用暂仅支持 Alva 引擎。").FontSize(12).TextColor(c.Theme().TextMuted).Padding(8, 12)
		}
	})
}

func workspacePermissionPolicy(engineID string) (string, string) {
	switch engineID {
	case "alva":
		return "每次询问", "需要权限时先询问；仅批准本次所示操作。当前宿主固定为 Ask，尚未提供策略切换。"
	case "codex", "pi":
		return "只读模式", "此引擎使用固定的只读限制，当前界面不提供提升权限。"
	case "claude":
		return "安全模式", "此引擎使用固定的安全模式，当前界面不提供提升权限。"
	default:
		return "引擎权限", "权限由此引擎的实际配置决定。当前界面不提供权限切换。"
	}
}

func (p *NativePage) buildWorkspacePermissionMenu(c *ui.Context, anchor ui.Element, target hub.Target) {
	workspaceComposerPopover(c, anchor, &p.composer.permissionOpen, 320, func() {
		label, description := workspacePermissionPolicy(target.EngineID)
		if p.workspacePlanEnabled() {
			label, description = "只读计划", p.workspacePlanDescription()
		}
		ui.Row(c).Role(ui.RoleMenuItemRadio).Label("Current permission policy").Checked(true).
			FillWidth().Padding(8, 12).AlignItems(ui.Center).Gap(8).Children(func() {
			ui.Icon(c, workspaceIcon("shield-check")).Size(16, 16).TextColor(c.Theme().Accent)
			ui.Text(c, label).FontSize(14).Grow(1)
			ui.Icon(c, workspaceIcon("check")).Size(16, 16).TextColor(c.Theme().Accent)
		})
		ui.Text(c, description).FontSize(12).LineHeight(1.5).TextColor(c.Theme().TextMuted).Padding(8, 12)
		if target.EngineID != "alva" {
			workspaceUnavailablePermission(c, "每次询问（Ask）", "当前引擎未提供逐次审批策略切换。")
		}
		workspaceUnavailablePermission(c, "自动审批（Auto）", "当前宿主未配置自动审批策略，无法选择。")
		workspaceUnavailablePermission(c, "完全访问（Full）", "当前宿主未配置完全访问的授权范围，无法选择。")
	})
}

func workspaceUnavailablePermission(c *ui.Context, label, reason string) {
	ui.ButtonBase(c).Role(ui.RoleMenuItemRadio).Label(label).Checked(false).Disabled(true).
		FillWidth().Padding(8, 12).Tooltip(reason).Children(func() {
		ui.Column(c).FillWidth().Gap(2).Children(func() {
			ui.Text(c, label+" · 不可用").FontSize(14).TextColor(c.Theme().TextMuted)
			ui.Text(c, reason).FontSize(12).LineHeight(1.5).TextColor(c.Theme().TextMuted)
		})
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
		focusIndex := 0
		for i, target := range p.targets {
			if hub.RouteKey(target) == hub.RouteKey(current) {
				focusIndex = i
				break
			}
		}
		ui.Column(c).Label("Configured agents and models").Role(ui.RoleList).FillWidth().Shrink(0).Gap(4).FocusGroup(ui.Vertical).Children(func() {
			for i, target := range p.targets {
				key := hub.RouteKey(target)
				selected := key == hub.RouteKey(current)
				button := ui.ButtonBase(c.Key(key)).Role(ui.RoleListItem).Selected(selected).
					Label("Use "+target.EngineID+" · "+target.ModelID).
					FillWidth().Shrink(0).Padding(8, 12).Gap(8).Radius(8).Justify(ui.Start).
					Disabled(p.busy || p.inputBlocked()).OnClick(func() {
					p.composer.closePopovers()
					if selected {
						return
					}
					if p.selected == "" {
						p.composer.target = target
					} else {
						p.switchBranch(target)
					}
				})
				if i == focusIndex {
					button.Bind(&p.composer.targetFocus)
				}
				if button.Focused() && p.composer.targetActive != key {
					p.composer.targetActive = key
					button.ScrollIntoView()
				}
				if selected {
					button.Background(workspacePalette(c).selection)
				} else if button.Hovered() || button.FocusVisible() {
					button.Background(workspacePalette(c).hover)
				}
				button.Children(func() {
					ui.Icon(c, workspaceIcon("bot")).Size(16, 16).TextColor(c.Theme().TextMuted)
					ui.Column(c).Grow(1).MinWidth(0).Gap(2).Children(func() {
						ui.Text(c, workspaceTargetLabel(target)).FontSize(14).TextColor(c.Theme().Text).SingleLine()
						ui.Text(c, target.ProviderID+" · "+target.AccountSource+" · "+target.BillingRoute).
							FontSize(12).TextColor(c.Theme().TextMuted).SingleLine()
					})
					if selected {
						ui.Icon(c, workspaceIcon("check")).Size(16, 16).TextColor(c.Theme().Accent)
					}
				})
			}
		})
	})
}
