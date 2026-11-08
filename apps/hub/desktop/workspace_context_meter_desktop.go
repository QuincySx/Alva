// INPUT: The transcript window's latest measured context usage, product manual compaction intents and engine capability, MyGo base controls
// OUTPUT: Composer context meter chip and its composition popover with 「压缩」, plus the Message Stream compaction status line
// POS: [L7 Hub UI] Presents what the engine measured; never estimates a total or a window it was not given.
//go:build desktop

package hubdesktop

import (
	"strconv"

	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/egoist/mygo/ui"
)

// workspaceContextWarnRatio is when the meter warns: the default automatic
// compaction trigger, so the warning means "the next request compacts".
const workspaceContextWarnRatio = 0.8

type workspaceContextMeter struct {
	usage              *engine.ContextUsage
	pending, supported bool
}

func (p *NativePage) workspaceContextMeter() workspaceContextMeter {
	if p.selected == "" || p.detail.Session.ID != p.selected || p.transcript == nil {
		return workspaceContextMeter{}
	}
	return workspaceContextMeter{usage: p.transcript.Context(), pending: p.Service.ContextCompactionPending(p.selected), supported: p.Service.ContextCompactionSupported(p.detail)}
}

// ratio is the used share of a declared window; 0 when undeclared.
func (m workspaceContextMeter) ratio() float64 {
	if m.usage == nil || m.usage.WindowTokens == 0 {
		return 0
	}
	return float64(m.usage.UsedTokens()) / float64(m.usage.WindowTokens)
}

// label is the chip text: the pending intent, a percentage of a declared
// window, or only the measured token count. Empty hides the chip.
func (m workspaceContextMeter) label() string {
	switch {
	case m.pending:
		return "下次发送前压缩"
	case m.usage == nil:
		return ""
	case m.usage.WindowTokens > 0:
		return "上下文 " + strconv.FormatUint(uint64(m.ratio()*100+0.5), 10) + "%"
	}
	return "上下文 " + compactTokens(m.usage.UsedTokens())
}

// compactTokens writes a count the way the chip has room for: exact below ten
// thousand, then 万 and 亿 with one decimal.
func compactTokens(n uint64) string {
	switch {
	case n < 10_000:
		return formatTokens(n)
	case n < 100_000_000:
		return strconv.FormatFloat(float64(n)/10_000, 'f', 1, 64) + "万"
	}
	return strconv.FormatFloat(float64(n)/100_000_000, 'f', 1, 64) + "亿"
}

func (p *NativePage) buildWorkspaceContextMeter(c *ui.Context) {
	meter := p.workspaceContextMeter()
	text := meter.label()
	if text == "" {
		p.composer.contextOpen = false
		return
	}
	t := c.Theme()
	icon := "database"
	warn := meter.ratio() >= workspaceContextWarnRatio
	switch {
	case meter.pending:
		icon = "file-text"
	case warn:
		icon = "triangle-alert"
	}
	chip := workspaceComposerChip(c.Key("context-meter"), icon, text, "Context usage").Shrink(0).
		Expanded(p.composer.contextOpen).OnClick(func() {
		p.closeWorkspacePicker()
		p.composer.contextOpen = !p.composer.contextOpen
		p.composer.addOpen, p.composer.permissionOpen, p.composer.targetOpen = false, false, false
	})
	switch {
	case meter.pending:
		chip.TextColor(t.Accent).Tooltip("已请求压缩：下次发送时先把早期对话浓缩成摘要。")
	case warn:
		chip.TextColor(t.Warning).Tooltip("上下文接近上限，下次请求前会自动压缩早期对话。")
	default:
		chip.Tooltip("最近一次请求的实际上下文用量，点开查看构成。")
	}
	workspaceContextPopover(c, chip, &p.composer.contextOpen, 320, func() { p.buildWorkspaceContextDetails(c, meter) })
}

// workspaceContextPopover opens above the chip, aligned to its right edge so
// it stays inside the composer on the right side of the control row.
func workspaceContextPopover(c *ui.Context, anchor ui.Element, open *bool, width float32, content func()) {
	_, height := c.Size()
	bounds := anchor.Bounds()
	available := max(0, max(bounds.Y, height-bounds.Y-bounds.H)-10)
	ui.PopoverBase(c, anchor, open, func(panel ui.Element) {
		panel.Label("Context usage details").AttachTo(anchor, ui.AnchorTopRight, ui.AnchorBottomRight).Width(width).MaxWidthPercent(95).MaxHeight(available).Margin(0, 0, 6, 0).Padding(12).Gap(8).
			Radius(16).Background(c.Theme().Surface).
			Shadow(0, 8, 30, -6, ui.RGBA(28, 32, 55, 0.17)).DrawOver(func(p *ui.Painter, r ui.Rect) {
			p.Stroke(r, c.Theme().Border, 16, 1)
		})
		ui.Scroll(c).Label("Context usage contents").Column().FillWidth().Gap(8).MaxHeight(max(0, available-24)).Children(content)
	})
}

func (p *NativePage) buildWorkspaceContextDetails(c *ui.Context, meter workspaceContextMeter) {
	t := c.Theme()
	caption := func(text string) {
		ui.Text(c, text).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(t.TextMuted)
	}
	ui.Text(c, "上下文").FontSize(workspaceFontSubtitle).FontWeight(600).LineHeight(1.5)
	if usage := meter.usage; usage != nil {
		used := usage.UsedTokens()
		summary := "已用 " + formatTokens(used) + " tokens · 模型窗口未声明"
		if usage.WindowTokens > 0 {
			summary = "已用 " + strconv.FormatUint(uint64(meter.ratio()*100+0.5), 10) + "% · " + formatTokens(used) + " / " + formatTokens(usage.WindowTokens) + " tokens"
		}
		ui.Text(c, summary).FontSize(workspaceFontBody).LineHeight(1.5)
		if usage.WindowTokens > 0 {
			ratio := float32(min(meter.ratio(), 1))
			fill := t.Accent
			if meter.ratio() >= workspaceContextWarnRatio {
				fill = t.Warning
			}
			ui.Box(c).Label("Context usage bar").FillWidth().Height(6).Radius(3).Background(workspacePalette(c).hover).Draw(func(painter *ui.Painter, r ui.Rect) {
				painter.Fill(ui.Rect{X: r.X, Y: r.Y, W: r.W * ratio, H: r.H}, fill, 3)
			})
		}
		if parts := usage.Parts; parts != nil {
			for _, part := range []struct {
				label  string
				tokens uint64
			}{{"系统", parts.System}, {"工具定义", parts.Tools}, {"摘要", parts.Summary}, {"最近消息", parts.Recent}} {
				ui.Row(c.Key("part-" + part.label)).Label("Context part " + part.label).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
					ui.Text(c, part.label).FontSize(workspaceFontBody).LineHeight(1.5).Grow(1)
					ui.Text(c, formatTokens(part.tokens)).FontSize(workspaceFontBody).LineHeight(1.5).TextColor(t.TextMuted)
				})
			}
			caption("构成按最近一次请求的实际总数等比估算。")
		}
		request := "最近一次请求：输入 " + formatTokens(usage.InputTokens)
		if usage.CachedInputTokens != nil {
			request += "（缓存 " + formatTokens(*usage.CachedInputTokens) + "）"
		}
		caption(request + " · 输出 " + formatTokens(usage.OutputTokens))
		if meter.ratio() >= workspaceContextWarnRatio {
			ui.Text(c, "接近上限，下次请求前会自动压缩早期对话。").FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(t.Warning)
		}
	}
	if !meter.supported {
		return
	}
	session := p.selected
	if meter.pending {
		caption("下次发送时会先把早期对话浓缩成摘要，原始记录保留。")
		p.workspaceControlAction(c, "context-compact-cancel", "取消压缩", "x", "secondary", false, func() {
			p.Service.CancelContextCompaction(session)
		}).Label("Cancel context compaction")
		return
	}
	caption("把早期对话浓缩成摘要，保留最近的原文；在下次发送时执行。")
	p.workspaceControlAction(c, "context-compact", "压缩", "file-text", "secondary", false, func() {
		go func() {
			err := p.Service.RequestContextCompaction(p.ctx, session)
			p.dispatch(func() {
				if err != nil && p.selected == session {
					p.problem = "无法请求压缩：" + err.Error()
				}
			})
		}()
	}).Label("Compact context before the next request")
}

// workspaceContextStatusText is the Message Stream line for a compaction block.
func workspaceContextStatusText(block product.Block) string {
	c := block.Context
	if c == nil {
		return block.Text
	}
	switch c.Status {
	case engine.ContextCompacted:
		text := "已压缩早期对话 · 覆盖 " + strconv.FormatUint(c.Messages, 10) + " 条消息（记录 #" + strconv.FormatUint(c.FromSequence, 10) + "–#" + strconv.FormatUint(c.ThroughSequence, 10) + "）"
		if c.Trigger == "manual" {
			text += " · 手动"
		}
		if c.Trigger == "overflow" {
			text += " · 超出上限后重试"
		}
		return text
	case engine.ContextCompactionSkipped:
		return "无需压缩：早期对话还不足以摘要"
	case engine.ContextSummaryCorrupt:
		return "摘要记录损坏，本次按近期对话截断"
	}
	return "压缩失败，本次按近期对话截断"
}

func (p *NativePage) buildWorkspaceContextStatus(c *ui.Context, row string, block product.Block) {
	t := c.Theme()
	icon, color := "file-text", t.TextMuted
	if block.Status == product.BlockStatusFailed {
		icon, color = "triangle-alert", t.Warning
	}
	binding := p.transcriptBinding()
	ui.Row(c).Label("Context status " + row).FillWidth().Gap(12).AlignItems(ui.Center).Children(func() {
		ui.Box(c).Width(32).Shrink(0).AlignItems(ui.Center).Justify(ui.Center).Children(func() {
			ui.Icon(c, workspaceIcon(icon)).Size(14, 14).TextColor(color)
		})
		ui.Text(c, workspaceContextStatusText(block)).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(color).Grow(1).MinWidth(0)
		if block.Ref != nil && block.Ref.Sequence != 0 {
			ui.ButtonBase(c.Key("open")).Label("Open summary record "+row).Padding(2, 6).Radius(6).TextColor(t.TextMuted).
				FontSize(workspaceFontCaption).Tooltip("在执行轨迹中打开这条记录").OnClick(func() { p.openContextRecord(binding, row) }).Children(func() {
				ui.Text(c, "查看记录").FontSize(workspaceFontCaption).LineHeight(1.5)
			})
		}
	})
}

// openContextRecord opens the trace of the native branch that owns the
// compaction's record, around that record.
func (p *NativePage) openContextRecord(binding workspaceTranscriptBinding, row string) {
	block, ok := p.transcriptBlocks[row]
	if binding != p.transcriptBinding() || !ok || block.Ref == nil || block.SessionID != p.selected {
		return
	}
	branchID := ""
	for _, branch := range p.detail.Branches {
		if branch.SessionID == block.SessionID && branch.NativeSessionID != "" && branch.NativeSessionID == block.Ref.SessionID {
			branchID = branch.ID
		}
	}
	if branchID == "" {
		p.problem = "这条压缩记录没有可用的原始分支。"
		return
	}
	target := product.TraceTarget{SessionID: binding.session, BranchID: branchID}
	p.trace.active = true
	if !sameTraceTarget(p.trace.target, target) || len(p.trace.frames) > 0 {
		p.trace.frames = nil
	}
	p.openTrace(target, block.Ref.Sequence, product.TraceHint{Turn: block.Turn, History: block.History})
}
