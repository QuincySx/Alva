// INPUT: TurnService queue/interject/edit/regenerate/select-version, ConversationService versions/fork, the open session's observed run state and Alva Kit controls
// OUTPUT: Running-time Enter/Cmd+Enter handling, the queue bar above the composer, the version switcher, edit-and-resend editor, fork and regenerate actions and the fork-origin banner
// POS: [L7 Hub UI] Presentation only: every durable rule (limits, idle-only versions, attachment refusal, downgrade to the queue) is decided by the product services and reported back
//go:build desktop

package hubdesktop

import (
	"errors"
	"strconv"
	"strings"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	view "github.com/QuincySx/Alva/apps/ui/chat"
	"github.com/QuincySx/Alva/apps/ui/kit"
	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/egoist/mygo/ui"
)

const workspaceThreadReadEvery = 700 * time.Millisecond

// workspaceThreadState is confined to the UI thread like NativePage. It holds
// what the product services last reported for the open session.
type workspaceThreadState struct {
	session string
	// seenUpdated is the session's UpdatedAt the queue was last read at: a
	// queue only changes while the session does, apart from the user's own
	// actions, which refresh explicitly.
	seenUpdated int64
	readAt      time.Time
	reading     bool
	items       []hub.PendingInput
	caps        product.TurnCapabilities
	// disabled says why the whole conversation has no versions or forks.
	disabled string
	switches map[string]product.VersionSwitch
	// notice is an inline hint (an interjection that became a queued message,
	// a fork that carries only a summary); the user closes it.
	notice  string
	working bool
	// queue entry being edited
	queueEditID, queueEditText string
	queueEditFocus             bool
	queueEditInput             ui.Handle
	// user message being edited and resent as a new version
	editTurn, editText, editProblem string
	editFocus                       bool
	editInput                       ui.Handle
	// aroundSequence opens the next loaded session around that history
	// sequence (the link back from a fork) instead of at its end.
	aroundSequence uint64
}

// workspaceThreadReset forgets everything about the previous session.
func (p *NativePage) workspaceThreadReset() { p.thread = workspaceThreadState{} }

// observeWorkspaceThread refreshes the queue, versions and engine abilities
// of the open session, a few small reads at most every 700 ms.
func (p *NativePage) observeWorkspaceThread() {
	t := &p.thread
	if p.selected == "" || t.reading || time.Since(t.readAt) < workspaceThreadReadEvery {
		return
	}
	id, generation := p.selected, p.generation
	first := t.session != id
	activeLine := ""
	if p.detail.Session.ID == id {
		activeLine = p.detail.Session.ActiveLineID
		if branch, err := product.ActiveBranch(p.detail); err == nil {
			t.caps = p.Service.Turns.EngineCapabilities(branch.Target.EngineID)
		}
	}
	versioned := activeLine != ""
	updated := p.detail.Session.UpdatedAt
	readQueue := first || len(t.items) != 0 || p.workspaceComposerRunning() || updated != t.seenUpdated
	t.reading, t.readAt = true, time.Now()
	go func() {
		var items []hub.PendingInput
		var qerr, verr, derr error
		var switches map[string]product.VersionSwitch
		var disabled string
		if readQueue {
			items, qerr = p.Service.Turns.Queued(p.ctx, id)
		}
		if first {
			disabled, derr = p.Service.Conversations.VersionsDisabled(p.ctx, id)
		}
		if first || versioned {
			switches, verr = p.Service.Conversations.VersionsOf(p.ctx, id, activeLine)
		}
		p.dispatch(func() {
			t.reading = false
			if p.selected != id || p.generation != generation {
				return
			}
			if readQueue && qerr == nil {
				t.items = items
				t.seenUpdated = updated
			}
			if first {
				if derr == nil {
					t.disabled = disabled
				}
				t.session = id
			}
			if (first || versioned) && verr == nil {
				t.switches = switches
			} else if !versioned && !first {
				t.switches = nil
			}
		})
	}()
}

func (p *NativePage) workspaceThreadRefresh() { p.thread.readAt = time.Time{} }

// workspaceQueueProblem says why the draft cannot be queued or interjected
// right now ("" when it can). Files, Skills and references stay with a normal
// send: a queued message is plain text.
func (p *NativePage) workspaceQueueProblem() string {
	switch {
	case p.thread.working || p.busy:
		return "正在提交，请稍候。"
	case p.voice.recorder != nil:
		return "请先结束录音并确认文字，再发送。"
	case p.workspacePendingFiles() || len(p.captureWorkspaceComposerFiles()) != 0 || len(p.captureWorkspaceSelections()) != 0 || len(p.captureWorkspaceReferences()) != 0:
		return "带附件、技能或引用的消息暂不支持排队或插话，请等待当前任务完成后发送。"
	case strings.TrimSpace(p.model.Draft) == "":
		return "请输入消息。"
	case len(p.model.Draft) > hub.MaxPendingInputText:
		return "排队的消息最多 16 KiB，请缩短内容，或等任务完成后再发送。"
	}
	return ""
}

// workspaceRunningInput is true when Enter means "queue" and Cmd+Enter means
// "interject": an open session whose Turn runs and is not waiting on a person.
func (p *NativePage) workspaceRunningInput() bool {
	return p.selected != "" && p.detail.Session.ID == p.selected && p.workspaceComposerRunning() && p.detail.Session.Status != engine.Waiting && !p.workspaceComposerLoading()
}

// submitRunningDraft queues (Enter) or interjects (Cmd+Enter) the draft. The
// draft is cleared only after the service accepted it.
func (p *NativePage) submitRunningDraft(interject bool) {
	if problem := p.workspaceQueueProblem(); problem != "" {
		p.thread.notice = problem
		return
	}
	text, id := p.model.Draft, p.selected
	executionID, _ := p.Service.Turns.Active(id)
	p.thread.working, p.thread.notice = true, ""
	go func() {
		var notice string
		var err error
		if interject {
			var result product.InterjectResult
			result, err = p.Service.Turns.Interject(p.ctx, id, executionID, text)
			notice = result.Notice
		} else {
			_, err = p.Service.Turns.Queue(p.ctx, id, text)
		}
		p.dispatch(func() {
			p.thread.working = false
			if err != nil {
				switch {
				case errors.Is(err, hub.ErrPendingFull):
					p.thread.notice = "队列已满（最多 8 条），请先取消一些或等待发送。"
				case errors.Is(err, hub.ErrSessionNotRunning):
					p.thread.notice = "任务刚刚结束，请直接发送。草稿已保留。"
				default:
					p.thread.notice = err.Error()
				}
				return
			}
			if p.selected == id && p.model.Draft == text {
				p.model.Draft = ""
				delete(p.drafts, id)
				p.markWorkspaceDraftEdited()
			}
			p.thread.notice = notice
			p.workspaceThreadRefresh()
		})
	}()
}

func (p *NativePage) cancelQueued(item hub.PendingInput) {
	if p.thread.working {
		return
	}
	id := p.selected
	p.thread.working = true
	go func() {
		err := p.Service.Turns.CancelQueued(p.ctx, id, item.ID)
		p.dispatch(func() {
			p.thread.working = false
			if err != nil && !errors.Is(err, hub.ErrPendingGone) {
				p.thread.notice = err.Error()
			}
			p.workspaceThreadRefresh()
		})
	}()
}

func (p *NativePage) openQueueEdit(item hub.PendingInput) {
	t := &p.thread
	t.queueEditID, t.queueEditText, t.queueEditFocus = item.ID, item.Text, true
}

func (p *NativePage) saveQueueEdit() {
	t := &p.thread
	id, inputID, text := p.selected, t.queueEditID, t.queueEditText
	if t.working || inputID == "" {
		return
	}
	if err := hub.ValidPendingText(text); err != nil {
		t.notice = err.Error()
		return
	}
	t.working = true
	go func() {
		_, err := p.Service.Turns.EditQueued(p.ctx, id, inputID, text)
		p.dispatch(func() {
			t.working = false
			if err != nil {
				t.notice = "这条消息已经发出或被取消，无法再修改。"
			}
			t.queueEditID, t.queueEditText = "", ""
			p.workspaceThreadRefresh()
		})
	}()
}

// sendQueuedNow resumes a paused queue: the head becomes the next Turn.
func (p *NativePage) sendQueuedNow(inputID string) {
	t := &p.thread
	id := p.selected
	if t.working || p.busy || p.workspaceComposerRunning() {
		return
	}
	t.working = true
	go func() {
		_, err := p.Service.Turns.SendQueued(p.ctx, id, inputID)
		p.dispatch(func() {
			t.working = false
			if p.selected != id {
				return
			}
			if err != nil {
				t.notice = err.Error()
				if errors.Is(err, hub.ErrSessionBusy) {
					t.notice = "任务正在进行，完成后会自动发送。"
				}
				p.workspaceThreadRefresh()
				return
			}
			p.problem = ""
			p.model.ActiveRunID = id
			p.model.Status = view.Running
			p.chat.ScrollToEnd()
			p.poll()
			p.refreshSessions()
			p.workspaceThreadRefresh()
		})
	}()
}

func queueStateBadge(state hub.PendingState) (string, kit.BadgeTone) {
	switch state {
	case hub.PendingInterjecting:
		return "插话中", kit.BadgeAccent
	case hub.PendingUncertain:
		return "可能已送达", kit.BadgeWarning
	}
	return "排队中", kit.BadgeNeutral
}

// buildWorkspaceQueue is the queue bar above the editor: every queued or
// interjected message with its state, edit and cancel, the paused-queue
// notice with its explicit send, and the inline hint.
func (p *NativePage) buildWorkspaceQueue(c *ui.Context) {
	t := &p.thread
	if p.selected == "" || t.session != p.selected {
		return
	}
	running := p.workspaceComposerRunning()
	if len(t.items) == 0 && t.notice == "" {
		return
	}
	ui.Column(c.Key("queue-bar")).Label("Queued messages").FillWidth().Gap(4).Children(func() {
		if t.notice != "" {
			kit.Notice(c, kit.NoticeProps{Tone: kit.BadgeInfo, Title: t.notice, Key: "queue-notice", OnClose: func() { t.notice = "" }})
		}
		for i, item := range t.items {
			p.buildWorkspaceQueueRow(c, i, item, running)
		}
		if len(t.items) == 0 {
			return
		}
		switch {
		case running:
			hint := "任务完成后会依次发送。Enter 排队。"
			if t.caps.Interject {
				hint = "任务完成后会依次发送。Enter 排队，⌘Enter 插话。"
			}
			kit.Text(c, hint, kit.TextProps{Role: kit.RoleCaption, Tone: kit.ToneSecondary, Key: "queue-hint"})
		case p.detail.Session.Status == engine.Succeeded && t.items[0].State == hub.PendingQueued:
			// The last Turn succeeded: the head starts by itself in a moment.
			kit.Text(c, "正在发送队首…", kit.TextProps{Role: kit.RoleCaption, Tone: kit.ToneSecondary, Key: "queue-sending"})
		case t.items[0].State != hub.PendingInterjecting:
			ui.Row(c.Key("queue-paused")).FillWidth().AlignItems(ui.Center).Gap(8).Children(func() {
				kit.Text(c, "队列已暂停（上一轮失败、被停止，或应用刚重新打开）。", kit.TextProps{Role: kit.RoleCaption, Tone: kit.ToneSecondary, Key: "queue-paused-text"})
				kit.Button(c, "发送队首", kit.ButtonProps{Key: "queue-send-head", Kind: kit.Primary, Disabled: t.working || p.busy, OnClick: func() { p.sendQueuedNow("") }})
			})
		}
	})
}

func (p *NativePage) buildWorkspaceQueueRow(c *ui.Context, index int, item hub.PendingInput, running bool) {
	t := &p.thread
	key := "queue-" + item.ID
	if t.queueEditID == item.ID {
		ui.Column(c.Key(key)).FillWidth().Gap(4).Children(func() {
			area := kit.TextArea(c, &t.queueEditText, kit.TextAreaProps{Label: "Edit queued message", MinLines: 1, MaxLines: 4, Key: "amend"})
			if t.queueEditFocus {
				t.queueEditInput.Focus(c)
				t.queueEditFocus = false
			}
			area.Bind(&t.queueEditInput)
			ui.Row(c.Key("edit-actions")).Gap(8).Children(func() {
				kit.Button(c, "取消", kit.ButtonProps{Key: "cancel", OnClick: func() { t.queueEditID, t.queueEditText = "", "" }})
				kit.Button(c, "保存", kit.ButtonProps{Key: "save", Kind: kit.Primary, Disabled: t.working, OnClick: p.saveQueueEdit})
			})
		})
		return
	}
	label, tone := queueStateBadge(item.State)
	editable := item.State == hub.PendingQueued
	removable := item.State != hub.PendingInterjecting
	kit.ListRow(c, item.Text, kit.ListRowProps{Key: key, Description: item.Error, Leading: func() {
		kit.Badge(c, label, kit.BadgeProps{Tone: tone, Key: "state"})
	}, Trailing: func() {
		kit.IconButton(c, icon.Edit, "修改排队消息", kit.IconButtonProps{Key: "amend", Disabled: !editable || t.working, Tooltip: pickReason(map[bool]string{false: "已交给模型，不能修改"}[editable], "修改这条排队消息"), OnClick: func() { p.openQueueEdit(item) }})
		kit.IconButton(c, icon.Close, "取消排队", kit.IconButtonProps{Key: "cancel", Disabled: !removable || t.working, Tooltip: pickReason(map[bool]string{false: "已交给模型，不能取消"}[removable], "取消这条消息"), OnClick: func() { p.cancelQueued(item) }})
		if !running && item.State != hub.PendingInterjecting {
			kit.IconButton(c, icon.ArrowUp, "现在发送", kit.IconButtonProps{Key: "send", Disabled: t.working || p.busy, Tooltip: "现在发送这条消息", OnClick: func() { p.sendQueuedNow(item.ID) }})
		}
	}})
	_ = index
}

// ---- versions, edit, fork, regenerate ----------------------------------

// workspaceReloadTranscript drops the cached window and reads the conversation
// again: after a version change the history behind it is another set of events.
func (p *NativePage) workspaceReloadTranscript() {
	p.generation++
	p.resetTranscript()
	p.model.Messages = nil
	p.chat.Busy = true
	p.polling = false
	p.workspaceThreadRefresh()
	p.loadTail()
}

// workspaceVersionReason says why a user message cannot be edited or forked
// ("" when it can): the conversation kind, an attachment or Skill, or a
// running Turn.
func (p *NativePage) workspaceVersionReason(block product.Block) string {
	switch {
	case p.thread.disabled != "":
		return p.thread.disabled
	case block.Rich:
		return "带附件或技能的轮次不支持编辑、重新生成和分叉"
	case p.workspaceComposerRunning() || p.busy:
		return "任务进行中，结束后可编辑或分叉"
	}
	return ""
}

func (p *NativePage) startVersionEdit(block product.Block) {
	t := &p.thread
	t.editTurn, t.editText, t.editProblem, t.editFocus = block.TurnID, block.Text, "", true
}

func (p *NativePage) submitVersionEdit() {
	t := &p.thread
	id, turn, text := p.selected, t.editTurn, t.editText
	if t.working || turn == "" {
		return
	}
	if err := hub.ValidTurnText(text); err != nil {
		t.editProblem = "请输入要重新发送的内容（最多 64 KiB）。"
		return
	}
	t.working = true
	go func() {
		_, err := p.Service.Turns.Edit(p.ctx, id, turn, text)
		p.dispatch(func() {
			t.working = false
			if p.selected != id {
				return
			}
			if err != nil {
				t.editProblem = p.versionError(err)
				return
			}
			t.editTurn, t.editText, t.editProblem = "", "", ""
			p.model.ActiveRunID, p.model.Status = id, view.Running
			p.workspaceReloadTranscript()
			p.poll()
			p.refreshSessions()
		})
	}()
}

func (p *NativePage) versionError(err error) string {
	switch {
	case errors.Is(err, hub.ErrVersionBusy), errors.Is(err, hub.ErrSessionBusy):
		return "任务进行中，结束后才能编辑、重新生成或切换版本。"
	case errors.Is(err, hub.ErrVersionBudget):
		return "这个任务的版本已满（最多 64 个）。"
	case errors.Is(err, hub.ErrVersionUnsupported):
		return err.Error()
	}
	return err.Error()
}

func (p *NativePage) regenerateReply(turnID string) {
	t := &p.thread
	id := p.selected
	if t.working || p.busy || p.workspaceComposerRunning() {
		return
	}
	t.working = true
	go func() {
		_, err := p.Service.Turns.Regenerate(p.ctx, id, turnID)
		p.dispatch(func() {
			t.working = false
			if p.selected != id {
				return
			}
			if err != nil {
				p.problem = p.versionError(err)
				return
			}
			p.problem = ""
			p.model.ActiveRunID, p.model.Status = id, view.Running
			p.workspaceReloadTranscript()
			p.poll()
			p.refreshSessions()
		})
	}()
}

func (p *NativePage) selectVersion(lineID string) {
	t := &p.thread
	id := p.selected
	if t.working || p.busy || p.workspaceComposerRunning() {
		return
	}
	t.working = true
	go func() {
		err := p.Service.Turns.SelectVersion(p.ctx, id, lineID)
		p.dispatch(func() {
			t.working = false
			if p.selected != id {
				return
			}
			if err != nil {
				p.problem = p.versionError(err)
				return
			}
			p.problem = ""
			p.workspaceReloadTranscript()
		})
	}()
}

func (p *NativePage) forkFromTurn(turnID string) {
	t := &p.thread
	id := p.selected
	if t.working || p.busy || p.workspaceComposerRunning() {
		return
	}
	t.working = true
	go func() {
		result, err := p.Service.Conversations.Fork(p.ctx, id, turnID)
		p.dispatch(func() {
			t.working = false
			if p.selected != id {
				return
			}
			if err != nil {
				p.problem = p.versionError(err)
				if errors.Is(err, hub.ErrSessionRunning) {
					p.problem = "任务进行中，结束后才能分叉。"
				}
				return
			}
			p.problem = ""
			p.refreshSessions()
			p.refreshWorkspaceNavigation()
			p.selectSession(result.Session.ID)
			if result.Degraded {
				p.thread.notice = "此引擎不能原生分叉，模型只看到摘要"
			}
		})
	}()
}

// openForkOrigin shows the conversation a fork came from, around the place it
// was forked.
func (p *NativePage) openForkOrigin() {
	origin := p.detail.Session.ForkedFrom
	if origin == nil || p.busy {
		return
	}
	p.pendingAround = origin.EventSequence
	p.selectSession(origin.SessionID)
}

// buildWorkspaceForkBanner is the line at the top of a forked conversation.
func (p *NativePage) buildWorkspaceForkBanner(c *ui.Context) {
	origin := p.detail.Session.ForkedFrom
	if origin == nil || p.detail.Session.ID != p.selected {
		return
	}
	title := origin.Title
	if title == "" {
		title = "原对话"
	}
	ui.Row(c.Key("fork-banner")).Label("Forked conversation").FillWidth().AlignItems(ui.Center).Gap(8).Padding(0, 0, 8, 0).Shrink(0).Children(func() {
		kit.Text(c, "从「"+title+"」分叉", kit.TextProps{Role: kit.RoleCaption, Tone: kit.ToneSecondary, SingleLine: true, Key: "text"})
		kit.Button(c, "查看分叉前的对话", kit.ButtonProps{Key: "open", Kind: kit.Ghost, Icon: icon.Branch, OnClick: p.openForkOrigin})
	})
}

// buildWorkspaceUserActions is the line under a user message: the version
// switcher (always shown when versions exist) and the hover actions.
func (p *NativePage) buildWorkspaceUserActions(c *ui.Context, block product.Block, hovered bool) {
	t := &p.thread
	switcher, versioned := t.switches[block.TurnID]
	reason := p.workspaceVersionReason(block)
	idle := !p.workspaceComposerRunning() && !p.busy && !t.working
	ui.Row(c.Key("user-actions-" + block.ID)).Label("User message actions").Gap(4).AlignItems(ui.Center).Justify(ui.End).Children(func() {
		if versioned {
			p.buildWorkspaceVersionSwitch(c, switcher, idle)
		}
		visible := hovered || reason != "" && hovered
		show := func(button ui.Element) {
			button.IgnoreInput(!visible)
			if !visible {
				button.Opacity(0)
			}
		}
		edit := kit.IconButton(c, icon.Edit, "编辑并重新发送", kit.IconButtonProps{Key: "amend", Disabled: reason != "", Tooltip: pickReason(reason, "编辑并重新发送：旧版本会保留"), OnClick: func() { p.startVersionEdit(block) }})
		show(edit)
		fork := kit.IconButton(c, icon.Branch, "从这里分叉", kit.IconButtonProps{Key: "fork", Disabled: reason != "", Tooltip: pickReason(reason, "从这条消息分出一个新任务，不复制历史"), OnClick: func() { p.forkFromTurn(block.TurnID) }})
		show(fork)
	})
}

func pickReason(reason, fallback string) string {
	if reason != "" {
		return reason
	}
	return fallback
}

// buildWorkspaceVersionSwitch draws "‹ 2/3 ›". Only an idle conversation can
// switch; the arrows then name the version they open.
func (p *NativePage) buildWorkspaceVersionSwitch(c *ui.Context, s product.VersionSwitch, idle bool) {
	reason := "任务进行中，结束后可切换版本"
	ui.Row(c.Key("versions-" + s.TurnID)).Label("Message versions").AlignItems(ui.Center).Gap(0).Children(func() {
		previous, next := s.Index-2, s.Index
		previousTip, nextTip := "上一个版本", "下一个版本"
		if !idle {
			previousTip, nextTip = reason, reason
		}
		kit.IconButton(c, icon.ChevronLeft, "上一个版本", kit.IconButtonProps{Key: "previous", Disabled: previous < 0 || !idle, Tooltip: previousTip, OnClick: func() { p.selectVersion(s.Choices[previous].LineID) }})
		kit.Text(c, strconv.Itoa(s.Index)+"/"+strconv.Itoa(s.Count), kit.TextProps{Role: kit.RoleCaption, Tone: kit.ToneSecondary, Key: "count"})
		kit.IconButton(c, icon.ChevronRight, "下一个版本", kit.IconButtonProps{Key: "next", Disabled: next >= len(s.Choices) || !idle, Tooltip: nextTip, OnClick: func() { p.selectVersion(s.Choices[next].LineID) }})
	})
}

// buildWorkspaceVersionEditor replaces the user's bubble while it is edited.
func (p *NativePage) buildWorkspaceVersionEditor(c *ui.Context, block product.Block) {
	t := &p.thread
	ui.Column(c.Key("version-edit-" + block.ID)).Label("Edit and resend").FillWidth().MaxWidth(560).Gap(8).Children(func() {
		area := kit.TextArea(c, &t.editText, kit.TextAreaProps{Label: "Edited message", MinLines: 2, MaxLines: 8, Problem: t.editProblem, Key: "text"})
		if t.editFocus {
			t.editInput.Focus(c)
			t.editFocus = false
		}
		area.Bind(&t.editInput)
		kit.Text(c, "重新发送后，这条消息及其后的内容会成为一个新版本；旧版本保留，可随时切换回来。", kit.TextProps{Role: kit.RoleCaption, Tone: kit.ToneSecondary, Key: "note"})
		ui.Row(c.Key("actions")).Gap(8).Justify(ui.End).Children(func() {
			kit.Button(c, "取消", kit.ButtonProps{Key: "cancel", OnClick: func() { t.editTurn, t.editText, t.editProblem = "", "", "" }})
			kit.Button(c, "重新发送", kit.ButtonProps{Key: "resend", Kind: kit.Primary, Busy: t.working, OnClick: p.submitVersionEdit})
		})
	})
}

// buildWorkspaceReplyActions adds "重新生成" under the last reply of an idle
// conversation.
func (p *NativePage) buildWorkspaceReplyActions(c *ui.Context, block product.Block) {
	if !p.workspaceLastReply(block) {
		return
	}
	reason := p.thread.disabled
	if reason == "" && (p.workspaceComposerRunning() || p.busy) {
		reason = "任务进行中，结束后可重新生成"
	}
	if reason == "" {
		if user, ok := p.workspaceUserBlockOf(block.TurnID); ok && user.Rich {
			reason = "带附件或技能的轮次不支持重新生成"
		}
	}
	kit.Button(c, "重新生成", kit.ButtonProps{Key: "regenerate-" + block.ID, Kind: kit.Ghost, Icon: icon.Refresh, Disabled: reason != "" || p.thread.working, Tooltip: pickReason(reason, "再问一次，生成一个新版本；旧回复会保留"), OnClick: func() { p.regenerateReply(block.TurnID) }})
}

// workspaceLastReply is true for the newest reply of a conversation whose
// newest Turn finished and that shows its own end.
func (p *NativePage) workspaceLastReply(block product.Block) bool {
	if block.Kind != product.BlockKindReply || block.Status != product.BlockStatusCompleted || p.transcript.MoreAfter() || p.detail.Session.ID != p.selected {
		return false
	}
	if p.detail.Session.Status != engine.Succeeded {
		return false
	}
	for i := len(p.transcriptRows) - 1; i >= 0; i-- {
		other, ok := p.transcriptBlocks[p.transcriptRows[i].ID]
		if ok && other.Kind == product.BlockKindReply {
			return other.ID == block.ID
		}
	}
	return false
}

func (p *NativePage) workspaceUserBlockOf(turnID string) (product.Block, bool) {
	for _, row := range p.transcriptRows {
		if block, ok := p.transcriptBlocks[row.ID]; ok && block.Kind == product.BlockKindUser && block.TurnID == turnID && block.Name == "" {
			return block, true
		}
	}
	return product.Block{}, false
}

// workspaceScrollToSequence brings the last block committed at or before a
// history sequence into view: where a fork link lands in the source.
func (p *NativePage) workspaceScrollToSequence(sequence uint64) {
	target := -1
	for i, row := range p.transcriptRows {
		if block, ok := p.transcriptBlocks[row.ID]; ok && block.History != 0 && block.History <= sequence {
			target = i
		}
	}
	if target < 0 {
		return
	}
	p.chat.ScrollToRow(target)
}
