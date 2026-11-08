// INPUT: Actual private product/native SQLite, the registered RepositoryInspector and committed Turns written through Store.Record
// OUTPUT: Trace pairing across pages, approximate legacy times, Message Stream Turn numbers, trimming, child enter/return and agent-call child lookup, unsupported engines, empty branches that start later, search jumps, details and layout regressions
// POS: [L7 Hub product] Execution-trace reader tests over real committed facts without a model, process or Agent restore
package product

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type traceFixture struct {
	s       *HubService
	log     storage.Session
	session hub.Session
	branch  string
	native  string
	turns   int
}

func newTraceFixture(t *testing.T) *traceFixture {
	t.Helper()
	s, _ := fixture(t)
	s.Traces = NewTraceService(s.Store, s)
	session, err := s.Create(t.Context(), "trace", "")
	if err != nil {
		t.Fatal(err)
	}
	native := "native-trace"
	log, err := s.Repository.Create(t.Context(), session.ID, native, "")
	if err != nil {
		t.Fatal(err)
	}
	if err = s.Store.BindNative(t.Context(), session.ID, session.ActiveBranchID, native); err != nil {
		t.Fatal(err)
	}
	return &traceFixture{s: s, log: log, session: session, branch: session.ActiveBranchID, native: native}
}

func (f *traceFixture) target() TraceTarget {
	return TraceTarget{SessionID: f.session.ID, BranchID: f.branch}
}

func (f *traceFixture) append(t *testing.T, records ...abi.SessionRecord) []abi.SessionRecord {
	t.Helper()
	for i := range records {
		records[i] = abi.BuiltinRecord(records[i])
	}
	committed, err := f.log.AppendRecords(t.Context(), records)
	if err != nil {
		t.Fatal(err)
	}
	return committed
}

func (f *traceFixture) record(t *testing.T, attempt string, event engine.Event) {
	t.Helper()
	if err := f.s.Store.Record(t.Context(), attempt, event, false); err != nil {
		t.Fatal(err)
	}
}

func ref(native string, record abi.SessionRecord) *engine.Reference {
	return &engine.Reference{SessionID: native, RecordID: record.ID, Sequence: record.Sequence}
}

func message(role wire.Role, text string) *wire.Message {
	m := wire.TextMessage(role, text)
	return &m
}

func filler(n int) []abi.SessionRecord {
	records := make([]abi.SessionRecord, n)
	for i := range records {
		records[i] = abi.SessionRecord{Kind: abi.RecordExtension, Namespace: "fixture", Type: "progress", SchemaVersion: 1, Payload: json.RawMessage(`{"tick":true}`)}
	}
	return records
}

type traceTurn struct {
	tools     int
	timed     bool // tool outcomes carry execution instants
	unanchor  bool // the attempt fails before any reply or tool event
	gap       int  // filler records between tool intents and outcomes
	open      bool // leave tools unsettled and the Turn running
	finalText string
}

// turn commits one product Turn and its native run. It returns the claim and
// the committed native records by name.
func (f *traceFixture) turn(t *testing.T, spec traceTurn) (hub.Claim, map[string]abi.SessionRecord) {
	t.Helper()
	f.turns++
	n := f.turns
	claim, err := f.s.Store.BeginTurn(t.Context(), f.session.ID, fmt.Sprintf("问题 %d", n))
	if err != nil {
		t.Fatal(err)
	}
	f.record(t, claim.Attempt.ID, engine.Event{Kind: engine.SessionStarted, SessionID: f.native})
	named := map[string]abi.SessionRecord{}
	first := f.append(t, abi.SessionRecord{Kind: abi.RecordConversation, Message: message(wire.RoleUser, fmt.Sprintf("问题 %d", n))}, abi.SessionRecord{Kind: abi.RecordModelIntent, ModelIntent: &abi.ModelIntent{RequestID: fmt.Sprintf("req-%d-a", n)}})
	named["input"], named["intent"] = first[0], first[1]
	if spec.unanchor {
		failed := f.append(t, abi.SessionRecord{Kind: abi.RecordModelOutcome, ModelOutcome: &abi.ModelOutcome{RequestID: fmt.Sprintf("req-%d-a", n), Status: abi.OutcomeFailed, Error: "provider unavailable"}})
		named["outcome"] = failed[0]
		f.record(t, claim.Attempt.ID, engine.Event{Kind: engine.Error, Text: "provider unavailable"})
		f.record(t, claim.Attempt.ID, engine.Event{Kind: engine.Completed, Outcome: engine.Failed})
		if err := f.s.Store.FinishTurn(t.Context(), claim.Turn.ID, engine.Failed); err != nil {
			t.Fatal(err)
		}
		return claim, named
	}
	run := fmt.Sprintf("run-%d", n)
	batch := []abi.SessionRecord{
		{Kind: abi.RecordConversation, Message: message(wire.RoleAssistant, fmt.Sprintf("第 %d 轮先调用工具", n))},
		{Kind: abi.RecordModelOutcome, ModelOutcome: &abi.ModelOutcome{RequestID: fmt.Sprintf("req-%d-a", n), Status: abi.OutcomeSucceeded, Usage: &wire.Usage{InputTokens: 100 + uint64(n), OutputTokens: 7}}},
	}
	for k := range spec.tools {
		batch = append(batch, abi.SessionRecord{Kind: abi.RecordToolIntent, ToolIntent: &abi.ToolIntent{RunID: run, Call: wire.ToolCall{ID: fmt.Sprintf("call-%d", k), Name: "read_file", Arguments: json.RawMessage(fmt.Sprintf(`{"path":"file-%d-%d.go"}`, n, k))}}})
	}
	committed := f.append(t, batch...)
	named["plan"], named["outcome"] = committed[0], committed[1]
	for k := range spec.tools {
		intent := committed[2+k]
		named[fmt.Sprintf("tool-%d", k)] = intent
		f.record(t, claim.Attempt.ID, engine.Event{Kind: engine.ToolStart, ID: intent.ToolIntent.Call.ID, Name: "read_file", Reference: ref(f.native, intent)})
	}
	if spec.gap > 0 {
		for remaining := spec.gap; remaining > 0; remaining -= 200 {
			f.append(t, filler(min(200, remaining))...)
		}
	}
	if spec.open {
		return claim, named
	}
	started := time.Now().UTC().Add(-time.Second)
	for k := range spec.tools {
		outcome := &abi.ToolOutcome{RunID: run, CallID: fmt.Sprintf("call-%d", k), Status: abi.OutcomeSucceeded, Result: wire.ToolResult{CallID: fmt.Sprintf("call-%d", k), Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: fmt.Sprintf("内容 %d-%d", n, k)}}}}
		if spec.timed {
			outcome.StartedAt, outcome.EndedAt = started, started.Add(250*time.Millisecond)
		}
		result := f.append(t, abi.SessionRecord{Kind: abi.RecordToolOutcome, ToolOutcome: outcome})
		named[fmt.Sprintf("result-%d", k)] = result[0]
		f.record(t, claim.Attempt.ID, engine.Event{Kind: engine.ToolEnd, ID: outcome.CallID, Name: "read_file", Reference: ref(f.native, result[0])})
	}
	f.finish(t, claim, named, n, spec.finalText)
	return claim, named
}

func (f *traceFixture) finish(t *testing.T, claim hub.Claim, named map[string]abi.SessionRecord, n int, text string) {
	t.Helper()
	if text == "" {
		text = fmt.Sprintf("第 %d 轮的回答", n)
	}
	f.append(t, abi.SessionRecord{Kind: abi.RecordModelIntent, ModelIntent: &abi.ModelIntent{RequestID: fmt.Sprintf("req-%d-b", n)}})
	final := f.append(t, abi.SessionRecord{Kind: abi.RecordConversation, Message: message(wire.RoleAssistant, text)}, abi.SessionRecord{Kind: abi.RecordModelOutcome, ModelOutcome: &abi.ModelOutcome{RequestID: fmt.Sprintf("req-%d-b", n), Status: abi.OutcomeSucceeded}})
	named["final"] = final[0]
	f.record(t, claim.Attempt.ID, engine.Event{Kind: engine.Message, ID: "assistant-1", Text: text, Reference: ref(f.native, final[0])})
	f.record(t, claim.Attempt.ID, engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded})
	if err := f.s.Store.FinishTurn(t.Context(), claim.Turn.ID, engine.Succeeded); err != nil {
		t.Fatal(err)
	}
}

func (f *traceFixture) tail(t *testing.T, w *TraceWindow) {
	t.Helper()
	chunk, err := f.s.Traces.Tail(t.Context(), f.target())
	if err != nil {
		t.Fatal(err)
	}
	w.Reset(chunk)
}

func stepsOf(w *TraceWindow, kind TraceStepKind) []TraceStep {
	var result []TraceStep
	for _, step := range w.Steps() {
		if step.Kind == kind {
			result = append(result, step)
		}
	}
	return result
}

func TestTraceTailPairsStepsAndMatchesMessageStreamTurns(t *testing.T) {
	f := newTraceFixture(t)
	f.turn(t, traceTurn{tools: 2, timed: true})
	f.turn(t, traceTurn{tools: 1})
	_, last := f.turn(t, traceTurn{tools: 1, timed: true})
	w := NewTraceWindow(0)
	f.tail(t, w)
	if w.MoreBefore() || w.MoreAfter() || w.Unsupported() || w.NativeID() != f.native {
		t.Fatal("short session window state", w.MoreBefore(), w.MoreAfter(), w.Unsupported())
	}
	steps := w.Steps()
	// Per Turn: input, planning model call, tools, final model call.
	if len(steps) != 3*3+4 {
		t.Fatal("unexpected step count", len(steps))
	}
	for i, step := range steps {
		if step.Turn == 0 || !step.OrdinalExact {
			t.Fatal("step lacks a proven Turn or exact ordinal", i, step)
		}
		if step.Kind == TraceStepModel && (step.Open || step.Status != "succeeded" || !step.Approximate) {
			t.Fatal("model intent/outcome not paired by RequestID", step)
		}
	}
	tools := stepsOf(w, TraceStepTool)
	if len(tools) != 4 || tools[0].Approximate || tools[0].Duration() != 250*time.Millisecond || !tools[2].Approximate || tools[2].Open || tools[2].Start.After(tools[2].End) {
		t.Fatal("tool times do not prefer execution instants with commit-time fallback", tools)
	}
	if steps[0].Kind != TraceStepInput || steps[0].Summary != "问题 1" || steps[0].Ordinal != 1 || steps[1].Ordinal != 2 || steps[1].Summary != "第 1 轮先调用工具" {
		t.Fatal("first Turn steps", steps[0], steps[1])
	}
	if step, ok := w.StepAt(last["result-0"].Sequence); !ok || step.Kind != TraceStepTool || step.Turn != 3 {
		t.Fatal("record lookup did not find its paired step", step, ok)
	}
	// The Message Stream numbers its blocks with the same Turn ordinals.
	transcripts := &TranscriptService{Store: f.s.Store}
	chunk, err := transcripts.Tail(t.Context(), f.session.ID)
	if err != nil {
		t.Fatal(err)
	}
	stream := NewTranscriptWindow(0)
	stream.Reset(chunk)
	compared := 0
	for _, block := range stream.Blocks() {
		if block.Ref == nil || block.Ref.Sequence == 0 {
			continue
		}
		step, ok := w.StepAt(block.Ref.Sequence)
		if !ok || step.Turn != block.Turn {
			t.Fatal("trace Turn differs from the Message Stream", block.ID, block.Turn, step.Turn)
		}
		compared++
	}
	// Four tool blocks and three final replies carry native references.
	if compared != 7 {
		t.Fatal("unexpected Message Stream blocks compared", compared)
	}
}

func TestTracePairsAnIntentWithItsOutcomeFromALaterPage(t *testing.T) {
	f := newTraceFixture(t)
	f.turn(t, traceTurn{tools: 1})
	claim, named := f.turn(t, traceTurn{tools: 1, open: true})
	w := NewTraceWindow(0)
	f.tail(t, w)
	running := stepsOf(w, TraceStepTool)
	if len(running) != 2 || !running[1].Open || running[1].Status != "running" || running[1].Turn != 2 {
		t.Fatal("unsettled tool is not an open running step", running)
	}
	outcome := f.append(t, abi.SessionRecord{Kind: abi.RecordToolOutcome, ToolOutcome: &abi.ToolOutcome{RunID: "run-2", CallID: "call-0", Status: abi.OutcomeFailed, Result: wire.ToolResult{CallID: "call-0", IsError: true}}})
	f.record(t, claim.Attempt.ID, engine.Event{Kind: engine.ToolEnd, ID: "call-0", Name: "read_file", Reference: ref(f.native, outcome[0]), IsError: true})
	f.finish(t, claim, named, 2, "")
	chunk, err := f.s.Traces.Newer(t.Context(), f.target(), w.Cursor())
	if err != nil {
		t.Fatal(err)
	}
	w.ApplyNewer(chunk)
	paired := stepsOf(w, TraceStepTool)
	if len(paired) != 2 || paired[1].Open || paired[1].Status != "failed" || paired[1].Last != outcome[0].Sequence || len(paired[1].Records) != 2 {
		t.Fatal("next page did not complete the pending pair", paired)
	}
	models := stepsOf(w, TraceStepModel)
	if last := models[len(models)-1]; last.Turn != 2 || last.Open || last.Summary != "第 2 轮的回答" {
		t.Fatal("newer model call lost its Turn or pairing", last)
	}
}

func TestTraceOlderPagePairsAnOutcomeWhoseIntentWasAboveTheWindow(t *testing.T) {
	f := newTraceFixture(t)
	f.turn(t, traceTurn{tools: 1})
	_, named := f.turn(t, traceTurn{tools: 1, gap: 260})
	w := NewTraceWindow(0)
	f.tail(t, w)
	if !w.MoreBefore() {
		t.Fatal("long tool call did not leave earlier records")
	}
	if _, ok := w.Step("tool/run-2/call-0"); ok {
		t.Fatal("an outcome without its intent became a step")
	}
	for w.MoreBefore() {
		chunk, err := f.s.Traces.Older(t.Context(), f.target(), w.Cursor())
		if err != nil {
			t.Fatal(err)
		}
		w.ApplyOlder(chunk)
	}
	step, ok := w.Step("tool/run-2/call-0")
	if !ok || step.Open || step.First != named["tool-0"].Sequence || step.Last != named["result-0"].Sequence || step.Turn != 2 {
		t.Fatal("older page did not pair the tool call across pages", step, ok)
	}
	if first := w.Steps()[0]; first.Turn != 1 || first.Ordinal != 1 || !first.OrdinalExact {
		t.Fatal("paging to the start lost exact numbering", first)
	}
}

func TestTraceLegacyRecordsUseApproximateCommitTimes(t *testing.T) {
	f := newTraceFixture(t)
	f.turn(t, traceTurn{tools: 1, timed: false})
	w := NewTraceWindow(0)
	f.tail(t, w)
	tool := stepsOf(w, TraceStepTool)[0]
	intent, _ := tool.Record("intent")
	outcome, _ := tool.Record("outcome")
	if !tool.Approximate || !tool.Start.Equal(intent.Time) || !tool.End.Equal(outcome.Time) {
		t.Fatal("legacy tool step did not fall back to approximate commit times", tool)
	}
}

func TestTraceTurnForFailedRunWithoutAnchors(t *testing.T) {
	f := newTraceFixture(t)
	f.turn(t, traceTurn{tools: 1})
	f.turn(t, traceTurn{unanchor: true})
	f.turn(t, traceTurn{tools: 1})
	w := NewTraceWindow(0)
	f.tail(t, w)
	inputs := stepsOf(w, TraceStepInput)
	if len(inputs) != 3 || inputs[1].Turn != 2 || inputs[2].Turn != 3 {
		t.Fatal("unanchored failed run was not labelled by its unique Attempt", inputs)
	}
	models := stepsOf(w, TraceStepModel)
	if models[2].Turn != 2 || models[2].Status != "failed" {
		t.Fatal("failed model call", models[2])
	}
}

func TestTraceWindowTrimsFarEndAndRebuildsOnPaging(t *testing.T) {
	f := newTraceFixture(t)
	for range 12 {
		f.turn(t, traceTurn{tools: 3, timed: true, gap: 20})
	}
	full := NewTraceWindow(0)
	f.tail(t, full)
	for full.MoreBefore() {
		chunk, err := f.s.Traces.Older(t.Context(), f.target(), full.Cursor())
		if err != nil {
			t.Fatal(err)
		}
		full.ApplyOlder(chunk)
	}
	want := map[string]uint64{}
	for _, step := range full.Steps() {
		want[step.ID] = step.Turn
	}
	w := NewTraceWindow(24 << 10)
	f.tail(t, w)
	w.Trim(true)
	if w.Bytes() > 24<<10 || !w.MoreBefore() {
		t.Fatal("tail window not trimmed from its oldest end", w.Bytes())
	}
	for w.MoreBefore() {
		chunk, err := f.s.Traces.Older(t.Context(), f.target(), w.Cursor())
		if err != nil {
			t.Fatal(err)
		}
		w.ApplyOlder(chunk)
		w.Trim(false)
		if w.Bytes() > 24<<10 {
			t.Fatal("older page exceeded the byte limit", w.Bytes())
		}
		if !w.MoreAfter() {
			t.Fatal("trimming the newest end did not report newer records")
		}
		for _, step := range w.Steps() {
			if turn, ok := want[step.ID]; !ok || step.Turn != turn {
				t.Fatal("trimmed window relabelled a step", step.ID, step.Turn, turn)
			}
		}
	}
	if first := w.Steps()[0]; first.Turn != 1 || first.Kind != TraceStepInput {
		t.Fatal("paging back through a trimmed window did not reach the first Turn", first)
	}
}

func TestTraceChildEnterAndReturn(t *testing.T) {
	f := newTraceFixture(t)
	_, named := f.turn(t, traceTurn{tools: 1, open: true})
	intent := named["tool-0"]
	child, err := f.s.Repository.CreateChild(t.Context(), f.session.ID, f.native, "native-child", "run-1", "call-0", "scope")
	if err != nil {
		t.Fatal(err)
	}
	childRecords := []abi.SessionRecord{
		abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: message(wire.RoleUser, "子任务")}),
		abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordModelIntent, ModelIntent: &abi.ModelIntent{RequestID: "child-req"}}),
	}
	if _, err = child.Session.AppendRecords(t.Context(), childRecords); err != nil {
		t.Fatal(err)
	}
	w := NewTraceWindow(0)
	f.tail(t, w)
	children := stepsOf(w, TraceStepChild)
	if len(children) != 1 || children[0].ChildLink != child.Link.Sequence || children[0].Name != "read_file" || children[0].Turn != 1 {
		t.Fatal("child link step", children)
	}
	// An agent call's tool block reaches its child step through the intent its
	// ToolStart references; any other record names no child.
	if step, ok := w.ChildStepOf(intent.Sequence); !ok || step.ID != children[0].ID || step.ChildLink != child.Link.Sequence {
		t.Fatal("tool intent did not resolve its child step", step, ok)
	}
	if _, ok := w.ChildStepOf(named["input"].Sequence); ok {
		t.Fatal("a record without a child link resolved a child step")
	}
	parentSteps := len(w.Steps())
	target, err := f.target().Child(children[0])
	if err != nil {
		t.Fatal(err)
	}
	childChunk, err := f.s.Traces.Tail(t.Context(), target)
	if err != nil {
		t.Fatal(err)
	}
	inner := NewTraceWindow(0)
	inner.Reset(childChunk)
	steps := inner.Steps()
	if inner.NativeID() != "native-child" || len(steps) != 2 || steps[0].Turn != 1 || steps[1].Turn != 1 || !steps[1].Open {
		t.Fatal("child trace did not inherit its parent step's Turn", inner.NativeID(), steps)
	}
	detail, err := f.s.Traces.Detail(t.Context(), f.target(), children[0])
	if err != nil || detail.ChildID != "native-child" || detail.NativeID != f.native {
		t.Fatal("child step detail", detail, err)
	}
	// Returning is the parent's retained window: unchanged by the child read.
	if len(w.Steps()) != parentSteps || w.NativeID() != f.native {
		t.Fatal("entering the child changed the parent window")
	}
	if _, err = f.s.Traces.Tail(t.Context(), TraceTarget{SessionID: f.session.ID, BranchID: f.branch, Path: []ChildHop{{LinkSequence: intent.Sequence}}}); err == nil {
		t.Fatal("a non-link record was accepted as a child hop")
	}
	deep := f.target()
	deep.Path = make([]ChildHop, 8)
	if _, err = deep.Child(children[0]); err == nil {
		t.Fatal("child depth limit not enforced")
	}
	if _, err = f.target().Child(stepsOf(w, TraceStepTool)[0]); err == nil {
		t.Fatal("a tool step was entered as a child")
	}
}

type traceForeignRuntime struct{ engine.Runtime }

func (traceForeignRuntime) Capabilities() engine.Capabilities { return engine.Capabilities{} }
func (traceForeignRuntime) Execute(context.Context, engine.Request, func(engine.Event) error) error {
	panic("trace observation must not execute")
}

func TestTraceUnsupportedEngineAndEmptyBranch(t *testing.T) {
	f := newTraceFixture(t)
	if err := f.s.Runtime.RegisterDescriptor("foreign", engine.Descriptor{Runtime: traceForeignRuntime{}, Config: engine.ConfigSurface{ProviderProtocol: "native"}}); err != nil {
		t.Fatal(err)
	}
	target := f.s.DefaultTarget
	target.EngineID = "foreign"
	branch, err := f.s.Store.Switch(t.Context(), f.session.ID, target, "foreign engine branch")
	if err != nil {
		t.Fatal(err)
	}
	if err = f.s.Store.BindNative(t.Context(), f.session.ID, branch.ID, "foreign-native"); err != nil {
		t.Fatal(err)
	}
	foreign := TraceTarget{SessionID: f.session.ID, BranchID: branch.ID}
	chunk, err := f.s.Traces.Tail(t.Context(), foreign)
	if err != nil || !chunk.Unsupported {
		t.Fatal("engine without Inspector did not report an unsupported trace", chunk, err)
	}
	w := NewTraceWindow(0)
	w.Reset(chunk)
	if !w.Unsupported() || len(w.Steps()) != 0 {
		t.Fatal("unsupported window state")
	}
	if _, err = f.s.Traces.Search(t.Context(), foreign, "x", 0, 0); !errors.Is(err, engine.ErrUnsupported) {
		t.Fatal("unsupported search", err)
	}
	empty, err := f.s.Create(t.Context(), "empty", "")
	if err != nil {
		t.Fatal(err)
	}
	chunk, err = f.s.Traces.Tail(t.Context(), TraceTarget{SessionID: empty.ID, BranchID: empty.ActiveBranchID})
	if err != nil || chunk.Unsupported || len(chunk.entries) != 0 {
		t.Fatal("branch without native records is not an empty trace", chunk, err)
	}
	// The first run binds a native session; following the empty window adopts it.
	emptyWindow := NewTraceWindow(0)
	emptyWindow.Reset(chunk)
	log, err := f.s.Repository.Create(t.Context(), empty.ID, "native-later", "")
	if err != nil {
		t.Fatal(err)
	}
	if err = f.s.Store.BindNative(t.Context(), empty.ID, empty.ActiveBranchID, "native-later"); err != nil {
		t.Fatal(err)
	}
	if _, err = log.AppendRecords(t.Context(), []abi.SessionRecord{abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: message(wire.RoleUser, "first run")})}); err != nil {
		t.Fatal(err)
	}
	newer, err := f.s.Traces.Newer(t.Context(), TraceTarget{SessionID: empty.ID, BranchID: empty.ActiveBranchID}, emptyWindow.Cursor())
	if err != nil {
		t.Fatal(err)
	}
	emptyWindow.ApplyNewer(newer)
	if emptyWindow.NativeID() != "native-later" || len(emptyWindow.Steps()) != 1 || emptyWindow.Steps()[0].Summary != "first run" {
		t.Fatal("empty trace did not follow the first native run", emptyWindow.NativeID(), emptyWindow.Steps())
	}
	if _, err = f.s.Traces.Tail(t.Context(), TraceTarget{SessionID: f.session.ID, BranchID: "missing"}); err == nil {
		t.Fatal("unknown branch authorized")
	}
	if f.s.Runtime.HasActive() {
		t.Fatal("trace reads started execution")
	}
}

func TestTraceSearchJumpsAndDetails(t *testing.T) {
	f := newTraceFixture(t)
	_, early := f.turn(t, traceTurn{tools: 1, timed: true, finalText: "needle-early answer"})
	for range 6 {
		f.turn(t, traceTurn{tools: 2, gap: 40})
	}
	page, err := f.s.Traces.Search(t.Context(), f.target(), "needle-early", 0, 0)
	for err == nil && len(page.Hits) == 0 && page.HasMore {
		page, err = f.s.Traces.Search(t.Context(), f.target(), "needle-early", page.Next, page.HighWater)
	}
	if err != nil || len(page.Hits) == 0 || page.Hits[0].Sequence != early["final"].Sequence {
		t.Fatal("literal search hit", page, err)
	}
	w := NewTraceWindow(0)
	f.tail(t, w)
	if _, ok := w.StepAt(page.Hits[0].Sequence); ok {
		t.Fatal("fixture hit is already inside the tail window")
	}
	chunk, err := f.s.Traces.Around(t.Context(), f.target(), page.Hits[0].Sequence, TraceHint{})
	if err != nil {
		t.Fatal(err)
	}
	w.Reset(chunk)
	step, ok := w.StepAt(page.Hits[0].Sequence)
	if !ok || step.Kind != TraceStepModel || step.Turn != 1 || step.Summary != "needle-early answer" {
		t.Fatal("jump did not load the hit's step", step, ok)
	}
	if !w.MoreAfter() {
		t.Fatal("jump window does not report newer records")
	}
	detail, err := f.s.Traces.Detail(t.Context(), f.target(), step)
	if err != nil || detail.Message != "needle-early answer" || detail.Usage != nil {
		t.Fatal("model detail", detail, err)
	}
	planning := w.Steps()[1]
	detail, err = f.s.Traces.Detail(t.Context(), f.target(), planning)
	if err != nil || detail.Usage == nil || detail.Usage.InputTokens != 101 || !strings.Contains(detail.Message, "先调用工具") {
		t.Fatal("model usage detail", detail, err)
	}
	tool, _ := w.Step("tool/run-1/call-0")
	detail, err = f.s.Traces.Detail(t.Context(), f.target(), tool)
	if err != nil || !strings.Contains(detail.Arguments, "file-1-0.go") || detail.Result != "内容 1-0" {
		t.Fatal("tool detail", detail, err)
	}
	outcome, _ := tool.Record("outcome")
	raw, err := f.s.Traces.Raw(t.Context(), f.target(), outcome.URI, 0)
	if err != nil || !strings.Contains(raw.Text, `"tool_outcome"`) || len(raw.Text) > 2048 {
		t.Fatal("raw record page", raw, err)
	}
	// A Message Stream jump carries the block's Turn and history position.
	hinted, err := f.s.Traces.Around(t.Context(), f.target(), early["result-0"].Sequence, TraceHint{Turn: 1, History: 3})
	if err != nil {
		t.Fatal(err)
	}
	w.Reset(hinted)
	if step, ok := w.StepAt(early["result-0"].Sequence); !ok || step.Turn != 1 || step.Kind != TraceStepTool {
		t.Fatal("hinted jump", step, ok)
	}
}

func TestLayoutTraceScales(t *testing.T) {
	f := newTraceFixture(t)
	f.turn(t, traceTurn{tools: 2, timed: true})
	f.turn(t, traceTurn{tools: 1})
	w := NewTraceWindow(0)
	f.tail(t, w)
	steps := w.Steps()
	for _, scale := range []TraceScale{TraceScaleDuration, TraceScaleTurn, TraceScaleCall, TraceScaleStep} {
		layout := LayoutTrace(steps, scale)
		if len(layout.Bars) != len(steps) {
			t.Fatal("layout lost steps", scale)
		}
		for _, bar := range layout.Bars {
			if bar.X0 < 0 || bar.X1 > 1.0001 || bar.X1 < bar.X0 || bar.Lane != LaneOf(steps[bar.Step].Kind) {
				t.Fatal("bar outside the axis or lane", scale, bar)
			}
		}
		switch scale {
		case TraceScaleDuration:
			if len(layout.Groups) != 0 || !layout.To.After(layout.From) {
				t.Fatal("duration extent", layout.From, layout.To)
			}
		case TraceScaleTurn:
			if len(layout.Groups) != 2 || layout.Groups[0].Label != "第 1 轮" || layout.Groups[1].X0 <= layout.Groups[0].X1 {
				t.Fatal("turn groups", layout.Groups)
			}
		case TraceScaleCall:
			// Per Turn: input, planning call with its tools, final call.
			if len(layout.Groups) != 6 {
				t.Fatal("call groups", layout.Groups)
			}
		case TraceScaleStep:
			if len(layout.Groups) != len(steps) {
				t.Fatal("step groups", len(layout.Groups))
			}
		}
	}
	if layout := LayoutTrace(nil, TraceScaleTurn); len(layout.Bars) != 0 {
		t.Fatal("empty layout")
	}
}

// syntheticTurn is one native run of five records starting at sequence from:
// input, model intent/outcome, tool intent/outcome. Its tool intent is the
// product-anchored record, at product sequence from.
func syntheticTurn(turn, from uint64) ([]traceEntry, traceAnchor, traceStart) {
	at := time.Date(2026, 11, 7, 0, 0, int(from), 0, time.UTC)
	request, call := fmt.Sprintf("req-%d", turn), fmt.Sprintf("call-%d", turn)
	entries := []traceEntry{
		{sequence: from, time: at, kind: "conversation", summary: fmt.Sprintf("user: 问题 %d", turn)},
		{sequence: from + 1, time: at, kind: "model_intent", requestID: request},
		{sequence: from + 2, time: at, kind: "model_outcome", requestID: request, status: "succeeded"},
		{sequence: from + 3, time: at, kind: "tool_intent", runID: "run", callID: call, summary: "read_file " + call},
		{sequence: from + 4, time: at, kind: "tool_outcome", runID: "run", callID: call, status: "succeeded"},
	}
	return entries, traceAnchor{native: from + 3, turn: turn, product: from}, traceStart{turn: turn, product: from - 1}
}

// keepBytes is a window limit that holds exactly entries plus n anchors/starts.
func keepBytes(entries []traceEntry, n int) int {
	total := 40 * n
	for i := range entries {
		total += entries[i].size()
	}
	return total
}

func turnsBySummary(w *TraceWindow) map[string]uint64 {
	result := map[string]uint64{}
	for _, step := range w.Steps() {
		if step.Kind == TraceStepInput {
			result[step.Summary] = step.Turn
		}
	}
	return result
}

// Trimming the newest end keeps anchors below the window that the scanned
// product range still covers: the next older page needs them.
func TestTraceWindowTrimNewestKeepsAnchorsBelowTheWindow(t *testing.T) {
	one, a1, s1 := syntheticTurn(1, 1)
	two, a2, s2 := syntheticTurn(2, 6)
	three, a3, s3 := syntheticTurn(3, 11)
	entries := append(append([]traceEntry(nil), two...), three...)
	w := NewTraceWindow(0)
	// An older scan overshot below the window into Turn 1's events.
	w.Reset(TraceChunk{NativeID: "n", MoreBefore: true, HighWater: 15, entries: entries, anchors: []traceAnchor{a1, a2, a3}, starts: []traceStart{s1, s2, s3}, product: traceProductRange{low: 1, high: 11}})
	w.limit = keepBytes(two, 3)
	w.Trim(false)
	if w.Last() >= 11 || !w.MoreAfter() {
		t.Fatal("newest end was not trimmed", w.Last())
	}
	// The service rescans product history only below the kept low bound, so
	// Turn 1's anchor is not read again.
	w.limit = traceDefaultBytes
	w.ApplyOlder(TraceChunk{NativeID: "n", MoreBefore: true, entries: one})
	if turns := turnsBySummary(w); turns["问题 1"] != 1 || turns["问题 2"] != 2 {
		t.Fatal("trimming the newest end lost the anchor below the window", turns)
	}
}

// The mirror: trimming the oldest end keeps anchors above the window that the
// scanned product range covers, for the next newer page.
func TestTraceWindowTrimOldestKeepsAnchorsAboveTheWindow(t *testing.T) {
	one, a1, s1 := syntheticTurn(1, 1)
	two, a2, s2 := syntheticTurn(2, 6)
	three, a3, s3 := syntheticTurn(3, 11)
	entries := append(append([]traceEntry(nil), one...), two...)
	w := NewTraceWindow(0)
	// A forward scan overshot above the window into Turn 3's events.
	w.Reset(TraceChunk{NativeID: "n", MoreAfter: true, HighWater: 15, entries: entries, anchors: []traceAnchor{a1, a2, a3}, starts: []traceStart{s1, s2, s3}, product: traceProductRange{low: 1, high: 11, start: true}})
	w.limit = keepBytes(two, 3)
	w.Trim(true)
	if w.First() <= 1 || !w.MoreBefore() {
		t.Fatal("oldest end was not trimmed", w.First())
	}
	if cursor := w.Cursor(); cursor.product.start || cursor.product.low <= 1 {
		t.Fatal("trimming the oldest end did not move the product low bound to the kept anchors", cursor.product)
	}
	w.limit = traceDefaultBytes
	w.ApplyNewer(TraceChunk{NativeID: "n", MoreAfter: true, HighWater: 20, entries: three})
	if turns := turnsBySummary(w); turns["问题 3"] != 3 || turns["问题 2"] != 2 {
		t.Fatal("trimming the oldest end lost the anchor above the window", turns)
	}
}

// forwardOnlyInspector is an Inspector written before backward pages: it does
// not declare them, so it may ignore Before.
type forwardOnlyInspector struct{ engine.Inspector }

func (i forwardOnlyInspector) InspectionCapabilities() engine.InspectionCapabilities {
	capabilities := i.Inspector.InspectionCapabilities()
	capabilities.BackwardPages = false
	return capabilities
}

func TestTraceTreatsInspectorWithoutBackwardPagesAsUnsupported(t *testing.T) {
	f := newTraceFixture(t)
	f.turn(t, traceTurn{tools: 1})
	descriptor, _ := f.s.Runtime.Descriptor("alva")
	if err := f.s.Runtime.RegisterDescriptor("legacy", engine.Descriptor{Runtime: traceForeignRuntime{}, Inspector: forwardOnlyInspector{descriptor.Inspector}, Config: engine.ConfigSurface{ProviderProtocol: "native"}}); err != nil {
		t.Fatal(err)
	}
	target := f.s.DefaultTarget
	target.EngineID = "legacy"
	branch, err := f.s.Store.Switch(t.Context(), f.session.ID, target, "forward-only Inspector branch")
	if err != nil {
		t.Fatal(err)
	}
	if err = f.s.Store.BindNative(t.Context(), f.session.ID, branch.ID, f.native); err != nil {
		t.Fatal(err)
	}
	chunk, err := f.s.Traces.Tail(t.Context(), TraceTarget{SessionID: f.session.ID, BranchID: branch.ID})
	if err != nil || !chunk.Unsupported || len(chunk.entries) != 0 {
		t.Fatal("an Inspector that may ignore Before was paged backward", chunk.Unsupported, len(chunk.entries), err)
	}
	if _, err = f.s.InspectorBeforeAt(t.Context(), f.session.ID, branch.ID, nil, 0, 0); !errors.Is(err, engine.ErrUnsupported) {
		t.Fatal("backward Inspector page used an Inspector without the capability", err)
	}
	// Forward reads keep working for it.
	if page, err := f.s.InspectorAt(t.Context(), f.session.ID, branch.ID, nil, "", 0, 0); err != nil || len(page.Entries) == 0 {
		t.Fatal("forward Inspector page", err)
	}
}
