// INPUT: The sidebar's session rows, ConversationService rename/pin/delete and TurnService.Continue
// OUTPUT: Row action menu, rename and delete-confirm dialogs, the pinned group and the stopped-reply Continue bar
// POS: [L7 Hub UI] Management goes through the product services only; the UI never reads or deletes product rows itself
//go:build desktop

package hubdesktop

import (
	"errors"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	view "github.com/QuincySx/Alva/apps/ui/chat"
	"github.com/QuincySx/Alva/apps/ui/kit"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/egoist/mygo/ui"
)

// sessionOpsState is confined to the UI thread like NativePage.
type sessionOpsState struct {
	pinned []hub.Session
	// rename dialog
	renameOpen    bool
	renameID      string
	renameText    string
	renameProblem string
	renameFocus   bool
	renameInput   ui.Handle
	// delete confirmation
	deleteOpen    bool
	deleteID      string
	deleteTitle   string
	deleteProblem string
	working       bool
}

// sessionRunning is true when the sidebar's observed status says the session
// still owns an execution; the service refuses a delete then as well.
func (p *NativePage) sessionRunning(session hub.Session) bool {
	status := session.Status
	if session.ID == p.selected && p.detail.Session.ID == session.ID && p.detail.Session.UpdatedAt >= session.UpdatedAt {
		status = p.detail.Session.Status
	}
	return hub.BlocksInput(status)
}

func (p *NativePage) loadPinnedSessions() {
	go func() {
		pinned, err := p.Service.Conversations.Pinned(p.ctx)
		p.dispatch(func() {
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.sessionOps.pinned = pinned
		})
	}()
}

func (p *NativePage) openSessionRename(session hub.Session) {
	o := &p.sessionOps
	o.renameOpen, o.renameFocus = true, true
	o.renameID, o.renameText, o.renameProblem = session.ID, session.Title, ""
}

func (p *NativePage) closeSessionRename() {
	o := &p.sessionOps
	o.renameOpen, o.renameFocus, o.renameProblem = false, false, ""
}

func (p *NativePage) submitSessionRename() {
	o := &p.sessionOps
	if o.working || !o.renameOpen {
		return
	}
	id, text := o.renameID, o.renameText
	if _, err := product.RenameTitle(text); err != nil {
		o.renameProblem = err.Error()
		return
	}
	o.working = true
	go func() {
		err := p.Service.Conversations.Rename(p.ctx, id, text)
		p.dispatch(func() {
			o.working = false
			if err != nil {
				o.renameProblem = err.Error()
				return
			}
			p.closeSessionRename()
			p.refreshWorkspaceNavigation()
			p.loadPinnedSessions()
			if p.selected == id {
				p.poll()
			}
		})
	}()
}

func (p *NativePage) toggleSessionPinned(session hub.Session) {
	o := &p.sessionOps
	if o.working {
		return
	}
	o.working = true
	go func() {
		err := p.Service.Conversations.SetPinned(p.ctx, session.ID, !session.Pinned)
		p.dispatch(func() {
			o.working = false
			if err != nil {
				p.problem = err.Error()
				if errors.Is(err, hub.ErrPinnedBudget) {
					p.problem = "置顶已满（最多 32 个），请先取消一些置顶。"
				}
				return
			}
			p.refreshWorkspaceNavigation()
			p.loadPinnedSessions()
		})
	}()
}

func (p *NativePage) openSessionDelete(session hub.Session) {
	o := &p.sessionOps
	if p.sessionRunning(session) {
		p.problem = "任务正在运行，请先停止后再删除。"
		return
	}
	o.deleteOpen, o.deleteID, o.deleteTitle, o.deleteProblem = true, session.ID, session.Title, ""
}

func (p *NativePage) closeSessionDelete() {
	o := &p.sessionOps
	o.deleteOpen, o.deleteProblem = false, ""
}

func (p *NativePage) confirmSessionDelete() {
	o := &p.sessionOps
	if o.working || !o.deleteOpen {
		return
	}
	id := o.deleteID
	o.working = true
	go func() {
		err := p.Service.Conversations.Delete(p.ctx, id)
		p.dispatch(func() {
			o.working = false
			if err != nil {
				o.deleteProblem = err.Error()
				if errors.Is(err, hub.ErrSessionRunning) {
					o.deleteProblem = "任务正在运行，请先停止后再删除。"
				}
				if errors.Is(err, hub.ErrSessionHasWorkers) {
					o.deleteProblem = "任务还有运行中的协作 Worker，请先停止它们再删除。"
				}
				return
			}
			p.closeSessionDelete()
			if p.selected == id {
				p.startWorkspaceTask() // leave the hidden session
			}
			p.refreshWorkspaceNavigation()
			p.loadPinnedSessions()
		})
	}()
}

// buildSessionMenu is the row's action menu, shared by the "更多" button and
// the right-click context menu.
func (p *NativePage) buildSessionMenu(m *ui.Menu, session hub.Session) {
	if m.Item("重命名").Disabled(p.sessionOps.working).Chosen() {
		p.openSessionRename(session)
	}
	label := "置顶"
	if session.Pinned {
		label = "取消置顶"
	}
	if m.Item(label).Disabled(p.sessionOps.working).Chosen() {
		p.toggleSessionPinned(session)
	}
	m.Separator()
	if m.Item("删除…").Disabled(p.sessionOps.working || p.sessionRunning(session)).Chosen() {
		p.openSessionDelete(session)
	}
}

func (p *NativePage) buildSessionRenameDialog(c *ui.Context) {
	o := &p.sessionOps
	if !o.renameOpen {
		return
	}
	kit.Dialog(c, &o.renameOpen, kit.DialogProps{Label: "Rename session dialog"}, func() {
		kit.DialogHeader(c, "重命名任务", "", func() { p.closeSessionRename() })
		input := workspaceControlInput(c, "session-rename-input", &o.renameText).Label("Session title").
			Placeholder("任务名称").FillWidth().Bind(&o.renameInput)
		input.OnSubmit(p.submitSessionRename)
		if o.renameFocus {
			o.renameInput.Focus(c)
			o.renameFocus = false
		}
		if o.renameProblem != "" {
			ui.Text(c, o.renameProblem).Label("Rename problem").Role(ui.RoleStatus).FontSize(workspaceFontCaption).TextColor(c.Theme().Danger)
		}
		kit.DialogFooter(c, func() {
			kit.Button(c, "取消", kit.ButtonProps{Key: "session-rename-cancel", OnClick: p.closeSessionRename})
			kit.Button(c, "保存", kit.ButtonProps{Key: "session-rename-save", Kind: kit.Primary, Disabled: o.working, OnClick: p.submitSessionRename})
		})
	})
}

func (p *NativePage) buildSessionDeleteDialog(c *ui.Context) {
	o := &p.sessionOps
	if !o.deleteOpen {
		return
	}
	kit.Dialog(c, &o.deleteOpen, kit.DialogProps{Label: "Delete session dialog"}, func() {
		kit.DialogHeader(c, "删除任务？", "「"+o.deleteTitle+"」会从列表、搜索和通知中消失，数据仍保留在本机。", func() { p.closeSessionDelete() })
		if o.deleteProblem != "" {
			ui.Text(c, o.deleteProblem).Label("Delete problem").Role(ui.RoleStatus).FontSize(workspaceFontCaption).TextColor(c.Theme().Danger)
		}
		kit.DialogFooter(c, func() {
			kit.Button(c, "取消", kit.ButtonProps{Key: "session-delete-cancel", OnClick: p.closeSessionDelete})
			kit.Button(c, "删除", kit.ButtonProps{Key: "session-delete-confirm", Kind: kit.Danger, Disabled: o.working, OnClick: p.confirmSessionDelete})
		})
	})
}

// workspaceStoppedReply reports whether the open session's newest Turn was
// stopped and nothing else is going on, the state in which Continue is offered.
// The service validates again, so a late click cannot continue a different Turn.
func (p *NativePage) workspaceStoppedReply() bool {
	return p.selected != "" && p.detail.Session.ID == p.selected && p.detail.Session.Status == engine.Cancelled &&
		!p.busy && !p.chat.Busy && !p.transcript.MoreAfter() && !p.workspaceComposerRunning()
}

func (p *NativePage) continueStopped() {
	id := p.selected
	if !p.workspaceStoppedReply() {
		return
	}
	p.generation++
	p.busy, p.chat.Busy = true, true
	go func() {
		_, err := p.Service.Turns.Continue(p.ctx, id)
		p.dispatch(func() {
			p.busy, p.chat.Busy = false, false
			if p.selected != id {
				return
			}
			if err != nil {
				p.problem = err.Error()
				if errors.Is(err, product.ErrNothingToContinue) {
					p.problem = "上一条回复不是被停止的回复，无法继续。"
				}
				return
			}
			p.problem = ""
			p.model.ActiveRunID = id
			p.model.Status = view.Running
			p.chat.ScrollToEnd()
			p.poll()
			p.refreshSessions()
		})
	}()
}

func (p *NativePage) buildWorkspaceContinue(c *ui.Context) {
	if !p.workspaceStoppedReply() {
		return
	}
	t := c.Theme()
	ui.Row(c.Key("continue-bar")).Label("Stopped reply actions").FillWidth().MaxWidth(760).AlignSelf(ui.Center).Gap(8).AlignItems(ui.Center).Padding(8, 8).Shrink(0).Children(func() {
		ui.Text(c, "上次回复已停止，已生成的内容已保留。").Label("Stopped reply notice").FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(t.TextMuted)
		workspaceControlButton(c, "continue-stopped", "继续", "secondary").Label("Continue stopped reply").
			Tooltip("接着上次停止的位置继续生成").OnClick(p.continueStopped)
	})
}
