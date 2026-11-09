// INPUT: MyGo native base controls, existing Hub draft/execution state and configured engine targets
// OUTPUT: Formal workspace composer with explicit send/stop, attachment, actual route/policy popovers and the context meter slot
// POS: [L7 Hub UI] Owns presentation drafts and overlays; all durable actions remain with the existing product owner
//go:build desktop

package hubdesktop

import (
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
	confirmFull                         bool
	settingsSaving                      bool
	modelPicker                         workspaceModelPickerState
	contextOpen                         bool // context meter popover (M4)
}

func (s *workspaceComposerState) closePopovers() {
	s.addOpen, s.permissionOpen, s.targetOpen = false, false, false
	s.targetActive = ""
	s.confirmFull = false
	s.targetFocus.CancelFocus()
	s.modelPicker.open = false
	s.contextOpen = false
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

func (p *NativePage) workspaceComposerLoading() bool {
	return p.selected != "" && (p.detail.Session.ID != p.selected || p.chat.Busy && !p.busy && !p.workspaceComposerRunning() && p.detail.Session.Status != engine.Waiting)
}

// workspaceComposerKey is the composer's text-key owner. Enter sends and
// Shift+Enter breaks the line; the platform Cmd/Ctrl+Enter still sends. Three
// guards keep Enter from sending: an input method composing (Enter picks a
// candidate there), an open completion menu (Enter picks an entry), and a
// composer that cannot send right now (the draft is kept and Enter keeps its
// old line-break meaning; a blank draft takes no stray newline).
func (p *NativePage) workspaceComposerKey(k ui.TextKey) bool {
	if k.Key == ui.KeyEnter && (k.Mods == ui.Cmd || k.Mods == 0 && !p.picker.open) {
		if p.picker.open || p.picker.composing {
			return true
		}
		// TextKey includes input queued before this key. Do not send the
		// previous frame's draft or clear text before the real receipt.
		draft := k.Text()
		if k.Mods == 0 && strings.TrimSpace(draft) == "" {
			return true // Enter on a blank draft neither sends nor adds a line
		}
		p.model.Draft = draft
		p.markWorkspaceDraftEdited()
		if p.workspaceRunningInput() {
			// While the Turn runs, Enter queues the message and Cmd+Enter
			// interjects it (the service queues it instead when the engine
			// cannot take it).
			p.submitRunningDraft(k.Mods == ui.Cmd)
			return true
		}
		if p.workspaceComposerSendProblem() == "" {
			p.sendWorkspaceDraft()
			return true
		}
		return k.Mods == ui.Cmd // a blocked Enter stays a line break; a blocked Cmd+Enter is taken
	}
	return p.workspacePickerKey(k)
}

// Pointer and editor shortcuts share this view-level readiness check. Durable
// admission remains in the existing product service and execution owner.
func (p *NativePage) workspaceComposerSendProblem() string {
	if p.Service.ComposerDrafts != nil {
		e := p.durableDrafts.entries[p.workspaceComposerDraftScope()]
		if e != nil && e.loading {
			return "正在载入共享草稿，请稍候。"
		}
		if e != nil && e.problem != "" {
			return e.problem
		}
	}
	switch {
	case p.composer.settingsSaving:
		return "正在保存下一轮设置，完成后可发送。"
	case p.busy:
		return "正在提交或导入附件，等待真实接收回执。"
	case p.workspaceComposerRunning():
		return "任务正在进行，草稿已保留；请等待完成或先停止任务。"
	case p.inputBlocked():
		return "任务正在等待回应，请先处理等待中的问题或审批。"
	case p.workspaceComposerLoading():
		return "正在加载当前会话，完成后可发送。"
	case p.voice.recorder != nil:
		return "请先结束录音并确认文字，再发送。"
	}
	if problem := p.workspaceAttachmentProblem(); problem != "" {
		return problem
	}
	if problem := p.workspaceTargetProblem(); problem != "" {
		return problem
	}
	if len(p.model.Draft) > view.MaxDraftBytes {
		return "消息超过 32 KiB，请缩短内容或添加文件。"
	}
	if !p.model.CanSend() {
		return "请输入消息后发送；Shift+Enter 换行。"
	}
	return ""
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
	if p.composer.settingsSaving {
		return "正在保存设置…"
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
	loading := p.workspaceComposerLoading()
	target := p.workspaceComposerTarget()
	composerContext := p.pane + "\x00" + p.workspace + "\x00" + p.selected + "\x00" + p.detail.Session.ActiveBranchID + "\x00" + hub.RouteKey(target)
	if p.composer.context != composerContext {
		p.composer.closePopovers()
		p.composer.context = composerContext
		if p.selected == "" && p.homePermissionScope != p.workspacePermissionScope() {
			p.acceptWorkspaceHomePermission(hub.PermissionAsk)
		}
	}
	attachmentProblem := p.workspaceAttachmentProblem()
	if attachmentProblem == "" {
		attachmentProblem = p.workspaceTargetProblem()
	}
	ui.Column(c.Key("workspace-composer")).FillWidth().Gap(8).Children(func() {
		p.buildWorkspaceDraftNotice(c)
		p.buildMCPApprovalNotice(c)
		p.buildWorkspaceQueue(c)
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
				if icon == "loader-circle" {
					p.workspaceLoadingIcon(c.Key("activity"), 16, t.TextMuted)
				} else {
					ui.Icon(c, workspaceIcon(icon)).Size(16, 16).TextColor(t.TextMuted)
				}
				ui.Text(c, status).Role(ui.RoleStatus).FontSize(workspaceFontBody).LineHeight(1.5).TextColor(t.TextMuted)
			})
		}
		editor := ui.Column(c.Key("editor")).Label("Task composer").FillWidth().Padding(8).Gap(8).
			Radius(16).Background(t.Surface)
		// The Foundation border is an inside stroke, separate from the 8px
		// content inset. MyGo's layout Border would add a second inset.
		editor.DrawOver(func(painter *ui.Painter, bounds ui.Rect) {
			color, width := t.Border, float32(1)
			if editor.FocusWithin() {
				color, width = t.Accent, 2
			}
			painter.Stroke(bounds, color, 16, width)
		})
		editor.Children(func() {
			p.buildWorkspaceAttachments(c)
			placeholder := "告诉我你要完成什么…"
			if running {
				placeholder = "补充说明，Enter 排队…"
				if p.thread.caps.Interject {
					placeholder = "补充说明，Enter 排队，⌘Enter 插话…"
				}
			}
			messageEditor := ui.TextAreaBase(c.Key("workspace-message"), &p.model.Draft).Label("Message").
				Placeholder(placeholder).FillWidth().MinWidth(0).FontSize(workspaceFontSubtitle).LineHeight(workspaceReadingLineHeight).
				Lines(2, 5).Padding(0).Border(0, ui.Transparent).FocusRing(false).
				Disabled(waiting || loading).OnChange(p.markWorkspaceDraftEdited).OnPaste(func(text string) bool {
				if len(text) <= view.MaxDraftBytes {
					return false
				}
				p.problem = "粘贴内容超过 32 KiB，请通过添加菜单附上文件。草稿未发送。"
				return true
			})
			p.buildWorkspacePicker(c, messageEditor, editor)
			ui.Row(c.Key("controls")).FillWidth().AlignItems(ui.Center).Gap(8).Children(func() {
				add := workspaceComposerIconButton(c.Key("add"), "plus", "Add attachments", "添加文件", workspaceIconControlSize).
					Disabled(p.busy || running || waiting || loading).Expanded(p.composer.addOpen).
					OnClick(func() {
						p.closeWorkspacePicker()
						p.composer.addOpen = !p.composer.addOpen
						p.composer.permissionOpen = false
						p.composer.targetOpen = false
					})
				p.buildWorkspaceAttachmentMenu(c, add)
				policy, policyDescription := p.workspacePermissionDescription(target.EngineID)
				policyIcon := "shield-check"
				if target.EngineID == "alva" {
					switch p.workspacePermissionPolicy() {
					case hub.PermissionAuto:
						policyIcon = "sparkles"
					case hub.PermissionFull:
						policyIcon = "triangle-alert"
					}
				}
				if p.workspacePlanEnabled() {
					policy, policyDescription = "只读计划", p.workspacePlanDescription()
					policyIcon = "list"
				}
				permission := workspaceComposerChip(c.Key("permission"), policyIcon, policy, "Permission policy").
					Tooltip(policyDescription).Expanded(p.composer.permissionOpen).OnClick(func() {
					p.closeWorkspacePicker()
					p.composer.permissionOpen = !p.composer.permissionOpen
					p.composer.confirmFull = false
					p.composer.addOpen, p.composer.targetOpen = false, false
				})
				p.buildWorkspacePermissionMenu(c, permission, target)
				ui.Box(c).Grow(1).MinWidth(0)
				p.buildWorkspaceContextMeter(c)
				model := workspaceComposerChip(c.Key("target"), "bot", workspaceTargetLabel(target), "Agent and model").
					MinWidth(0).MaxWidth(240).Shrink(1).Disabled(p.busy || loading).
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
				p.buildWorkspaceVoice(c)
				if running {
					hint := "停止任务；等待执行进程和回调清理完成。"
					if p.stopRetry[p.selected] {
						hint = "停止请求失败，可重试；执行仍在收尾。"
					} else if p.modelStopping() {
						hint = "正在停止，等待执行进程和回调清理完成。"
					} else if p.busy {
						hint = "正在提交，等待真实接收回执后可停止。"
					}
					id := p.selected
					executionID, _ := p.Service.Turns.Active(id)
					p.workspaceComposerAction(c.Key("stop-"+id+"-"+executionID), "square", "Stop", hint, executionID == "" || p.busy || p.modelStopping() && !p.stopRetry[id], true, p.modelStopping() && !p.stopRetry[id], func() { p.stopExecution(id, executionID) })
				} else {
					hint, enabled := p.workspaceComposerSendProblem(), false
					if hint == "" {
						hint, enabled = "发送消息；Enter 发送，Shift+Enter 换行。", true
					}
					p.workspaceComposerAction(c.Key("send"), "arrow-up", "Send", hint, !enabled, enabled || p.busy, p.busy, p.sendWorkspaceDraft)
				}
			})
		})
		if attachmentProblem != "" {
			ui.Text(c, attachmentProblem).Role(ui.RoleStatus).FontSize(workspaceFontCaption).TextColor(t.Danger)
		}
		if len(p.model.Draft) > view.MaxDraftBytes {
			ui.Text(c, "消息超过 32 KiB，请缩短内容或添加文件。").Role(ui.RoleStatus).FontSize(workspaceFontCaption).TextColor(t.Danger)
		}
	})
}

// Disabled native controls do not receive tooltips. Their fixed-size parent
// exposes the reason to pointer and keyboard users while the action stays disabled.
func (p *NativePage) workspaceComposerAction(c *ui.Context, icon, label, hint string, disabled, primary, loading bool, action func()) {
	slot := ui.Box(c).Size(workspaceIconControlSize, workspaceIconControlSize)
	if disabled && !loading {
		slot.Label(label + " unavailable").Focusable().Tooltip(hint)
	}
	slot.Children(func() {
		activityColor := c.Theme().TextMuted
		if primary {
			activityColor = c.Theme().Surface
		}
		button := workspaceComposerIconButtonState(c.Key("control"), icon, label, hint, workspaceIconControlSize, loading, activityColor, p.reducedMotion(c)).Radius(16).
			Disabled(disabled && !loading).OnClick(func() {
			if !disabled {
				action()
			}
		})
		if primary {
			button.Background(c.Theme().Text).TextColor(c.Theme().Surface)
		} else {
			button.Background(workspacePalette(c).subtle).TextColor(c.Theme().TextMuted)
		}
	})
}

func workspaceComposerIconButton(c *ui.Context, icon, label, hint string, size float32) ui.Element {
	return workspaceComposerIconButtonState(c, icon, label, hint, size, false, c.Theme().TextMuted, c.Preferences().ReduceMotion)
}

func workspaceComposerIconButtonState(c *ui.Context, icon, label, hint string, size float32, loading bool, activityColor ui.Color, reduced bool) ui.Element {
	b := ui.ButtonBase(c).Label(label).Size(size, size).Padding(0).Radius(8).
		AlignItems(ui.Center).Justify(ui.Center).TextColor(c.Theme().TextMuted).Tooltip(hint)
	if b.Hovered() || b.FocusVisible() {
		b.Background(workspacePalette(c).hover).TextColor(c.Theme().Text)
	}
	b.Children(func() {
		if loading {
			workspaceLoadingIconMotion(c.Key("activity"), workspaceIconSize, activityColor, reduced)
		} else {
			iconSize := workspaceIconSize
			ui.Icon(c, workspaceIcon(icon)).Size(iconSize, iconSize)
		}
	})
	return b
}

func workspaceComposerChip(c *ui.Context, icon, text, label string) ui.Element {
	b := ui.ButtonBase(c).Label(label).Height(workspaceIconControlSize).Padding(4, 8).Gap(8).Radius(8).
		AlignItems(ui.Center).Justify(ui.Center).TextColor(c.Theme().TextMuted)
	if b.Hovered() || b.FocusVisible() {
		b.Background(workspacePalette(c).hover).TextColor(c.Theme().Text)
	}
	b.Children(func() {
		ui.Icon(c, workspaceIcon(icon)).Size(16, 16).Shrink(0)
		ui.Text(c, text).FontSize(workspaceFontBody).LineHeight(1.5).SingleLine().MinWidth(0).Shrink(1)
		ui.Icon(c, workspaceIcon("chevron-down")).Size(14, 14).Shrink(0)
	})
	return b
}

func workspaceComposerPopover(c *ui.Context, anchor ui.Element, open *bool, width float32, content func()) {
	_, height := c.Size()
	bounds := anchor.Bounds()
	available := max(0, max(bounds.Y, height-bounds.Y-bounds.H)-10)
	ui.PopoverBase(c, anchor, open, func(panel ui.Element) {
		panel.Label("Composer menu").AttachTo(anchor, ui.AnchorTopLeft, ui.AnchorBottomLeft).Width(width).MaxWidthPercent(95).MaxHeight(available).Margin(0, 0, 6, 0).Padding(8).Gap(0).
			Radius(16).Background(c.Theme().Surface).
			Shadow(0, 8, 30, -6, ui.RGBA(28, 32, 55, 0.17)).DrawOver(func(p *ui.Painter, r ui.Rect) {
			p.Stroke(r, c.Theme().Border, 16, 1)
		})
		ui.Scroll(c).Label("Composer menu contents").Column().FillWidth().Gap(0).MaxHeight(max(0, available-16)).Children(content)
	})
}

// Single-line MenuItem and SelectOption use the normal 28px row and
// symmetric vertical padding; the inner focus stroke never alters layout.
func workspaceComposerMenuItem(c *ui.Context, label string) ui.Element {
	item := ui.ButtonBase(c).Label(label).FillWidth().MinHeight(workspaceNavigationRowHeight).Padding(4, 8).Gap(8).Radius(8).
		AlignItems(ui.Center).Justify(ui.Start).TextColor(c.Theme().Text).
		FontSize(workspaceFontBody).FontWeight(400).LineHeight(1.5).FocusRing(false)
	if item.Hovered() {
		item.Background(workspacePalette(c).hover)
	}
	item.DrawOver(func(p *ui.Painter, r ui.Rect) {
		if item.FocusVisible() {
			p.Stroke(r, c.Theme().Accent, 8, 2)
		}
	})
	return item
}

func (p *NativePage) buildWorkspaceAttachmentMenu(c *ui.Context, anchor ui.Element) {
	workspaceComposerPopover(c, anchor, &p.composer.addOpen, 352, func() {
		target := p.workspaceComposerTarget()
		enabled := p.window != nil && !p.busy && !p.inputBlocked()
		item := workspaceComposerMenuItem(c, "Attach file").Disabled(!enabled).
			OnClick(func() { p.composer.addOpen = false; p.chooseWorkspaceFiles() })
		if !enabled {
			item.TextColor(c.Theme().TextMuted)
		}
		item.Children(func() {
			ui.Icon(c, workspaceIcon("paperclip")).Size(workspaceIconSize, workspaceIconSize).Shrink(0).TextColor(c.Theme().TextMuted)
			ui.Text(c, "文件、图片或视频…").FontSize(workspaceFontBody).LineHeight(1.5).SingleLine().Grow(1).MinWidth(0)
		})
		for _, entry := range []struct{ kind, icon, label string }{
			{"references", "folder", "文件或路径…"},
			{"commands", "code-xml", "命令与 Skills…"},
		} {
			disabled := p.busy || p.inputBlocked()
			button := workspaceComposerMenuItem(c.Key(entry.kind), entry.label).Disabled(disabled).OnClick(func() {
				p.composer.addOpen = false
				p.openWorkspacePicker(entry.kind)
			})
			if disabled {
				button.TextColor(c.Theme().TextMuted)
			}
			button.Children(func() {
				ui.Icon(c, workspaceIcon(entry.icon)).Size(workspaceIconSize, workspaceIconSize).Shrink(0).TextColor(c.Theme().TextMuted)
				ui.Text(c, entry.label).FontSize(workspaceFontBody).LineHeight(1.5).SingleLine().Grow(1).MinWidth(0)
			})
		}
		p.buildWorkspacePlanMenuItem(c)
		disabled := p.Service.Goals == nil || target.EngineID != "alva" || p.busy || p.inputBlocked()
		goal := workspaceComposerMenuItem(c.Key("new-goal"), "New Goal").Disabled(disabled).
			Tooltip("目标引用由 Alva Agent 使用").OnClick(func() { p.composer.addOpen = false; p.openWorkspaceGoal() })
		if disabled {
			goal.TextColor(c.Theme().TextMuted)
		}
		goal.Children(func() {
			ui.Icon(c, workspaceIcon("target")).Size(workspaceIconSize, workspaceIconSize).Shrink(0).TextColor(c.Theme().TextMuted)
			ui.Text(c, "新建或选择目标").FontSize(workspaceFontBody).LineHeight(1.5).SingleLine().Grow(1).MinWidth(0)
		})
	})
}

func workspacePermissionPolicy(engineID string) (string, string) {
	switch engineID {
	case "alva":
		return "每次询问", "读取无需确认；写入、命令、网络及未知效果逐次询问。选择仅影响下一轮，不会立即执行。"
	default:
		return "引擎权限", "权限由此引擎的实际配置决定。当前界面不提供权限切换。"
	}
}

// workspaceEnginePermission describes a non-native engine's approval behaviour
// from its declared capability, never from its name: an engine whose runtime
// forwards permission requests asks through the approval card; any other runs
// under its own fixed limits and never asks.
func (p *NativePage) workspaceEnginePermission(engineID string) (string, string) {
	if p.Service != nil && p.Service.Turns != nil {
		if description, ok := p.Service.Turns.Engine(engineID); ok {
			if description.Execution.PermissionCallback {
				return "逐次审批", "此引擎的写入、命令和网络操作会在这里弹出审批卡，由你逐次批准。当前界面不提供提升权限。"
			}
			return "此引擎不会询问", "此引擎没有接入逐次审批：它按自带的固定限制自行运行，Alva 无法在执行前拦截或询问。当前界面不提供提升权限。"
		}
	}
	return workspacePermissionPolicy(engineID)
}

func (p *NativePage) buildWorkspacePermissionMenu(c *ui.Context, anchor ui.Element, target hub.Target) {
	if target.EngineID == "alva" {
		p.buildNativePermissionMenu(c, anchor)
		return
	}
	workspaceComposerPopover(c, anchor, &p.composer.permissionOpen, 320, func() {
		label, description := p.workspaceEnginePermission(target.EngineID)
		if p.workspacePlanEnabled() {
			label, description = "只读计划", p.workspacePlanDescription()
		}
		ui.Row(c).Role(ui.RoleMenuItemRadio).Label("Current permission policy").Checked(true).
			FillWidth().Padding(8, 12).AlignItems(ui.Center).Gap(8).Children(func() {
			ui.Icon(c, workspaceIcon("shield-check")).Size(16, 16).TextColor(c.Theme().Accent)
			ui.Text(c, label).FontSize(workspaceFontBody).Grow(1)
			ui.Icon(c, workspaceIcon("check")).Size(16, 16).TextColor(c.Theme().Accent)
		})
		ui.Text(c, description).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(c.Theme().TextMuted).Padding(8, 12)
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
			ui.Text(c, label+" · 不可用").FontSize(workspaceFontBody).TextColor(c.Theme().TextMuted)
			ui.Text(c, reason).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(c.Theme().TextMuted)
		})
	})
}
