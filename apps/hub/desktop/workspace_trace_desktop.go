// INPUT: Product TraceService pages/details/search, TraceWindow steps, LayoutTrace geometry and MyGo chart, list and markdown controls
// OUTPUT: Conversation/trace view switch, swim-lane timeline with scale choice, virtual step list with paging and error back-off, step detail tabs with sequence-addressed raw records, literal search jumps and child enter/return
// POS: [L7 Hub UI] Execution-trace presentation of the middle window; reads only through the product TraceService and never starts execution
//go:build desktop

package hubdesktop

import (
	"fmt"
	"strconv"
	"strings"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/chart"
	"github.com/egoist/mygo/ui/markdown"
)

// workspaceTraceFrame is a parent trace kept while a child session is shown;
// returning restores it as it was, without another read.
type workspaceTraceFrame struct {
	target   product.TraceTarget
	window   *product.TraceWindow
	selected string
}

type workspaceTraceState struct {
	active     bool
	target     product.TraceTarget
	generation uint64
	window     *product.TraceWindow
	steps      []product.TraceStep
	layout     product.TraceLayout
	scale      product.TraceScale
	selected   string
	list       ui.ListState
	loading    bool
	epoch      uint64
	problem    string
	frames     []workspaceTraceFrame
	live       bool
	// followPending asks for one more live Newer once the read in flight
	// ends, so the refresh after a run stops is never skipped.
	followPending bool
	// pageFailed stops scroll-driven paging after an error until the user
	// acts or the next poll tick.
	pageFailed bool
	// version counts step-list replacements; built is the version the list
	// was last laid out with, so scroll paging never reads stale rows.
	version, built uint64

	// selectEpoch names the current selection; detailEpoch the current detail
	// read; detailShape the step state that read was made for.
	detail      product.TraceDetail
	detailBusy  bool
	selectEpoch uint64
	detailEpoch uint64
	detailShape string
	tab         string

	rawSeq  uint64
	rawURI  string
	rawText string
	rawNext int64
	rawMore bool
	rawBusy bool

	query       string
	searchQuery string
	search      product.TraceSearchPage
	searchHits  []product.TraceHit
	searchBusy  bool
	searchEpoch uint64

	markdown *markdown.Options
	// childJump is a tool record a pending jump selects and then enters the
	// child session of (an agent call's card).
	childJump uint64
	// windowLimit overrides the TraceWindow byte limit (tests); 0 = default.
	windowLimit int
}

func (p *NativePage) resetTrace() {
	old := p.trace
	// Epochs only move forward, so no read started before the reset matches.
	p.trace = workspaceTraceState{active: old.active, markdown: old.markdown, windowLimit: old.windowLimit,
		epoch: old.epoch + 1, selectEpoch: old.selectEpoch + 1, detailEpoch: old.detailEpoch + 1, searchEpoch: old.searchEpoch + 1}
}

func (p *NativePage) showTrace() {
	if p.selected == "" {
		return
	}
	p.trace.active = true
	if p.trace.window == nil || p.trace.generation != p.generation || p.trace.target.SessionID != p.selected {
		p.openDefaultTrace()
	}
}

func (p *NativePage) showConversation() { p.trace.active = false }

func (p *NativePage) openDefaultTrace() {
	if p.selected == "" || p.detail.Session.ID != p.selected || p.detail.Session.ActiveBranchID == "" {
		return
	}
	p.trace.frames = nil
	p.openTrace(product.TraceTarget{SessionID: p.selected, BranchID: p.detail.Session.ActiveBranchID}, 0, product.TraceHint{})
}

// traceCurrent reports whether an asynchronous result still belongs to the
// trace that requested it.
func (p *NativePage) traceCurrent(session string, generation, epoch uint64) bool {
	return p.selected == session && p.generation == generation && p.trace.epoch == epoch
}

// settleTraceRead ends the page read started at epoch and reports whether its
// result may be applied. A read superseded by a newer one leaves loading to
// that read. Otherwise loading ends here even when the result is dropped; a
// result captured before an explicit edit moved the generation (Send, Stop,
// upload) reloads the trace around its current position, as the conversation
// reloads its tail.
func (p *NativePage) settleTraceRead(session string, generation, epoch uint64) bool {
	if p.trace.epoch != epoch || p.selected != session {
		return false
	}
	p.trace.loading = false
	if p.generation != generation {
		p.reloadTrace()
		return false
	}
	return true
}

// reloadTrace reopens the shown target, around the selected step if any.
func (p *NativePage) reloadTrace() {
	focus := uint64(0)
	if step, ok := p.traceSelectedStep(); ok {
		focus = step.First
	}
	p.openTrace(p.trace.target, focus, product.TraceHint{})
}

func (p *NativePage) clearTraceSearch() {
	p.trace.search, p.trace.searchHits, p.trace.searchQuery, p.trace.searchBusy = product.TraceSearchPage{}, nil, "", false
	p.trace.searchEpoch++
}

// openTrace shows target, selecting the step folding in record focus when it
// is set. A focus already inside the shown window is selected without a read.
func (p *NativePage) openTrace(target product.TraceTarget, focus uint64, hint product.TraceHint) {
	if p.Service.Traces == nil {
		p.trace.problem = "执行轨迹读取服务不可用。"
		return
	}
	childJump := p.trace.childJump
	p.trace.childJump = 0
	if focus == 0 || childJump != focus {
		childJump = 0
	}
	same := p.trace.window != nil && p.trace.generation == p.generation && sameTraceTarget(p.trace.target, target)
	if same && focus != 0 {
		if step, ok := p.trace.window.StepAt(focus); ok {
			p.selectTraceStep(step.ID)
			p.revealTraceStep(step.ID)
			if childJump != 0 {
				p.enterTraceChildOf(step)
			}
			return
		}
	}
	p.trace.epoch++
	p.trace.target, p.trace.generation = target, p.generation
	p.trace.loading, p.trace.problem, p.trace.pageFailed = true, "", false
	if !same {
		p.trace.window, p.trace.steps, p.trace.layout = nil, nil, product.TraceLayout{}
		p.clearTraceSearch()
	}
	p.clearTraceSelection()
	session, generation, epoch := p.selected, p.generation, p.trace.epoch
	go func() {
		var chunk product.TraceChunk
		var err error
		if focus == 0 {
			chunk, err = p.Service.Traces.Tail(p.ctx, target)
		} else {
			chunk, err = p.Service.Traces.Around(p.ctx, target, focus, hint)
		}
		p.dispatch(func() {
			if !p.settleTraceRead(session, generation, epoch) {
				return
			}
			defer p.resumeTraceFollow()
			if err != nil {
				p.trace.problem = err.Error()
				return
			}
			window := product.NewTraceWindow(p.trace.windowLimit)
			window.Reset(chunk)
			window.Trim(focus == 0)
			p.trace.window = window
			p.refreshTraceSteps()
			if focus != 0 {
				if step, ok := window.StepAt(focus); ok {
					p.selectTraceStep(step.ID)
					p.revealTraceStep(step.ID)
					if childJump != 0 {
						p.enterTraceChildOf(step)
					}
				}
				return
			}
			p.trace.list.ScrollToEnd()
		})
	}()
}

func sameTraceTarget(a, b product.TraceTarget) bool {
	if a.SessionID != b.SessionID || a.BranchID != b.BranchID || len(a.Path) != len(b.Path) {
		return false
	}
	for i := range a.Path {
		if a.Path[i] != b.Path[i] {
			return false
		}
	}
	return true
}

func (p *NativePage) refreshTraceSteps() {
	p.trace.version++
	if p.trace.window == nil {
		p.trace.steps, p.trace.layout = nil, product.TraceLayout{}
		return
	}
	p.trace.steps = p.trace.window.Steps()
	p.trace.layout = product.LayoutTrace(p.trace.steps, p.traceScale())
	if p.trace.selected == "" {
		return
	}
	step, ok := p.trace.window.Step(p.trace.selected)
	if !ok {
		p.clearTraceSelection()
		return
	}
	// An open call that paired, or gained records, re-reads its detail.
	if traceStepShape(step) != p.trace.detailShape {
		p.loadTraceDetail(step)
	}
}

func traceStepShape(step product.TraceStep) string {
	return fmt.Sprint(step.Open, step.Status, step.Last, len(step.Records))
}

func (p *NativePage) traceScale() product.TraceScale {
	if p.trace.scale == "" {
		return product.TraceScaleDuration
	}
	return p.trace.scale
}

func (p *NativePage) setTraceScale(scale product.TraceScale) {
	p.trace.scale = scale
	p.trace.layout = product.LayoutTrace(p.trace.steps, scale)
}

// traceOlder prepends the previous page; the keyed list keeps the rows in view.
func (p *NativePage) traceOlder() {
	w := p.trace.window
	if w == nil || p.trace.loading || !w.MoreBefore() || p.Service.Traces == nil {
		return
	}
	p.trace.loading = true
	session, generation, epoch, target, cursor := p.selected, p.generation, p.trace.epoch, p.trace.target, w.Cursor()
	go func() {
		chunk, err := p.Service.Traces.Older(p.ctx, target, cursor)
		p.dispatch(func() {
			if !p.settleTraceRead(session, generation, epoch) || p.trace.window != w {
				return
			}
			defer p.resumeTraceFollow()
			if err != nil {
				p.trace.problem, p.trace.pageFailed = err.Error(), true
				return
			}
			w.ApplyOlder(chunk)
			w.Trim(false)
			p.refreshTraceSteps()
		})
	}()
}

// traceNewer appends the next page: live following of a window that holds the
// newest records, or paging forward (more) through a window that does not.
func (p *NativePage) traceNewer(more bool) {
	w := p.trace.window
	if w == nil || p.trace.loading || w.MoreAfter() != more || w.Unsupported() || p.Service.Traces == nil {
		return
	}
	if !more {
		p.trace.followPending = false
	}
	p.trace.loading = true
	session, generation, epoch, target, cursor := p.selected, p.generation, p.trace.epoch, p.trace.target, w.Cursor()
	atEnd := p.trace.list.AtEnd()
	go func() {
		chunk, err := p.Service.Traces.Newer(p.ctx, target, cursor)
		p.dispatch(func() {
			if !p.settleTraceRead(session, generation, epoch) || p.trace.window != w {
				return
			}
			defer p.resumeTraceFollow()
			if err != nil {
				p.trace.problem, p.trace.pageFailed = err.Error(), true
				return
			}
			w.ApplyNewer(chunk)
			w.Trim(true)
			p.refreshTraceSteps()
			if atEnd {
				p.trace.list.ScrollToEnd()
			}
		})
	}()
}

// pollTrace refreshes a visible trace while its session runs, and once more
// after it stops so the final records appear.
func (p *NativePage) pollTrace() {
	p.trace.pageFailed = false // a new tick allows one more scroll-driven page
	if !p.trace.active || p.trace.window == nil || p.selected == "" || p.trace.target.SessionID != p.selected {
		return
	}
	running := p.detail.Session.ID == p.selected && p.detail.Session.Status == hub.Running
	if running || p.trace.live {
		p.trace.live = running
		p.trace.followPending = true
		p.resumeTraceFollow()
	}
}

// resumeTraceFollow issues the pending live read once no read is in flight.
func (p *NativePage) resumeTraceFollow() {
	w := p.trace.window
	if !p.trace.followPending || p.trace.loading || w == nil {
		return
	}
	if w.MoreAfter() || w.Unsupported() {
		p.trace.followPending = false // not following the newest records
		return
	}
	p.traceNewer(false)
}

// loadEarlierTrace is the explicit action; it also ends an error back-off.
func (p *NativePage) loadEarlierTrace() {
	p.trace.pageFailed = false
	p.traceOlder()
}

// followTraceScroll loads earlier steps when the list reaches its top and
// later ones when a window that ends before the newest record reaches its end.
func (p *NativePage) followTraceScroll() {
	w := p.trace.window
	if w == nil || p.trace.loading || p.trace.pageFailed || len(p.trace.steps) == 0 || p.trace.built != p.trace.version {
		return
	}
	first, last := p.trace.list.Visible()
	if last < first {
		return
	}
	if first < 3 && w.MoreBefore() {
		p.traceOlder()
		return
	}
	if w.MoreAfter() && last >= len(p.trace.steps)-3 {
		p.traceNewer(true)
	}
}

func (p *NativePage) clearTraceSelection() {
	p.trace.selected = ""
	p.trace.detail, p.trace.detailBusy, p.trace.detailShape = product.TraceDetail{}, false, ""
	p.trace.selectEpoch++
	p.trace.detailEpoch++
	p.trace.rawSeq, p.trace.rawURI, p.trace.rawText, p.trace.rawNext, p.trace.rawMore, p.trace.rawBusy = 0, "", "", 0, false, false
}

func (p *NativePage) revealTraceStep(id string) {
	for i, step := range p.trace.steps {
		if step.ID == id {
			p.trace.list.ScrollTo(i, ui.Center)
			return
		}
	}
}

// selectTraceStep selects a step and reads its bodies for the detail panel.
func (p *NativePage) selectTraceStep(id string) {
	w := p.trace.window
	if w == nil {
		return
	}
	step, ok := w.Step(id)
	if !ok {
		return
	}
	p.clearTraceSelection()
	p.trace.selected = id
	if p.trace.tab == "" {
		p.trace.tab = "overview"
	}
	p.loadTraceDetail(step)
}

// loadTraceDetail (re)reads the selected step's detail for its current shape.
func (p *NativePage) loadTraceDetail(step product.TraceStep) {
	p.trace.detailEpoch++
	p.trace.detailBusy, p.trace.detailShape = true, traceStepShape(step)
	session, generation, epoch, detailEpoch, target, id := p.selected, p.generation, p.trace.epoch, p.trace.detailEpoch, p.trace.target, step.ID
	go func() {
		detail, err := p.Service.Traces.Detail(p.ctx, target, step)
		p.dispatch(func() {
			if p.trace.detailEpoch != detailEpoch || p.trace.selected != id {
				return // a later read or selection owns detailBusy
			}
			p.trace.detailBusy = false
			if p.selected != session || p.trace.epoch != epoch {
				return
			}
			if p.generation != generation {
				if current, ok := p.traceSelectedStep(); ok {
					p.loadTraceDetail(current)
				}
				return
			}
			if err != nil {
				p.trace.problem = err.Error()
				return
			}
			p.trace.detail = detail
		})
	}()
}

func (p *NativePage) traceSelectedStep() (product.TraceStep, bool) {
	if p.trace.window == nil || p.trace.selected == "" {
		return product.TraceStep{}, false
	}
	return p.trace.window.Step(p.trace.selected)
}

// readTraceRaw reads one of the selected step's records, identified by its
// sequence, in 2 KiB pages.
func (p *NativePage) readTraceRaw(sequence uint64, more bool) {
	step, ok := p.traceSelectedStep()
	if !ok || p.trace.rawBusy {
		return
	}
	var record product.TraceRecord
	for _, candidate := range step.Records {
		if candidate.Sequence == sequence {
			record = candidate
		}
	}
	if record.Sequence == 0 || record.URI == "" {
		return
	}
	offset := int64(0)
	if more && p.trace.rawSeq == sequence {
		offset = p.trace.rawNext
	} else {
		p.trace.rawText, p.trace.rawNext, p.trace.rawMore = "", 0, false
	}
	p.trace.rawSeq, p.trace.rawURI, p.trace.rawBusy = sequence, record.URI, true
	session, generation, epoch, selectEpoch, target := p.selected, p.generation, p.trace.epoch, p.trace.selectEpoch, p.trace.target
	go func() {
		page, err := p.Service.Traces.Raw(p.ctx, target, record.URI, offset)
		p.dispatch(func() {
			if p.trace.selectEpoch != selectEpoch || p.trace.rawSeq != sequence {
				return // a later selection or record owns rawBusy
			}
			p.trace.rawBusy = false
			if !p.traceCurrent(session, generation, epoch) {
				return // stale page; the user can read it again
			}
			if err != nil {
				p.trace.problem = err.Error()
				return
			}
			p.trace.rawText += page.Text
			p.trace.rawNext, p.trace.rawMore = page.NextOffset, page.Truncated
		})
	}()
}

// enterTraceChild keeps the parent trace and opens the child session's.
func (p *NativePage) enterTraceChild(step product.TraceStep) {
	child, err := p.trace.target.Child(step)
	if err != nil {
		p.trace.problem = err.Error()
		return
	}
	p.trace.frames = append(p.trace.frames, workspaceTraceFrame{target: p.trace.target, window: p.trace.window, selected: p.trace.selected})
	p.trace.window = nil
	p.openTrace(child, 0, product.TraceHint{})
}

// enterTraceChildOf enters the child session an agent call's selected tool
// step started; its link record names the step's intent as parent.
func (p *NativePage) enterTraceChildOf(step product.TraceStep) {
	if p.trace.window == nil {
		return
	}
	child, ok := p.trace.window.ChildStepOf(step.First)
	if !ok {
		p.trace.problem = "没有找到这次调用的子会话记录。"
		return
	}
	p.enterTraceChild(child)
}

func (p *NativePage) returnTraceParent() {
	if len(p.trace.frames) == 0 {
		return
	}
	frame := p.trace.frames[len(p.trace.frames)-1]
	p.trace.frames = p.trace.frames[:len(p.trace.frames)-1]
	p.trace.epoch++
	p.trace.loading, p.trace.problem, p.trace.pageFailed = false, "", false
	p.trace.target, p.trace.window = frame.target, frame.window
	p.clearTraceSearch()
	p.refreshTraceSteps()
	p.clearTraceSelection()
	if frame.selected != "" {
		p.selectTraceStep(frame.selected)
		p.revealTraceStep(frame.selected)
	}
}

// searchTrace pages a literal search; continuing appends the next page.
func (p *NativePage) searchTrace(more bool) {
	query := strings.TrimSpace(p.trace.query)
	if p.trace.searchBusy || p.Service.Traces == nil || query == "" && !more {
		return
	}
	after, high := uint64(0), uint64(0)
	if more {
		query, after, high = p.trace.searchQuery, p.trace.search.Next, p.trace.search.HighWater
	} else {
		p.trace.searchHits = nil
	}
	p.trace.searchBusy, p.trace.searchQuery = true, query
	p.trace.searchEpoch++
	session, token, target := p.selected, p.trace.searchEpoch, p.trace.target
	go func() {
		page, err := p.Service.Traces.Search(p.ctx, target, query, after, high)
		p.dispatch(func() {
			if p.trace.searchEpoch != token {
				return // results were cleared or replaced; that path ended searchBusy
			}
			p.trace.searchBusy = false
			// Hits are committed facts of the target; a reload of the same
			// target keeps them.
			if p.selected != session || !sameTraceTarget(p.trace.target, target) {
				return
			}
			if err != nil {
				p.trace.problem = err.Error()
				return
			}
			p.trace.search = page
			p.trace.searchHits = append(p.trace.searchHits, page.Hits...)
		})
	}()
}

func (p *NativePage) jumpTraceHit(hit product.TraceHit) {
	p.openTrace(p.trace.target, hit.Sequence, product.TraceHint{})
}

// --- presentation ---

func traceKindLabel(kind product.TraceStepKind) string {
	switch kind {
	case product.TraceStepInput:
		return "用户"
	case product.TraceStepModel:
		return "模型"
	case product.TraceStepTool:
		return "工具"
	case product.TraceStepChild:
		return "子 Agent"
	}
	return string(kind)
}

func traceStatusLabel(status string) string {
	switch status {
	case "running":
		return "进行中"
	case "succeeded":
		return "成功"
	case "failed":
		return "失败"
	case "cancelled":
		return "已取消"
	case "":
		return "—"
	}
	return status
}

func traceDuration(d time.Duration) string {
	switch {
	case d <= 0:
		return ""
	case d < time.Second:
		return strconv.FormatInt(d.Milliseconds(), 10) + " ms"
	case d < time.Minute:
		return strconv.FormatFloat(d.Seconds(), 'f', 1, 64) + " s"
	}
	return d.Round(time.Second).String()
}

// traceElapsedLabel names a timeline tick by its offset from the window start.
func traceElapsedLabel(d time.Duration) string {
	if d <= 0 {
		return "0"
	}
	return "+" + traceDuration(d)
}

func traceClock(at time.Time) string {
	if at.IsZero() {
		return "—"
	}
	return at.Local().Format("2006-01-02 15:04:05.000")
}

func traceStepTitle(step product.TraceStep) string {
	turn := "第 ? 轮"
	if step.Turn != 0 {
		turn = "第 " + strconv.FormatUint(step.Turn, 10) + " 轮"
	}
	if step.Ordinal == 0 {
		return turn
	}
	if !step.OrdinalExact {
		return turn + " · 窗口内第 " + strconv.Itoa(step.Ordinal) + " 步"
	}
	return turn + " · 第 " + strconv.Itoa(step.Ordinal) + " 步"
}

func traceLaneColor(c *ui.Context, step product.TraceStep) ui.Color {
	t := c.Theme()
	if step.Status == "failed" || step.Status == "cancelled" {
		return t.Danger
	}
	switch step.Kind {
	case product.TraceStepInput:
		return t.Accent
	case product.TraceStepModel:
		return t.Success
	case product.TraceStepChild:
		return t.Warning
	}
	return t.TextMuted
}

// buildWorkspaceViewSwitch is the header's conversation/trace choice.
func (p *NativePage) buildWorkspaceViewSwitch(c *ui.Context) {
	if p.selected == "" || p.pane != "" {
		return
	}
	t := c.Theme()
	ui.Row(c.Key("workspace-view-switch")).Label("Middle window view").Gap(2).Padding(2).Radius(8).
		Background(workspacePalette(c).subtle).Shrink(0).Children(func() {
		for _, choice := range []struct {
			key, text, label string
			trace            bool
		}{{"view-conversation", "对话", "Show conversation", false}, {"view-trace", "轨迹", "Show execution trace", true}} {
			selected := p.trace.active == choice.trace
			button := ui.ButtonBase(c.Key(choice.key)).Label(choice.label).Selected(selected).Height(24).Padding(0, 10).
				Radius(6).AlignItems(ui.Center).FontSize(workspaceFontBody).TextColor(t.TextMuted).FocusRing(false)
			if selected {
				button.Background(t.Surface).TextColor(t.Text)
			} else if button.Hovered() {
				button.TextColor(t.Text)
			}
			trace := choice.trace
			button.OnClick(func() {
				if trace {
					p.showTrace()
				} else {
					p.showConversation()
				}
			}).Children(func() { ui.Text(c, choice.text).FontSize(workspaceFontBody).LineHeight(1.5).SingleLine() })
		}
	})
}

func (p *NativePage) buildWorkspaceTrace(c *ui.Context) {
	if p.trace.window == nil && !p.trace.loading && p.trace.problem == "" {
		p.openDefaultTrace()
	}
	p.followTraceScroll()
	t := c.Theme()
	ui.Column(c.Key("workspace-trace")).Label("Execution trace").Grow(1).MinHeight(0).FillWidth().Padding(16, 0, 0, 0).Gap(12).Children(func() {
		p.buildTraceToolbar(c)
		if p.trace.problem != "" {
			ui.Text(c, p.trace.problem).Role(ui.RoleStatus).FontSize(workspaceFontBody).TextColor(t.Danger)
		}
		w := p.trace.window
		switch {
		case w != nil && w.Unsupported():
			ui.Text(c, "当前分支的引擎没有提供执行轨迹，只能查看对话。").FontSize(workspaceFontBody).TextColor(t.TextMuted)
			return
		case w == nil:
			ui.Text(c, "正在读取执行记录…").FontSize(workspaceFontBody).TextColor(t.TextMuted)
			return
		case len(p.trace.steps) == 0 && !w.MoreBefore():
			ui.Text(c, "这个会话还没有执行记录。").FontSize(workspaceFontBody).TextColor(t.TextMuted)
			return
		}
		p.buildTraceSearchResults(c)
		p.buildTraceTimeline(c)
		ui.Row(c).Grow(1).MinHeight(0).FillWidth().Gap(16).AlignItems(ui.Stretch).Children(func() {
			ui.Column(c).Label("Trace steps").Basis(0).Grow(1).MinWidth(0).MinHeight(0).Gap(8).Children(func() { p.buildTraceSteps(c) })
			ui.Column(c).Label("Trace step detail").Basis(0).Grow(1).MinWidth(0).MinHeight(0).Gap(8).Children(func() { p.buildTraceDetail(c) })
		})
	})
}

func (p *NativePage) buildTraceToolbar(c *ui.Context) {
	t := c.Theme()
	ui.Row(c).Label("Trace toolbar").FillWidth().Gap(8).AlignItems(ui.Center).Wrap().Children(func() {
		if len(p.trace.frames) > 0 {
			workspaceControlIconButton(c, "trace-parent", "返回父会话", "chevron-left", "").Label("Return to parent trace").OnClick(p.returnTraceParent)
			ui.Text(c, fmt.Sprintf("子 Agent 会话 · 第 %d 层", len(p.trace.target.Path))).FontSize(workspaceFontBody).TextColor(t.TextMuted).SingleLine()
		} else {
			p.buildTraceBranchMenu(c)
		}
		ui.Box(c).Grow(1)
		ui.Row(c).Label("Trace scale").Gap(2).Padding(2).Radius(8).Background(workspacePalette(c).subtle).Children(func() {
			for _, choice := range []struct {
				scale product.TraceScale
				text  string
			}{{product.TraceScaleDuration, "时长"}, {product.TraceScaleTurn, "轮次"}, {product.TraceScaleCall, "调用"}, {product.TraceScaleStep, "子调用"}} {
				selected := p.traceScale() == choice.scale
				button := ui.ButtonBase(c.Key("trace-scale-"+string(choice.scale))).Label("Trace scale "+choice.text).Selected(selected).Height(24).Padding(0, 8).Radius(6).TextColor(t.TextMuted).FocusRing(false)
				if selected {
					button.Background(t.Surface).TextColor(t.Text)
				}
				scale := choice.scale
				button.OnClick(func() { p.setTraceScale(scale) }).Children(func() { ui.Text(c, choice.text).FontSize(workspaceFontBody).LineHeight(1.5).SingleLine() })
			}
		})
		workspaceControlInput(c, "trace-query", &p.trace.query).Label("Trace literal search").Width(200).OnSubmit(func() { p.searchTrace(false) })
		p.workspaceControlAction(c, "trace-search", "搜索", "search", "", p.trace.searchBusy, func() { p.searchTrace(false) }, strings.TrimSpace(p.trace.query) == "").Label("Search trace")
	})
}

func (p *NativePage) buildTraceBranchMenu(c *ui.Context) {
	label := "主会话"
	var choices []hub.Branch
	for _, branch := range p.detail.Branches {
		if branch.SessionID == p.selected && branch.NativeSessionID != "" {
			choices = append(choices, branch)
		}
		if branch.ID == p.trace.target.BranchID {
			label = "分支 · " + branch.Target.EngineID
		}
	}
	if len(choices) < 2 {
		ui.Text(c, label).FontSize(workspaceFontBody).TextColor(c.Theme().TextMuted).SingleLine()
		return
	}
	workspaceControlIconButton(c, "trace-branch", label, "git-branch", "").Label("Trace branch").Menu(func(m *ui.Menu) {
		for _, branch := range choices {
			if m.Item(branch.Target.EngineID + " · " + branch.ID).Chosen() {
				p.trace.frames = nil
				p.openTrace(product.TraceTarget{SessionID: p.selected, BranchID: branch.ID}, 0, product.TraceHint{})
			}
		}
	})
}

func (p *NativePage) buildTraceSearchResults(c *ui.Context) {
	if p.trace.searchQuery == "" {
		return
	}
	t := c.Theme()
	ui.Column(c).Label("Trace search results").FillWidth().Gap(4).Children(func() {
		status := fmt.Sprintf("“%s” 的匹配：%d 条", p.trace.searchQuery, len(p.trace.searchHits))
		if p.trace.searchBusy {
			status += "，正在搜索…"
		}
		ui.Text(c, status).FontSize(workspaceFontCaption).TextColor(t.TextMuted)
		ui.Scroll(c).FillWidth().MaxHeight(112).Children(func() {
			for _, hit := range p.trace.searchHits {
				hit := hit
				ui.ButtonBase(c.Key("trace-hit-"+strconv.FormatUint(hit.Sequence, 10))).Label("Trace hit "+strconv.FormatUint(hit.Sequence, 10)).
					FillWidth().Height(26).Padding(0, 8).Radius(6).AlignItems(ui.Center).Justify(ui.Start).TextColor(t.Text).
					OnClick(func() { p.jumpTraceHit(hit) }).Children(func() {
					ui.Text(c, fmt.Sprintf("#%d · %s · %s", hit.Sequence, hit.Type, hit.Summary)).FontSize(workspaceFontCaption).SingleLine().MinWidth(0)
				})
			}
		})
		if p.trace.search.HasMore {
			p.workspaceControlAction(c, "trace-search-more", "继续搜索更早之后的记录", "", "ghost", p.trace.searchBusy, func() { p.searchTrace(true) }).Label("Continue trace search")
		}
	})
}

func (p *NativePage) buildTraceTimeline(c *ui.Context) {
	t := c.Theme()
	steps, layout := p.trace.steps, p.trace.layout
	lanes := []chart.Tick{{At: 5.0 / 6, Label: "输入"}, {At: 0.5, Label: "模型"}, {At: 1.0 / 6, Label: "工具"}}
	var bottom []chart.Tick
	if p.traceScale() == product.TraceScaleDuration {
		if layout.To.After(layout.From) {
			// Clock-style tick labels (".200", ":17") read as noise on a trace;
			// label each tick with the time elapsed since the window's start.
			scale := chart.NewTime(layout.From, layout.To)
			bottom = scale.Ticks(5)
			for i := range bottom {
				bottom[i].Label = traceElapsedLabel(scale.Invert(bottom[i].At).Sub(layout.From))
			}
		}
	} else {
		every := max(1, len(layout.Groups)/8)
		for i, group := range layout.Groups {
			if i%every == 0 {
				bottom = append(bottom, chart.Tick{At: (group.X0 + group.X1) / 2, Label: group.Label})
			}
		}
	}
	label := func(side chart.Side, tick chart.Tick) {
		ui.Text(c, tick.Label).FontSize(workspaceFontCaption).TextColor(t.TextMuted).NoWrap()
	}
	selected := p.trace.selected
	chart.Plot(c, chart.Axes{Left: lanes, Bottom: bottom, Label: label}, func(area ui.Element) {
		area.Label("Trace timeline").Background(workspacePalette(c).subtle).Radius(8)
		bars := layout.Bars
		colors := make([]ui.Color, len(bars))
		marked := -1
		for i, bar := range bars {
			colors[i] = traceLaneColor(c, steps[bar.Step])
			if steps[bar.Step].ID == selected {
				marked = i
			}
		}
		border, accent := t.Border, t.Text
		area.Draw(func(painter *ui.Painter, r ui.Rect) {
			laneHeight := r.H / 3
			for lane := 1; lane < 3; lane++ {
				y := r.Y + float32(lane)*laneHeight
				painter.Line(r.X, y, r.X+r.W, y, 1, border)
			}
			for i, bar := range bars {
				x0 := r.X + bar.X0*r.W
				width := max(3, (bar.X1-bar.X0)*r.W)
				y := r.Y + float32(bar.Lane)*laneHeight + laneHeight*0.25
				box := ui.Rect{X: x0, Y: y, W: width, H: laneHeight * 0.5}
				painter.Fill(box, colors[i], 2)
				if i == marked {
					painter.Stroke(ui.Rect{X: box.X - 2, Y: box.Y - 2, W: box.W + 4, H: box.H + 4}, accent, 3, 1.5)
				}
			}
		})
		// One transparent hit target over the plot maps a click to its bar.
		hit := ui.ButtonBase(c.Key("trace-timeline-hit")).Label("Select trace step on timeline").Absolute().Left(0).Top(0).Right(0).Bottom(0).
			Background(ui.Transparent).FocusRing(false)
		x, y, over := hit.PointerPosition()
		bounds := hit.Bounds()
		hit.OnClick(func() {
			if !over || bounds.W <= 0 || bounds.H <= 0 {
				return
			}
			if step, ok := traceBarAt(steps, bars, x/bounds.W, y/bounds.H); ok {
				p.selectTraceStep(step)
				p.revealTraceStep(step)
			}
		})
	}).Height(148).Gap(6).FillWidth()
}

// traceBarAt picks the bar in the clicked lane nearest the click.
func traceBarAt(steps []product.TraceStep, bars []product.TraceBar, fx, fy float32) (string, bool) {
	lane := product.TraceLane(min(2, max(0, int(fy*3))))
	best, distance := -1, float32(2)
	for i, bar := range bars {
		if bar.Lane != lane {
			continue
		}
		d := float32(0)
		if fx < bar.X0 {
			d = bar.X0 - fx
		} else if fx > bar.X1 {
			d = fx - bar.X1
		}
		if d < distance {
			best, distance = i, d
		}
	}
	if best < 0 || distance > 0.05 {
		return "", false
	}
	return steps[bars[best].Step].ID, true
}

func (p *NativePage) buildTraceSteps(c *ui.Context) {
	t := c.Theme()
	w := p.trace.window
	if w.MoreBefore() {
		p.workspaceControlAction(c, "trace-older", "点击加载更早的历史", "", "ghost", p.trace.loading, p.loadEarlierTrace).Label("Load earlier trace")
	}
	steps := p.trace.steps
	p.trace.built = p.trace.version
	p.trace.list.Key = func(i int) any { return steps[i].ID }
	ui.List(c.Key("trace-step-list"), &p.trace.list, len(steps), func(i int) {
		step := steps[i]
		selected := step.ID == p.trace.selected
		row := ui.ButtonBase(c.Key("trace-step-"+step.ID)).Label("Trace step "+step.ID).Selected(selected).FillWidth().Height(34).
			Padding(0, 8).Gap(8).Radius(6).AlignItems(ui.Center).Justify(ui.Start).TextColor(t.Text).FocusRing(false)
		if selected {
			row.Background(workspacePalette(c).selection)
		} else if row.Hovered() {
			row.Background(workspacePalette(c).hover)
		}
		row.OnClick(func() { p.selectTraceStep(step.ID) }).Children(func() {
			ui.Box(c).Width(56).Shrink(0).Height(20).Radius(4).Background(workspacePalette(c).subtle).AlignItems(ui.Center).Justify(ui.Center).Children(func() {
				ui.Text(c, traceKindLabel(step.Kind)).FontSize(workspaceFontCaption).TextColor(traceLaneColor(c, step)).SingleLine()
			})
			summary := step.Summary
			if step.Kind == product.TraceStepTool && step.Status != "running" && step.Status != "succeeded" {
				summary += " · " + traceStatusLabel(step.Status)
			}
			ui.Text(c, summary).FontSize(workspaceFontBody).SingleLine().Grow(1).MinWidth(0)
			duration := traceDuration(step.Duration())
			if step.Open {
				duration = "进行中"
			}
			ui.Text(c, duration).FontSize(workspaceFontCaption).TextColor(t.TextMuted).SingleLine().Shrink(0)
		})
	}).Label("Trace step list").Grow(1).MinHeight(0).FillWidth()
}

func (p *NativePage) buildTraceDetail(c *ui.Context) {
	t := c.Theme()
	step, ok := p.traceSelectedStep()
	if !ok {
		ui.Text(c, "选择一个步骤查看详情。").FontSize(workspaceFontBody).TextColor(t.TextMuted)
		return
	}
	ui.Text(c, traceStepTitle(step)).FontSize(workspaceFontSubtitle).FontWeight(600).LineHeight(1.5)
	ui.Row(c).Label("Trace detail tabs").Gap(4).Children(func() {
		for _, tab := range []struct{ key, text string }{{"overview", "概述"}, {"preview", "预览"}, {"raw", "原始内容"}} {
			selected := p.trace.tab == tab.key
			kind := "ghost"
			if selected {
				kind = ""
			}
			key := tab.key
			workspaceControlButton(c, "trace-tab-"+key, tab.text, kind).Label("Trace detail " + tab.text).Selected(selected).OnClick(func() {
				p.trace.tab = key
				if key == "raw" && p.trace.rawURI == "" && len(step.Records) > 0 {
					p.readTraceRaw(step.Records[0].Sequence, false)
				}
			})
		}
	})
	ui.Scroll(c).Label("Trace detail content").Grow(1).MinHeight(0).FillWidth().Gap(8).Children(func() {
		switch p.trace.tab {
		case "preview":
			p.buildTracePreview(c, step)
		case "raw":
			p.buildTraceRaw(c, step)
		default:
			p.buildTraceOverview(c, step)
		}
	})
}

func traceField(c *ui.Context, name, value string) {
	ui.Row(c).Label("Trace field " + name).FillWidth().Gap(12).AlignItems(ui.Start).Children(func() {
		ui.Text(c, name).Width(84).Shrink(0).FontSize(workspaceFontBody).TextColor(c.Theme().TextMuted)
		ui.Text(c, value).Grow(1).MinWidth(0).FontSize(workspaceFontBody).LineHeight(1.5).Selectable()
	})
}

func (p *NativePage) buildTraceOverview(c *ui.Context, step product.TraceStep) {
	approximate := ""
	if step.Approximate {
		approximate = "（近似：记录提交时间）"
	}
	traceField(c, "类型", traceKindLabel(step.Kind))
	traceField(c, "状态", traceStatusLabel(step.Status))
	traceField(c, "开始", traceClock(step.Start)+approximate)
	end := traceClock(step.End)
	if step.Open {
		end = "尚未结束"
	}
	traceField(c, "结束", end+approximate)
	if d := traceDuration(step.Duration()); d != "" {
		traceField(c, "耗时", d+approximate)
	}
	detail := p.trace.detail
	busy := p.trace.detailBusy || detail.StepID != step.ID
	if step.Kind == product.TraceStepModel {
		usage := "—"
		if busy {
			usage = "读取中…"
		} else if detail.Usage != nil {
			usage = fmt.Sprintf("输入 %d · 输出 %d", detail.Usage.InputTokens, detail.Usage.OutputTokens)
			if detail.Usage.CachedInputTokens != nil {
				usage += fmt.Sprintf(" · 缓存命中 %d", *detail.Usage.CachedInputTokens)
			}
		}
		traceField(c, "token 用量", usage)
	}
	if step.Kind == product.TraceStepTool {
		traceField(c, "工具", step.Name)
		arguments := detail.Arguments
		if busy {
			arguments = "读取中…"
		}
		if len(arguments) > 240 {
			arguments = strings.ToValidUTF8(arguments[:240], "") + "…"
		}
		traceField(c, "参数摘要", arguments)
	}
	if detail.Error != "" && !busy {
		traceField(c, "错误", detail.Error)
	}
	session := p.trace.window.NativeID()
	if len(p.trace.target.Path) > 0 {
		session += "（子会话）"
	}
	traceField(c, "所属会话", session)
	if step.Kind == product.TraceStepChild {
		child := detail.ChildID
		if busy {
			child = "读取中…"
		}
		traceField(c, "子会话", child)
		workspaceControlIconButton(c, "trace-enter-child", "进入子会话", "users-round", "", len(p.trace.target.Path) >= 8).Label("Enter child trace").OnClick(func() { p.enterTraceChild(step) })
	}
}

func (p *NativePage) buildTracePreview(c *ui.Context, step product.TraceStep) {
	t := c.Theme()
	detail := p.trace.detail
	if p.trace.detailBusy || detail.StepID != step.ID {
		ui.Text(c, "正在读取正文…").FontSize(workspaceFontBody).TextColor(t.TextMuted)
		return
	}
	options := p.trace.markdown
	if options == nil {
		options = &markdown.Options{}
	}
	section := func(title, body string, rendered bool) {
		if body == "" {
			return
		}
		ui.Text(c, title).FontSize(workspaceFontCaption).FontWeight(600).TextColor(t.TextMuted)
		if rendered {
			markdown.View(c, body, options)
		} else {
			ui.Text(c, body).FontSize(workspaceFontBody).LineHeight(1.5).Selectable()
		}
	}
	switch step.Kind {
	case product.TraceStepTool:
		section("参数", detail.Arguments, false)
		section("结果", detail.Result, false)
	default:
		section("正文", detail.Message, true)
	}
	section("错误", detail.Error, false)
	if detail.Truncated {
		ui.Text(c, "正文较长，这里只显示开头部分；完整内容在“原始内容”中分页查看。").FontSize(workspaceFontCaption).TextColor(t.TextMuted)
	}
	if detail.Message == "" && detail.Arguments == "" && detail.Result == "" && detail.Error == "" {
		ui.Text(c, "这一步没有可读正文。").FontSize(workspaceFontBody).TextColor(t.TextMuted)
	}
}

func (p *NativePage) buildTraceRaw(c *ui.Context, step product.TraceStep) {
	t := c.Theme()
	ui.Row(c).Label("Trace raw records").Gap(4).Wrap().Children(func() {
		// Several records can share a role (llm_* audit records), so each
		// button names and reads its record by sequence.
		for _, record := range step.Records {
			sequence := record.Sequence
			kind := "ghost"
			if p.trace.rawSeq == sequence {
				kind = ""
			}
			name := record.Role
			if record.Role == "related" && record.Type != "" {
				name = record.Type
			}
			workspaceControlButton(c, "trace-raw-"+strconv.FormatUint(sequence, 10), fmt.Sprintf("#%d %s", sequence, name), kind).
				Label(fmt.Sprintf("Trace raw #%d", sequence)).OnClick(func() { p.readTraceRaw(sequence, false) })
		}
	})
	text := p.trace.rawText
	if text == "" && p.trace.rawBusy {
		text = "读取中…"
	}
	ui.Text(c, text).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(t.Text).Selectable()
	if p.trace.rawMore {
		p.workspaceControlAction(c, "trace-raw-more", "加载更多", "", "", p.trace.rawBusy, func() { p.readTraceRaw(p.trace.rawSeq, true) }).Label("Load more raw record")
	}
}
