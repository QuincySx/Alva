// INPUT: Explicit microphone intent, owned system-local speech recognition and page/session draft identity.
// OUTPUT: Independent recording Stop/Cancel, joined cleanup and bounded draft-only transcript adoption.
// POS: [L7 Hub UI] Voice never sends, stops an Agent, changes session or replaces an edited draft.
//go:build desktop

package hubdesktop

import (
	"context"
	"fmt"
	"strings"
	"time"
	"unicode/utf8"

	view "github.com/QuincySx/Alva/apps/ui/chat"
	"github.com/QuincySx/Alva/platform/system/speech"
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

type workspaceVoiceState struct {
	start                               func(context.Context, speech.Options) (speech.Recorder, error)
	recorder                            speech.Recorder
	sessionID, baseDraft                string
	generation                          uint64
	discard, stopRequested, deferResult bool
	pending, pendingSession, problem    string
	startFailure                        string
}

func (p *NativePage) startWorkspaceVoice() {
	p.observeWorkspaceVoice()
	v := &p.voice
	if v.recorder != nil || v.pending != "" {
		return
	}
	start := v.start
	if start == nil {
		start = speech.Start
	}
	recorder, err := start(p.ctx, speech.Options{MaximumDuration: time.Minute})
	if err != nil {
		v.problem = err.Error()
		if recorder != nil {
			v.startFailure = err.Error()
			v.recorder, v.discard, v.stopRequested = recorder, true, true
			recorder.Cancel()
		}
		return
	}
	if recorder == nil {
		v.problem = "本地语音录入未能启动。"
		return
	}
	v.recorder, v.sessionID, v.baseDraft = recorder, p.selected, p.model.Draft
	v.generation = p.generation
	v.discard, v.stopRequested, v.deferResult, v.problem = false, false, false, ""
	v.startFailure = ""
}

func workspaceVoiceMerge(base, text string) (string, bool) {
	result := base
	if result != "" && !strings.HasSuffix(result, "\n") {
		result += "\n"
	}
	result += text
	return result, len(result) <= view.MaxDraftBytes && utf8.ValidString(result)
}

// Observation is cold and nonblocking. The owner remains installed until Done
// and Close both confirm real native cleanup, including delayed OS permission callbacks.
func (p *NativePage) observeWorkspaceVoice() {
	v := &p.voice
	if v.recorder == nil {
		return
	}
	select {
	case <-v.recorder.Done():
	default:
		return
	}
	snapshot := v.recorder.Snapshot()
	ctx, cancel := context.WithTimeout(context.Background(), time.Millisecond)
	err := v.recorder.Close(ctx)
	cancel()
	if err != nil {
		v.problem = "本地语音仍在收尾，请稍候再试。"
		return
	}
	v.recorder = nil
	if v.discard || snapshot.State == speech.Cancelled {
		v.problem = v.startFailure
		return
	}
	if snapshot.State != speech.Completed {
		v.problem = snapshot.Problem
		if v.problem == "" {
			v.problem = "本地语音识别未完成，请重试。"
		}
		return
	}
	text := strings.TrimSpace(snapshot.Text)
	if text == "" {
		v.problem = "未识别到语音，请重试。"
		return
	}
	if len(text) > view.MaxDraftBytes || !utf8.ValidString(text) {
		v.problem = "语音文字超过 32 KiB 或格式无效，请分段录入。"
		return
	}
	v.problem = ""
	merged, fits := workspaceVoiceMerge(v.baseDraft, text)
	if !v.deferResult && p.selected == v.sessionID && p.generation == v.generation && p.model.Draft == v.baseDraft && fits {
		p.model.Draft = merged
		v.problem = ""
		return
	}
	v.pending, v.pendingSession = text, v.sessionID
	if !fits {
		v.problem = "合并后超过 32 KiB；请缩短原会话草稿，再明确采纳语音文字。"
	}
}

func (p *NativePage) applyWorkspaceVoice() {
	v := &p.voice
	if !p.workspaceVoicePendingHere() {
		return
	}
	merged, fits := workspaceVoiceMerge(p.model.Draft, v.pending)
	if !fits {
		v.problem = "合并后超过 32 KiB；请缩短草稿后重试。"
		return
	}
	p.model.Draft = merged
	v.pending, v.pendingSession, v.problem = "", "", ""
}

func (p *NativePage) workspaceVoicePendingHere() bool {
	v := &p.voice
	return v.pending != "" && p.selected == v.pendingSession
}

// The first Home Send may create a durable session before its later admission
// fails. Bind only that still-current Home draft; an unrelated future Home is
// never a destination for an older recording or retained transcript.
func (p *NativePage) bindWorkspaceVoiceHome(id string) {
	v := &p.voice
	if id == "" || p.selected != "" {
		return
	}
	if v.pending != "" && v.pendingSession == "" {
		v.pendingSession = id
		if v.sessionID == "" {
			v.sessionID = id
		}
	}
	if v.recorder != nil && v.sessionID == "" && v.generation == p.generation {
		v.sessionID = id
		v.deferResult = true
	}
}

func (p *NativePage) cancelWorkspaceVoice() {
	v := &p.voice
	if v.recorder == nil {
		return
	}
	v.discard, v.stopRequested = true, true
	v.recorder.Cancel()
}

// Navigation ends microphone capture but retains any final text for the original
// session. Explicit cancellation and window closure discard an active recording.
func (p *NativePage) finishWorkspaceVoiceForNavigation() {
	v := &p.voice
	if v.recorder == nil {
		return
	}
	v.deferResult = true
	if !v.stopRequested {
		v.stopRequested = true
		v.recorder.Stop()
	}
}

func (p *NativePage) guardWorkspaceVoiceClose(event *mygo.CloseEvent) bool {
	p.cancelWorkspaceVoice()
	p.observeWorkspaceVoice()
	if p.voice.recorder == nil {
		return false
	}
	event.PreventDefault()
	p.voice.problem = "正在关闭麦克风和本地转写，请等待实际收尾后再关闭窗口。"
	return true
}

func (p *NativePage) buildWorkspaceVoice(c *ui.Context) {
	v := &p.voice
	if v.recorder == nil {
		workspaceComposerIconButton(c.Key("dictation"), "mic", "Start dictation", "系统本地语音转文字；仅写入草稿，不自动发送", workspaceIconControlSize).
			Disabled(v.pending != "" || !speech.Supported() && v.start == nil).OnClick(p.startWorkspaceVoice)
		return
	}
	snapshot := v.recorder.Snapshot()
	label := "准备语音…"
	switch snapshot.State {
	case speech.Requesting:
		label = "等待权限…"
	case speech.Recording:
		elapsed := 0
		if !snapshot.StartedAt.IsZero() {
			elapsed = min(60, max(0, int(time.Since(snapshot.StartedAt).Seconds())))
		}
		label = fmt.Sprintf("%d:%02d", elapsed/60, elapsed%60)
	case speech.Transcribing:
		label = "转写中"
	default:
		label = "语音收尾…"
	}
	if v.discard {
		label = "取消录音…"
	} else if v.stopRequested && snapshot.State == speech.Recording {
		label = "正在停止…"
	}
	ui.ButtonBase(c.Key("dictation-recording")).Label("Stop dictation").Description(label).Height(workspaceIconControlSize).MinWidth(0).MaxWidth(160).Padding(0, 12).Gap(8).Radius(16).
		AlignItems(ui.Center).Background(workspacePalette(c).subtle).
		Tooltip("结束录音并取得文字；不会停止 Agent 任务").OnClick(func() {
		if !v.stopRequested {
			v.stopRequested = true
			v.recorder.Stop()
		}
	}).Children(func() {
		if snapshot.State == speech.Recording && !v.stopRequested {
			ui.Box(c).Size(8, 8).Shrink(0).Radius(4).Background(c.Theme().Danger)
		} else {
			p.workspaceLoadingIcon(c.Key("activity"), 16, c.Theme().TextMuted)
		}
		ui.Text(c, label).Label("Dictation status").Role(ui.RoleStatus).MinWidth(0).FontSize(workspaceFontBody).LineHeight(1.5).SingleLine()
	})
}

func (p *NativePage) buildWorkspaceVoiceNotice(c *ui.Context) {
	v := &p.voice
	if v.recorder == nil && v.pending == "" && v.problem == "" {
		return
	}
	ui.Column(c.Key("dictation-notice")).Label("Dictation notice").FillWidth().MinWidth(0).Gap(8).Children(func() {
		if v.recorder != nil {
			ui.Row(c.Key("dictation-cancel")).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
				ui.Text(c, "系统本地识别 · 最长 60 秒").FontSize(workspaceFontCaption).TextColor(c.Theme().TextMuted).Grow(1).MinWidth(0).SingleLine()
				workspaceControlButton(c, "cancel-dictation", "取消录音", "ghost", v.discard).Label("Cancel dictation").OnClick(p.cancelWorkspaceVoice)
			})
		}
		if v.pending != "" {
			label := "语音文字保留待采纳，不会覆盖当前草稿。"
			if p.selected != v.pendingSession {
				label = "语音文字属于原会话；请返回原会话后采纳。"
			}
			ui.Text(c, label).Label("Pending dictation scope").FillWidth().FontSize(workspaceFontCaption).TextColor(c.Theme().TextMuted)
			ui.Row(c.Key("dictation-adoption")).FillWidth().Gap(8).Children(func() {
				workspaceControlButton(c, "apply-dictation", "采纳到草稿", "secondary", !p.workspaceVoicePendingHere()).Label("Apply dictation text").OnClick(p.applyWorkspaceVoice)
				workspaceControlButton(c, "discard-dictation", "丢弃文字", "ghost").Label("Discard dictation text").OnClick(func() { v.pending, v.pendingSession, v.problem = "", "", "" })
			})
			ui.Scroll(c.Key("dictation-text")).FillWidth().MinWidth(0).MaxHeight(72).Children(func() {
				ui.Text(c, v.pending).Label("Pending dictation text").FillWidth().MinWidth(0).FontSize(workspaceFontBody)
			})
		}
		if v.problem != "" {
			ui.Text(c, v.problem).Label("Dictation error").FillWidth().FontSize(workspaceFontCaption).TextColor(c.Theme().Danger)
		}
	})
}
