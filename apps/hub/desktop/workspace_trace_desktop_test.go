// INPUT: Actual product/native SQLite Turns written through Store.Record, the product TraceService and headless MyGo wheel, click and frame timing
// OUTPUT: View switch keeping both scroll positions, earlier paging without jumps, selected step detail tabs, child enter/return, unsupported engines, search jumps, timeline clicks and an opt-in trace scroll frame gate
// POS: [L7 Hub UI tests] Execution-trace view regressions without a model, process or OS window
//go:build desktop

package hubdesktop

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"sort"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/engine/alva"
	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
	"github.com/egoist/mygo/ui"
)

type traceSeed struct {
	service *product.HubService
	session hub.Session
	native  string
	log     storage.Session
	turns   int
}

// newTraceSeed creates a product session bound to a native session whose
// engine has the real Alva RepositoryInspector.
func newTraceSeed(t *testing.T) *traceSeed {
	t.Helper()
	return newTraceSeedWith(t, nil)
}

// newTraceSeedWith lets a test wrap the real Inspector, e.g. to hold or fail
// selected reads.
func newTraceSeedWith(t *testing.T, gate *gatedTraceInspector) *traceSeed {
	t.Helper()
	service, _ := fixture(t)
	real, err := alva.NewRepositoryInspector(service.Repository)
	if err != nil {
		t.Fatal(err)
	}
	var inspector engine.Inspector = real
	if gate != nil {
		gate.Inspector = real
		inspector = gate
	}
	if err = service.Runtime.RegisterDescriptor("alva", engine.Descriptor{Runtime: transcriptColdRuntime{}, Inspector: inspector, Config: engine.ConfigSurface{ProviderProtocol: "native"}}); err != nil {
		t.Fatal(err)
	}
	session, err := service.Create(t.Context(), "Trace", "")
	if err != nil {
		t.Fatal(err)
	}
	native := "native-trace-view"
	log, err := service.Repository.Create(t.Context(), session.ID, native, "")
	if err != nil {
		t.Fatal(err)
	}
	if err = service.Store.BindNative(t.Context(), session.ID, session.ActiveBranchID, native); err != nil {
		t.Fatal(err)
	}
	return &traceSeed{service: service, session: session, native: native, log: log}
}

func (s *traceSeed) append(t *testing.T, records ...abi.SessionRecord) []abi.SessionRecord {
	t.Helper()
	for i := range records {
		records[i] = abi.BuiltinRecord(records[i])
	}
	committed, err := s.log.AppendRecords(t.Context(), records)
	if err != nil {
		t.Fatal(err)
	}
	return committed
}

func (s *traceSeed) record(t *testing.T, attempt string, event engine.Event) {
	t.Helper()
	if err := s.service.Store.Record(t.Context(), attempt, event, false); err != nil {
		t.Fatal(err)
	}
}

func traceRef(native string, record abi.SessionRecord) *engine.Reference {
	return &engine.Reference{SessionID: native, RecordID: record.ID, Sequence: record.Sequence}
}

func traceMessage(role wire.Role, text string) *wire.Message {
	m := wire.TextMessage(role, text)
	return &m
}

// turn commits one finished Turn: input, a planning model call with tools,
// their results and a final reply, plus the product events referencing them.
// open leaves the tools unsettled and returns before the reply.
func (s *traceSeed) turn(t *testing.T, tools int, result string, open bool) (hub.Claim, []abi.SessionRecord) {
	t.Helper()
	s.turns++
	n := s.turns
	claim, err := s.service.Store.BeginTurn(t.Context(), s.session.ID, fmt.Sprintf("Question %d", n))
	if err != nil {
		t.Fatal(err)
	}
	s.record(t, claim.Attempt.ID, engine.Event{Kind: engine.SessionStarted, SessionID: s.native})
	s.append(t, abi.SessionRecord{Kind: abi.RecordConversation, Message: traceMessage(wire.RoleUser, fmt.Sprintf("Question %d", n))}, abi.SessionRecord{Kind: abi.RecordModelIntent, ModelIntent: &abi.ModelIntent{RequestID: fmt.Sprintf("plan-%d", n)}})
	run := fmt.Sprintf("run-%d", n)
	batch := []abi.SessionRecord{
		{Kind: abi.RecordConversation, Message: traceMessage(wire.RoleAssistant, fmt.Sprintf("Plan %d", n))},
		{Kind: abi.RecordModelOutcome, ModelOutcome: &abi.ModelOutcome{RequestID: fmt.Sprintf("plan-%d", n), Status: abi.OutcomeSucceeded, Usage: &wire.Usage{InputTokens: 100 + uint64(n), OutputTokens: 9}}},
	}
	for k := range tools {
		batch = append(batch, abi.SessionRecord{Kind: abi.RecordToolIntent, ToolIntent: &abi.ToolIntent{RunID: run, Call: wire.ToolCall{ID: fmt.Sprintf("call-%d", k), Name: "read_file", Arguments: json.RawMessage(fmt.Sprintf(`{"path":"file-%d-%d.go"}`, n, k))}}})
	}
	committed := s.append(t, batch...)
	intents := committed[2:]
	for _, intent := range intents {
		s.record(t, claim.Attempt.ID, engine.Event{Kind: engine.ToolStart, ID: intent.ToolIntent.Call.ID, Name: "read_file", Reference: traceRef(s.native, intent)})
	}
	if open {
		return claim, intents
	}
	started := time.Now().UTC().Add(-time.Second)
	for k := range tools {
		text := result
		if text == "" {
			text = fmt.Sprintf("content %d-%d", n, k)
		}
		outcome := s.append(t, abi.SessionRecord{Kind: abi.RecordToolOutcome, ToolOutcome: &abi.ToolOutcome{RunID: run, CallID: fmt.Sprintf("call-%d", k), Status: abi.OutcomeSucceeded, Result: wire.ToolResult{CallID: fmt.Sprintf("call-%d", k), Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: text}}}, StartedAt: started, EndedAt: started.Add(120 * time.Millisecond)}})
		s.record(t, claim.Attempt.ID, engine.Event{Kind: engine.ToolEnd, ID: fmt.Sprintf("call-%d", k), Name: "read_file", Reference: traceRef(s.native, outcome[0])})
	}
	s.finish(t, claim, n)
	return claim, intents
}

func (s *traceSeed) finish(t *testing.T, claim hub.Claim, n int) {
	t.Helper()
	s.append(t, abi.SessionRecord{Kind: abi.RecordModelIntent, ModelIntent: &abi.ModelIntent{RequestID: fmt.Sprintf("final-%d", n)}})
	final := s.append(t, abi.SessionRecord{Kind: abi.RecordConversation, Message: traceMessage(wire.RoleAssistant, fmt.Sprintf("Answer %d. ", n)+strings.Repeat("A committed paragraph keeps each row tall enough to scroll. ", 3))}, abi.SessionRecord{Kind: abi.RecordModelOutcome, ModelOutcome: &abi.ModelOutcome{RequestID: fmt.Sprintf("final-%d", n), Status: abi.OutcomeSucceeded}})
	s.record(t, claim.Attempt.ID, engine.Event{Kind: engine.Message, ID: "assistant-1", Text: fmt.Sprintf("Answer %d. ", n) + strings.Repeat("A committed paragraph keeps each row tall enough to scroll. ", 3), Reference: traceRef(s.native, final[0])})
	s.record(t, claim.Attempt.ID, engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded})
	if err := s.service.Store.FinishTurn(t.Context(), claim.Turn.ID, engine.Succeeded); err != nil {
		t.Fatal(err)
	}
}

func (s *traceSeed) page(t *testing.T) (*NativePage, *ui.Tester) {
	t.Helper()
	p := NewNativePage(s.service)
	p.ctx = t.Context()
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	p.selectSession(s.session.ID)
	waitTranscript(t, tester, func() bool { return !p.polling && !p.chat.Busy && len(p.model.Messages) > 0 })
	return p, tester
}

func openTraceView(t *testing.T, tester *ui.Tester, p *NativePage) {
	t.Helper()
	connectionClick(t, tester, "Show execution trace")
	waitTranscript(t, tester, func() bool { return p.trace.window != nil && !p.trace.loading })
	tester.Frame()
}

func wheelTrace(t *testing.T, tester *ui.Tester, delta float32, stop func() bool) {
	t.Helper()
	list, ok := tester.Find("Trace step list")
	if !ok {
		t.Fatal("trace step list is not rendered")
	}
	for range 400 {
		if stop() {
			return
		}
		tester.Scroll(list.X+list.W/2, list.Y+list.H/2, 0, delta)
		tester.Frame()
	}
	t.Fatal("trace wheel scrolling never reached its target")
}

func TestWorkspaceTraceSwitchKeepsEachViewScrollPosition(t *testing.T) {
	seed := newTraceSeed(t)
	for range 40 {
		seed.turn(t, 1, "", false)
	}
	p, tester := seed.page(t)
	wheelTranscript(t, tester, -240, func() bool {
		first, last := p.chat.VisibleRows()
		return last >= first && first > 8 && first < len(p.model.Messages)-12 && !p.chat.AtEnd()
	})
	chatFirst, _ := p.chat.VisibleRows()
	chatAnchor := p.model.Messages[chatFirst].ID
	openTraceView(t, tester, p)
	if _, ok := tester.Find("Message history"); ok {
		t.Fatal("conversation list is still rendered in the trace view")
	}
	wheelTrace(t, tester, -120, func() bool {
		first, last := p.trace.list.Visible()
		return last >= first && first > 6 && first < len(p.trace.steps)-15
	})
	traceFirst, _ := p.trace.list.Visible()
	traceAnchor := p.trace.steps[traceFirst].ID
	connectionClick(t, tester, "Show conversation")
	tester.Frame()
	if first, last := p.chat.VisibleRows(); last < first || p.model.Messages[first].ID != chatAnchor {
		t.Fatal("returning to the conversation lost its scroll position", chatAnchor, first)
	}
	connectionClick(t, tester, "Show execution trace")
	tester.Frame()
	if first, last := p.trace.list.Visible(); last < first || p.trace.steps[first].ID != traceAnchor || p.trace.loading {
		t.Fatal("returning to the trace lost its scroll position or reloaded", traceAnchor, first)
	}
}

func TestWorkspaceTraceLoadingEarlierKeepsVisibleRows(t *testing.T) {
	seed := newTraceSeed(t)
	for range 40 {
		seed.turn(t, 2, "", false)
	}
	p, tester := seed.page(t)
	openTraceView(t, tester, p)
	if !p.trace.window.MoreBefore() || !tester.HasText("点击加载更早的历史") {
		t.Fatal("long trace does not offer earlier history")
	}
	loaded := len(p.trace.steps)
	anchor := ""
	wheelTrace(t, tester, -240, func() bool {
		first, last := p.trace.list.Visible()
		if last >= first && first < 3 {
			anchor = p.trace.steps[first].ID
			return true
		}
		return false
	})
	waitTranscript(t, tester, func() bool { return !p.trace.loading && len(p.trace.steps) > loaded })
	tester.Frame()
	first, last := p.trace.list.Visible()
	if last < first || p.trace.steps[first].ID != anchor {
		t.Fatal("loading earlier steps moved the first visible row", anchor, p.trace.steps[first].ID)
	}
	seen := map[string]bool{}
	for _, step := range p.trace.steps {
		if seen[step.ID] {
			t.Fatal("paging duplicated a step", step.ID)
		}
		seen[step.ID] = true
	}
	before := len(p.trace.steps)
	if p.trace.window.MoreBefore() {
		connectionClick(t, tester, "Load earlier trace")
		waitTranscript(t, tester, func() bool { return !p.trace.loading })
		if len(p.trace.steps) <= before {
			t.Fatal("the load-earlier action did not add steps")
		}
	}
}

func TestWorkspaceTraceSelectedStepShowsDetailTabs(t *testing.T) {
	seed := newTraceSeed(t)
	seed.turn(t, 1, "", false)
	seed.turn(t, 1, strings.Repeat("长结果 long result ", 300), false)
	p, tester := seed.page(t)
	openTraceView(t, tester, p)
	var model, tool product.TraceStep
	for _, step := range p.trace.steps {
		if step.Turn == 2 && step.Kind == product.TraceStepModel && model.ID == "" {
			model = step
		}
		if step.Turn == 2 && step.Kind == product.TraceStepTool {
			tool = step
		}
	}
	connectionClick(t, tester, "Trace step "+model.ID)
	settleTrace(t, tester, p)
	if !tester.HasText("第 2 轮 · 第 2 步") || !tester.HasText("输入 102 · 输出 9") || !tester.HasText("（近似：记录提交时间）") {
		t.Fatal("model step overview lacks its title, token usage or approximate times", tester.Texts())
	}
	// The trace Turn equals the conversation's block Turn for the same tool.
	for _, block := range p.transcript.Blocks() {
		if block.Kind == product.BlockKindTool && block.Ref != nil && block.Ref.Sequence == tool.Last && block.Turn != tool.Turn {
			t.Fatal("trace Turn differs from the conversation block", block.Turn, tool.Turn)
		}
	}
	connectionClick(t, tester, "Trace step "+tool.ID)
	settleTrace(t, tester, p)
	if !tester.HasText("第 2 轮 · 第 3 步") || !tester.HasText("read_file") || !strings.Contains(strings.Join(tester.Texts(), "\n"), "file-2-0.go") {
		t.Fatal("tool step overview", tester.Texts())
	}
	connectionClick(t, tester, "Trace detail 预览")
	tester.Frame()
	if !strings.Contains(strings.Join(tester.Texts(), "\n"), "长结果 long result") || !tester.HasText("结果") {
		t.Fatal("tool preview lacks its result text", tester.Texts())
	}
	connectionClick(t, tester, "Trace detail 原始内容")
	settleTrace(t, tester, p)
	outcome, _ := tool.Record("outcome")
	connectionClick(t, tester, fmt.Sprintf("Trace raw #%d", outcome.Sequence))
	settleTrace(t, tester, p)
	first := len(p.trace.rawText)
	if first == 0 || first > 2048 || !p.trace.rawMore || !strings.Contains(p.trace.rawText, "tool_outcome") {
		t.Fatal("raw record did not open with a bounded 2 KiB page", first, p.trace.rawMore)
	}
	connectionClick(t, tester, "Load more raw record")
	settleTrace(t, tester, p)
	if len(p.trace.rawText) <= first {
		t.Fatal("load more did not append the next raw page")
	}
	if p.Service.Runtime.HasActive() {
		t.Fatal("trace detail started execution")
	}
}

func TestWorkspaceTraceChildEnterAndReturn(t *testing.T) {
	seed := newTraceSeed(t)
	seed.turn(t, 1, "", false)
	_, intents := seed.turn(t, 1, "", true)
	child, err := seed.service.Repository.CreateChild(t.Context(), seed.session.ID, seed.native, "native-trace-child", intents[0].ToolIntent.RunID, intents[0].ToolIntent.Call.ID, "scope")
	if err != nil {
		t.Fatal(err)
	}
	for _, batch := range [][]abi.SessionRecord{
		{abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: traceMessage(wire.RoleUser, "Child task")}), abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordModelIntent, ModelIntent: &abi.ModelIntent{RequestID: "child"}})},
		{abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: traceMessage(wire.RoleAssistant, "Child answer")}), abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordModelOutcome, ModelOutcome: &abi.ModelOutcome{RequestID: "child", Status: abi.OutcomeSucceeded}})},
	} {
		if _, err = child.Session.AppendRecords(t.Context(), batch); err != nil {
			t.Fatal(err)
		}
	}
	p, tester := seed.page(t)
	openTraceView(t, tester, p)
	link := "child/" + fmt.Sprint(child.Link.Sequence)
	connectionClick(t, tester, "Trace step "+link)
	settleTrace(t, tester, p)
	if !tester.HasText("native-trace-child") || !tester.HasText("第 2 轮 · 第 4 步") {
		t.Fatal("child step overview lacks its linked session", tester.Texts())
	}
	parent := p.trace.window
	connectionClick(t, tester, "Enter child trace")
	waitTranscript(t, tester, func() bool { return p.trace.window != nil && !p.trace.loading })
	tester.Frame()
	if p.trace.window.NativeID() != "native-trace-child" || len(p.trace.steps) != 2 || p.trace.steps[1].Summary != "Child answer" || p.trace.steps[1].Turn != 2 || !tester.HasText("子 Agent 会话 · 第 1 层") {
		t.Fatal("child trace", p.trace.window.NativeID(), p.trace.steps)
	}
	connectionClick(t, tester, "Return to parent trace")
	settleTrace(t, tester, p)
	if p.trace.window != parent || p.trace.selected != link || len(p.trace.frames) != 0 || p.trace.window.NativeID() != seed.native {
		t.Fatal("returning did not restore the parent trace and its selection", p.trace.selected)
	}
}

type traceViewForeignRuntime struct{ engine.Runtime }

func (traceViewForeignRuntime) Capabilities() engine.Capabilities { return engine.Capabilities{} }
func (traceViewForeignRuntime) Execute(context.Context, engine.Request, func(engine.Event) error) error {
	panic("trace observation must not execute")
}

func TestWorkspaceTraceUnsupportedEngineExplains(t *testing.T) {
	service, _ := fixture(t)
	if err := service.Runtime.RegisterDescriptor("foreign", engine.Descriptor{Runtime: traceViewForeignRuntime{}, Config: engine.ConfigSurface{ProviderProtocol: "native"}}); err != nil {
		t.Fatal(err)
	}
	target := service.DefaultTarget
	target.EngineID = "foreign"
	session, err := service.Store.CreateSession(t.Context(), "Foreign", t.TempDir(), target)
	if err != nil {
		t.Fatal(err)
	}
	seedTranscriptTurns(t, service, session.ID, 0, 1)
	p := NewNativePage(service)
	p.ctx = t.Context()
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	p.selectSession(session.ID)
	waitTranscript(t, tester, func() bool { return !p.polling && len(p.model.Messages) > 0 })
	openTraceView(t, tester, p)
	if !tester.HasText("当前分支的引擎没有提供执行轨迹，只能查看对话。") || p.trace.problem != "" || p.problem != "" || service.Runtime.HasActive() {
		t.Fatal("engine without Inspector did not explain the missing trace", tester.Texts(), p.trace.problem, p.problem)
	}
}

func TestWorkspaceTraceSearchJumpAndTimelineClick(t *testing.T) {
	seed := newTraceSeed(t)
	seed.turn(t, 1, "needle-in-first-turn", false)
	for range 30 {
		seed.turn(t, 2, "", false)
	}
	p, tester := seed.page(t)
	openTraceView(t, tester, p)
	connectionClick(t, tester, "Trace literal search")
	tester.Type("needle-in-first-turn")
	tester.Frame()
	connectionClick(t, tester, "Search trace")
	waitTranscript(t, tester, func() bool {
		if !p.trace.searchBusy && len(p.trace.searchHits) == 0 && p.trace.search.HasMore {
			p.searchTrace(true)
		}
		return !p.trace.searchBusy && len(p.trace.searchHits) > 0
	})
	tester.Frame()
	hit := p.trace.searchHits[0]
	connectionClick(t, tester, fmt.Sprintf("Trace hit %d", hit.Sequence))
	settleTrace(t, tester, p)
	step, ok := p.traceSelectedStep()
	if !ok || step.Kind != product.TraceStepTool || step.Turn != 1 || step.Last != hit.Sequence {
		t.Fatal("search hit did not jump to its step", step, ok, hit, p.trace.problem, p.trace.window.First(), p.trace.window.Last())
	}
	// A click on a timeline bar selects that step.
	area, ok := tester.Find("Trace timeline")
	if !ok {
		t.Fatal("timeline not rendered")
	}
	var target product.TraceBar
	for _, bar := range p.trace.layout.Bars {
		if p.trace.steps[bar.Step].Kind == product.TraceStepInput {
			target = bar
		}
	}
	tester.ClickAt(area.X+(target.X0+target.X1)/2*area.W+1, area.Y+area.H/6)
	settleTrace(t, tester, p)
	if selected, ok := p.traceSelectedStep(); !ok || selected.ID != p.trace.steps[target.Step].ID {
		t.Fatal("timeline click did not select the bar's step", selected.ID, p.trace.steps[target.Step].ID)
	}
	for _, scale := range []string{"轮次", "调用", "子调用", "时长"} {
		connectionClick(t, tester, "Trace scale "+scale)
		tester.Frame()
	}
	if p.traceScale() != product.TraceScaleDuration || len(p.trace.layout.Bars) != len(p.trace.steps) {
		t.Fatal("scale switch lost the layout")
	}
}

// ALVA_UI_BENCH=1 measures headless build/layout/paint CPU while scrolling a
// full trace window; it does not measure GPU present or real input latency.
func TestWorkspaceTraceScrollFrameBudget(t *testing.T) {
	if os.Getenv("ALVA_UI_BENCH") != "1" {
		t.Skip("set ALVA_UI_BENCH=1 to run the trace frame-time gate")
	}
	seed := newTraceSeed(t)
	for range 400 {
		seed.turn(t, 3, "", false)
	}
	p, tester := seed.page(t)
	openTraceView(t, tester, p)
	for p.trace.window.MoreBefore() {
		p.traceOlder()
		waitTranscript(t, tester, func() bool { return !p.trace.loading })
	}
	steps := len(p.trace.steps)
	list, ok := tester.Find("Trace step list")
	if !ok || steps < 2000 {
		t.Fatal("bench window is too small", steps)
	}
	var frames []time.Duration
	for _, delta := range []float32{-240, 240, -2400} {
		before, _ := p.trace.list.Visible()
		for range 200 {
			start := time.Now()
			tester.Scroll(list.X+list.W/2, list.Y+list.H/2, 0, delta)
			tester.Frame()
			frames = append(frames, time.Since(start))
		}
		if after, _ := p.trace.list.Visible(); after == before {
			t.Fatal("bench frames did not scroll the trace list", delta, before)
		}
	}
	sort.Slice(frames, func(i, j int) bool { return frames[i] < frames[j] })
	p95 := frames[len(frames)*95/100]
	t.Logf("%d trace steps (%d records, %d B window) headless scroll: p50=%s p95=%s max=%s n=%d", steps, p.trace.window.Records(), p.trace.window.Bytes(), frames[len(frames)/2], p95, frames[len(frames)-1], len(frames))
	if p95 > 8*time.Millisecond {
		t.Fatal("trace scroll frame CPU p95 exceeds 8 ms", p95)
	}
}

// gatedTraceInspector holds matching Page calls after the real read until
// released, so a held result reflects the facts at call time, and can fail
// matching calls outright.
type gatedTraceInspector struct {
	engine.Inspector
	mu      sync.Mutex
	hold    func(engine.InspectionQuery) bool
	fail    func(engine.InspectionQuery) bool
	entered chan struct{}
	release chan struct{}
	failed  atomic.Int32
}

func (g *gatedTraceInspector) arm(hold func(engine.InspectionQuery) bool) (<-chan struct{}, chan struct{}) {
	g.mu.Lock()
	defer g.mu.Unlock()
	g.hold, g.entered, g.release = hold, make(chan struct{}, 1), make(chan struct{})
	return g.entered, g.release
}

func (g *gatedTraceInspector) disarm() {
	g.mu.Lock()
	g.hold = nil
	g.mu.Unlock()
}

func (g *gatedTraceInspector) Page(ctx context.Context, scope engine.InspectionScope, query engine.InspectionQuery) (engine.InspectionPage, error) {
	g.mu.Lock()
	hold, fail, entered, release := g.hold, g.fail, g.entered, g.release
	g.mu.Unlock()
	if fail != nil && fail(query) {
		g.failed.Add(1)
		return engine.InspectionPage{}, errors.New("fixture page unavailable")
	}
	page, err := g.Inspector.Page(ctx, scope, query)
	if hold != nil && hold(query) {
		entered <- struct{}{}
		select {
		case <-release:
		case <-ctx.Done():
		}
	}
	return page, err
}

func olderPage(q engine.InspectionQuery) bool {
	return q.Before != 0 && q.Before != engine.InspectionEnd
}

func forwardPage(q engine.InspectionQuery) bool {
	return q.After != 0 && q.Before == 0 && q.Literal == ""
}

func waitEntered(t *testing.T, tester *ui.Tester, entered <-chan struct{}) {
	t.Helper()
	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		tester.Frame()
		select {
		case <-entered:
			return
		default:
			time.Sleep(time.Millisecond)
		}
	}
	t.Fatal("held trace read never started")
}

// settleOpenTurn commits the outcome of an open Turn's first tool and its reply.
func (s *traceSeed) settleOpenTurn(t *testing.T, claim hub.Claim, intent abi.SessionRecord, text string) {
	t.Helper()
	call := intent.ToolIntent.Call.ID
	outcome := s.append(t, abi.SessionRecord{Kind: abi.RecordToolOutcome, ToolOutcome: &abi.ToolOutcome{RunID: intent.ToolIntent.RunID, CallID: call, Status: abi.OutcomeSucceeded, Result: wire.ToolResult{CallID: call, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: text}}}}})
	s.record(t, claim.Attempt.ID, engine.Event{Kind: engine.ToolEnd, ID: call, Name: "read_file", Reference: traceRef(s.native, outcome[0])})
	s.finish(t, claim, s.turns)
}

func (s *traceSeed) lastNative(t *testing.T) uint64 {
	t.Helper()
	page, err := s.service.Repository.PageBefore(t.Context(), s.session.ID, s.native, 0, 1, 0)
	if err != nil || len(page.Entries) != 1 {
		t.Fatal(err)
	}
	return page.Entries[0].Sequence
}

// A page read dropped because an explicit edit (Stop, Send) moved the
// generation must not leave the trace loading forever.
func TestWorkspaceTraceDroppedReadDoesNotLeaveLoadingStuck(t *testing.T) {
	gate := &gatedTraceInspector{}
	seed := newTraceSeedWith(t, gate)
	for range 30 {
		seed.turn(t, 1, "", false)
	}
	p, tester := seed.page(t)
	openTraceView(t, tester, p)
	entered, release := gate.arm(olderPage)
	p.traceOlder()
	waitEntered(t, tester, entered)
	gate.disarm()
	p.generation++ // Stop/Send reject observations captured before them.
	close(release)
	waitTranscript(t, tester, func() bool { return !p.trace.loading && p.trace.window != nil && p.trace.generation == p.generation })
	if p.trace.problem != "" {
		t.Fatal("recovery reported a problem", p.trace.problem)
	}
	// Paging still works afterwards.
	before := len(p.trace.steps)
	connectionClick(t, tester, "Load earlier trace")
	waitTranscript(t, tester, func() bool { return !p.trace.loading && len(p.trace.steps) > before })
}

// A search result dropped by a reload or a return to the parent must not
// leave the search busy forever.
func TestWorkspaceTraceDroppedSearchDoesNotStayBusy(t *testing.T) {
	gate := &gatedTraceInspector{}
	seed := newTraceSeedWith(t, gate)
	seed.turn(t, 1, "needle", false)
	p, tester := seed.page(t)
	openTraceView(t, tester, p)
	for _, interrupt := range []func(){
		func() { p.openTrace(p.trace.target, 0, product.TraceHint{}) },
		func() {
			p.trace.frames = append(p.trace.frames, workspaceTraceFrame{target: p.trace.target, window: p.trace.window})
			p.returnTraceParent()
		},
	} {
		entered, release := gate.arm(func(q engine.InspectionQuery) bool { return q.Literal != "" })
		p.trace.query = "needle"
		p.searchTrace(false)
		waitEntered(t, tester, entered)
		gate.disarm()
		interrupt()
		close(release)
		waitTranscript(t, tester, func() bool { return !p.trace.loading && !p.trace.searchBusy })
		p.searchTrace(false)
		waitTranscript(t, tester, func() bool { return !p.trace.searchBusy && len(p.trace.searchHits) > 0 })
	}
}

// A selected step that was open refreshes its detail when its outcome pairs.
func TestWorkspaceTraceSelectedOpenStepRefreshesWhenItPairs(t *testing.T) {
	seed := newTraceSeed(t)
	seed.turn(t, 1, "", false)
	claim, intents := seed.turn(t, 1, "", true)
	p, tester := seed.page(t)
	openTraceView(t, tester, p)
	connectionClick(t, tester, "Trace step tool/run-2/call-0")
	settleTrace(t, tester, p)
	if step, _ := p.traceSelectedStep(); !step.Open || p.trace.detail.Result != "" {
		t.Fatal("fixture tool step is not open", step)
	}
	seed.settleOpenTurn(t, claim, intents[0], "paired later")
	p.traceNewer(false)
	settleTrace(t, tester, p)
	if step, _ := p.traceSelectedStep(); step.Open || p.trace.detail.Result != "paired later" {
		t.Fatal("selected step detail did not refresh after its outcome paired", step.Open, p.trace.detail)
	}
}

// The final refresh after a run stops happens even if a read was in flight.
func TestWorkspaceTraceFinalRefreshAfterRunEnds(t *testing.T) {
	gate := &gatedTraceInspector{}
	seed := newTraceSeedWith(t, gate)
	seed.turn(t, 1, "", false)
	claim, intents := seed.turn(t, 1, "", true)
	p, tester := seed.page(t)
	openTraceView(t, tester, p)
	p.detail.Session.Status = hub.Running
	entered, release := gate.arm(forwardPage)
	p.pollTrace()
	waitEntered(t, tester, entered)
	gate.disarm()
	seed.settleOpenTurn(t, claim, intents[0], "final")
	p.detail.Session.Status = engine.Succeeded
	p.pollTrace() // the run has stopped; one more read must follow
	close(release)
	last := seed.lastNative(t)
	waitTranscript(t, tester, func() bool { return !p.trace.loading && p.trace.window.Last() == last })
	if p.trace.live {
		t.Fatal("trace still follows a stopped run")
	}
}

// Raw buttons address records by sequence, not by role.
func TestWorkspaceTraceRawContentAddressesRecordsBySequence(t *testing.T) {
	seed := newTraceSeed(t)
	audit := func(kind string) abi.SessionRecord {
		return abi.SessionRecord{Kind: abi.RecordExtension, Namespace: "alva.kernel.audit", Type: kind, SchemaVersion: 1, Payload: json.RawMessage(`{"event":"` + kind + `"}`)}
	}
	seed.append(t, abi.SessionRecord{Kind: abi.RecordConversation, Message: traceMessage(wire.RoleUser, "Audited question")}, abi.SessionRecord{Kind: abi.RecordModelIntent, ModelIntent: &abi.ModelIntent{RequestID: "audited"}})
	related := seed.append(t, audit("llm_call_start"), audit("llm_call_end"))
	seed.append(t, abi.SessionRecord{Kind: abi.RecordConversation, Message: traceMessage(wire.RoleAssistant, "Audited answer")}, abi.SessionRecord{Kind: abi.RecordModelOutcome, ModelOutcome: &abi.ModelOutcome{RequestID: "audited", Status: abi.OutcomeSucceeded}})
	// Native records only: the conversation has no product history to wait for.
	p := NewNativePage(seed.service)
	p.ctx = t.Context()
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	p.selectSession(seed.session.ID)
	waitTranscript(t, tester, func() bool { return !p.polling && p.detail.Session.ID == seed.session.ID })
	openTraceView(t, tester, p)
	connectionClick(t, tester, "Trace step model/audited")
	settleTrace(t, tester, p)
	connectionClick(t, tester, "Trace detail 原始内容")
	settleTrace(t, tester, p)
	for _, record := range related {
		connectionClick(t, tester, fmt.Sprintf("Trace raw #%d", record.Sequence))
		settleTrace(t, tester, p)
		if p.trace.rawSeq != record.Sequence || !strings.Contains(p.trace.rawText, record.Type) {
			t.Fatal("raw button opened another record of the same role", record.Sequence, p.trace.rawSeq, p.trace.rawText)
		}
	}
}

// A persistent page error backs off instead of re-reading every frame.
func TestWorkspaceTracePageErrorBacksOff(t *testing.T) {
	gate := &gatedTraceInspector{}
	seed := newTraceSeedWith(t, gate)
	for range 30 {
		seed.turn(t, 1, "", false)
	}
	p, tester := seed.page(t)
	openTraceView(t, tester, p)
	gate.mu.Lock()
	gate.fail = olderPage
	gate.mu.Unlock()
	wheelTrace(t, tester, -240, func() bool { return gate.failed.Load() > 0 })
	waitTranscript(t, tester, func() bool { return !p.trace.loading })
	for range 30 {
		tester.Frame()
	}
	if calls := gate.failed.Load(); calls != 1 || p.trace.problem == "" {
		t.Fatal("failing older page was re-read every frame", calls, p.trace.problem)
	}
	p.pollTrace() // a new poll tick allows one more attempt
	waitTranscript(t, tester, func() bool { return gate.failed.Load() == 2 && !p.trace.loading })
	for range 30 {
		tester.Frame()
	}
	if calls := gate.failed.Load(); calls != 2 {
		t.Fatal("failing older page retried more than once per poll tick", calls)
	}
}
