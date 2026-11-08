// INPUT: Actual private native record storage, product TranscriptService/TraceService pages, cold Inspector adapter and headless MyGo transcript controls.
// OUTPUT: Canonical light/dark message geometry, tool blocks opening their trace step, stale binding rejection and scroll-driven paging.
// POS: [L7 Hub UI tests] Exercises retained original evidence without launching a model, process or OS window.
//go:build desktop

package hubdesktop

import (
	"context"
	"encoding/json"
	"errors"
	"math"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/apps/hub/product"
	view "github.com/QuincySx/Alva/apps/ui/chat"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/engine/alva"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
)

// fixedHistory serves fixture entries through the product TranscriptService, so
// tests reach the window the same way a reopened page does.
type fixedHistory []hub.HistoryEntry

func (h fixedHistory) History(_ context.Context, _ string, after uint64, limit int) ([]hub.HistoryEntry, error) {
	var page []hub.HistoryEntry
	for _, entry := range h {
		if entry.Sequence > after && len(page) < limit {
			page = append(page, entry)
		}
	}
	return page, nil
}

func (h fixedHistory) HistoryBefore(_ context.Context, _ string, before uint64, limit int) ([]hub.HistoryEntry, error) {
	var page []hub.HistoryEntry
	for i := len(h) - 1; i >= 0 && len(page) < limit; i-- {
		if before == 0 || h[i].Sequence < before {
			page = append([]hub.HistoryEntry{h[i]}, page...)
		}
	}
	return page, nil
}

func replaceTranscript(t *testing.T, p *NativePage, entries ...hub.HistoryEntry) {
	t.Helper()
	chunk, err := (&product.TranscriptService{Store: fixedHistory(entries)}).Tail(t.Context(), p.selected)
	if err != nil {
		t.Fatal(err)
	}
	p.transcript.Reset(chunk)
	p.projectTranscript()
}

func appendTranscript(t *testing.T, p *NativePage, entries ...hub.HistoryEntry) {
	t.Helper()
	chunk, err := (&product.TranscriptService{Store: fixedHistory(entries)}).Newer(t.Context(), p.selected, p.transcript.Last())
	if err != nil {
		t.Fatal(err)
	}
	p.transcript.ApplyNewer(chunk)
	p.projectTranscript()
}

func transcriptFixtureHistory(session, native string, reference *engine.Reference, failed bool) []hub.HistoryEntry {
	return []hub.HistoryEntry{
		{Sequence: 1, SessionID: session, TurnID: "turn", Input: "User paragraph"},
		{Sequence: 2, SessionID: session, AttemptID: "attempt", Event: engine.Event{Kind: engine.ToolEnd, SessionID: native, ID: "tool", Name: "demo_echo", Text: "Original result summary", Reference: reference, IsError: failed}},
		{Sequence: 3, SessionID: session, AttemptID: "attempt", Event: engine.Event{Kind: engine.Message, ID: "reply", Text: "Assistant paragraph"}},
	}
}

type transcriptColdRuntime struct{ engine.Runtime }

func (transcriptColdRuntime) Capabilities() engine.Capabilities { return engine.Capabilities{} }
func (transcriptColdRuntime) Execute(context.Context, engine.Request, func(engine.Event) error) error {
	panic("transcript observation must not execute")
}

type transcriptInspector struct {
	engine.Inspector
	pages, reads atomic.Int32
	failReads    atomic.Int32 // reads that fail before reaching the store
	entered      chan struct{}
	release      <-chan struct{}
	readEntered  chan struct{}
	readRelease  <-chan struct{}
}

func (i *transcriptInspector) Page(ctx context.Context, scope engine.InspectionScope, query engine.InspectionQuery) (engine.InspectionPage, error) {
	i.pages.Add(1)
	if i.entered != nil {
		i.entered <- struct{}{}
		select {
		case <-i.release:
		case <-ctx.Done():
			return engine.InspectionPage{}, ctx.Err()
		}
	}
	return i.Inspector.Page(ctx, scope, query)
}

func (i *transcriptInspector) Read(ctx context.Context, scope engine.InspectionScope, uri string, offset int64) (engine.InspectionText, error) {
	i.reads.Add(1)
	if i.failReads.Load() > 0 {
		i.failReads.Add(-1)
		return engine.InspectionText{}, errors.New("fixture read failure")
	}
	if i.readEntered != nil {
		i.readEntered <- struct{}{}
		select {
		case <-i.readRelease:
		case <-ctx.Done():
			return engine.InspectionText{}, ctx.Err()
		}
	}
	return i.Inspector.Read(ctx, scope, uri, offset)
}

func transcriptPageFixture(t *testing.T, failed, noReference bool) (*NativePage, *transcriptInspector, engine.Reference) {
	t.Helper()
	s, _ := fixture(t)
	session, err := s.Create(t.Context(), "Transcript", "")
	if err != nil {
		t.Fatal(err)
	}
	native := "native-transcript"
	log, err := s.Repository.Create(t.Context(), session.ID, native, "")
	if err != nil {
		t.Fatal(err)
	}
	user := wire.TextMessage(wire.RoleUser, "User paragraph")
	plan := wire.TextMessage(wire.RoleAssistant, "Calling the echo tool")
	status := abi.OutcomeSucceeded
	if failed {
		status = abi.OutcomeFailed
	}
	var records []abi.SessionRecord
	for _, batch := range [][]abi.SessionRecord{
		{{Kind: abi.RecordConversation, Message: &user}, {Kind: abi.RecordModelIntent, ModelIntent: &abi.ModelIntent{RequestID: "request"}}},
		{{Kind: abi.RecordConversation, Message: &plan}, {Kind: abi.RecordModelOutcome, ModelOutcome: &abi.ModelOutcome{RequestID: "request", Status: abi.OutcomeSucceeded}},
			{Kind: abi.RecordToolIntent, ToolIntent: &abi.ToolIntent{RunID: "run", Call: wire.ToolCall{ID: "tool", Name: "demo_echo", Arguments: json.RawMessage(`{"text":"echo"}`)}}}},
		{{Kind: abi.RecordToolOutcome, ToolOutcome: &abi.ToolOutcome{RunID: "run", CallID: "tool", Status: status, Result: wire.ToolResult{CallID: "tool", IsError: failed, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: "exact retained original tool result"}}}}}},
	} {
		for i := range batch {
			batch[i] = abi.BuiltinRecord(batch[i])
		}
		committed, err := log.AppendRecords(t.Context(), batch)
		if err != nil {
			t.Fatal(err)
		}
		records = append(records, committed...)
	}
	records = records[len(records)-1:]
	if err = s.Store.BindNative(t.Context(), session.ID, session.ActiveBranchID, native); err != nil {
		t.Fatal(err)
	}
	nativeInspector, err := alva.NewRepositoryInspector(s.Repository)
	if err != nil {
		t.Fatal(err)
	}
	inspector := &transcriptInspector{Inspector: nativeInspector}
	if err = s.Runtime.RegisterDescriptor("alva", engine.Descriptor{Runtime: transcriptColdRuntime{}, Inspector: inspector, Config: engine.ConfigSurface{ProviderProtocol: "native"}}); err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(s)
	p.ctx = t.Context()
	p.selected = session.ID
	p.detail, err = s.Store.Detail(t.Context(), session.ID)
	if err != nil {
		t.Fatal(err)
	}
	reference := engine.Reference{SessionID: native, RecordID: records[0].ID, Sequence: records[0].Sequence}
	var ref *engine.Reference
	if !noReference {
		ref = &reference
	}
	replaceTranscript(t, p, transcriptFixtureHistory(session.ID, native, ref, failed)...)
	p.applyDetail()
	return p, inspector, reference
}

func transcriptPreferences(t *testing.T, p *NativePage) *product.PreferencesService {
	t.Helper()
	store, ok := p.Service.Store.(hub.PreferencesStore)
	if !ok {
		t.Fatal("fixture has no preference store")
	}
	preferences, err := product.NewPreferencesService(t.Context(), store)
	if err != nil {
		t.Fatal(err)
	}
	p.Service.Preferences = preferences
	t.Cleanup(func() { _ = preferences.Close(context.Background()) })
	return preferences
}

// The resource composition fixture reads these committed records through the
// same selectSession/history path as a reopened product session. No runtime runs.
func seedWorkspaceResourceConversation(t *testing.T, app *bootstrap.Application, session hub.Session) {
	t.Helper()
	claim, err := app.Service.Store.BeginTurn(t.Context(), session.ID, "把模型连接和伙伴群聊的流程整理成一份可实现的方案。")
	if err != nil {
		t.Fatal(err)
	}
	for _, event := range []engine.Event{
		{Kind: engine.SessionStarted, SessionID: "native-workspace-render-fixture"},
		{Kind: engine.Message, ID: "reply", Text: "模型连接、伙伴协作和执行反馈已分好结构，正在补充交互细节。"},
		{Kind: engine.Completed, Outcome: engine.Succeeded},
	} {
		if err := app.Service.Store.Record(t.Context(), claim.Attempt.ID, event, false); err != nil {
			t.Fatal(err)
		}
	}
	if err := app.Service.Store.FinishTurn(t.Context(), claim.Turn.ID, engine.Succeeded); err != nil {
		t.Fatal(err)
	}
}

// settleTrace waits for the trace page and the selected step's detail.
func settleTrace(t *testing.T, tester *ui.Tester, p *NativePage) {
	t.Helper()
	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		tester.Frame()
		if !p.trace.loading && !p.trace.detailBusy && !p.trace.rawBusy && !p.trace.searchBusy {
			tester.Frame()
			return
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("trace view did not settle")
}

func TestWorkspaceTranscriptToolOpensItsTraceStep(t *testing.T) {
	for _, failed := range []bool{false, true} {
		t.Run(map[bool]string{false: "completed", true: "failed"}[failed], func(t *testing.T) {
			p, inspector, reference := transcriptPageFixture(t, failed, false)
			tester := ui.NewTester(workspaceStaticView(p), 1440, 960)
			if inspector.pages.Load() != 0 || inspector.reads.Load() != 0 || tester.HasText("Original result summary") || strings.Contains(strings.Join(tester.Texts(), "\n"), "Reference:") {
				t.Fatal("collapsed tool read or dumped original evidence", tester.Texts())
			}
			activity, ok := tester.Find("Tool activity attempt/tool")
			if !ok || math.Abs(float64(activity.H-37)) > .01 || activity.W > 760 {
				t.Fatal("tool disclosure lost compact reading geometry", activity)
			}
			for _, label := range []string{"Tool activity icon attempt/tool", "Tool disclosure icon attempt/tool"} {
				icon, ok := tester.Find(label)
				if !ok || icon.W != 16 || icon.H != 16 {
					t.Fatal("tool icon does not use canonical sixteen pixel geometry", label, icon)
				}
			}
			connectionClick(t, tester, "Tool activity attempt/tool")
			if !tester.HasText("Original result summary") || inspector.pages.Load() != 0 || inspector.reads.Load() != 0 {
				t.Fatal("disclosure lost original summary or read without the record action")
			}
			connectionClick(t, tester, "Inspect tool result attempt/tool")
			settleTrace(t, tester, p)
			step, selected := p.traceSelectedStep()
			want := "succeeded"
			if failed {
				want = "failed"
			}
			if !p.trace.active || p.problem != "" || p.trace.problem != "" || !selected || step.ID != "tool/run/tool" || step.Last != reference.Sequence || step.Status != want {
				t.Fatal("tool block did not switch to the trace and select its step", p.trace.active, p.problem, p.trace.problem, step)
			}
			if p.trace.target.BranchID != p.detail.Session.ActiveBranchID || p.trace.detail.Result != "exact retained original tool result" || inspector.pages.Load() == 0 || inspector.reads.Load() == 0 {
				t.Fatal("trace step detail did not read the tool's own records", p.trace.target, p.trace.detail, inspector.pages.Load(), inspector.reads.Load())
			}
			if !tester.HasText("第 ? 轮 · 窗口内第 3 步") && !tester.HasText("第 ? 轮") || !tester.HasText("工具") {
				t.Fatal("trace view did not render the selected step", tester.Texts())
			}
			if p.Service.Runtime.HasActive() || p.transcriptRows[1].Text != "Original result summary" {
				t.Fatal("inspection executed or replaced retained evidence")
			}
		})
	}
}

func TestWorkspaceTranscriptNoReferenceAndStaleBindings(t *testing.T) {
	p, inspector, _ := transcriptPageFixture(t, true, true)
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	connectionClick(t, tester, "Tool activity attempt/tool")
	if !tester.HasText("Original result summary") {
		t.Fatal("failure without a reference lost its inspectable summary")
	}
	oldBinding, oldTool := p.transcriptBinding(), p.transcriptTools["attempt/tool"]
	p.generation++
	p.inspectTranscriptTool(oldBinding, "attempt/tool", oldTool)
	tester.Frame()
	// A generation change (Send/Stop) keeps the block's disclosure, but an
	// action captured under the old generation starts no read.
	if p.trace.active || inspector.pages.Load() != 0 || inspector.reads.Load() != 0 || !tester.HasText("Original result summary") {
		t.Fatal("stale observation started a read or a generation change dropped the disclosure")
	}
	connectionClick(t, tester, "Inspect tool result attempt/tool")
	settleTrace(t, tester, p)
	if !p.trace.active || p.trace.window == nil || p.trace.selected != "" || inspector.pages.Load() == 0 || inspector.reads.Load() != 0 || p.transcriptRows[1].Status != view.Failed {
		t.Fatal("missing-reference fallback fabricated a selection or body, or hid failure", p.trace.active, p.trace.selected, inspector.pages.Load(), inspector.reads.Load())
	}
	pages := inspector.pages.Load()
	p.showConversation()
	entries := transcriptFixtureHistory(oldBinding.session, "native-transcript", nil, true)
	for _, change := range []func(){
		func() { p.generation++ },
		func() { p.selected = "other-session" },
		func() { p.detail.Session.ActiveBranchID = "other-branch" },
		func() { replaceTranscript(t, p) },
	} {
		p.selected, p.detail.Session.ActiveBranchID = oldBinding.session, oldBinding.branch
		replaceTranscript(t, p, entries...)
		binding := p.transcriptBinding()
		change()
		p.inspectTranscriptTool(binding, "attempt/tool", oldTool)
	}
	if p.trace.active || inspector.pages.Load() != pages || inspector.reads.Load() != 0 {
		t.Fatal("stale session, branch or removed row targeted a later view")
	}
}

func TestWorkspaceTranscriptUnsettledToolTracesOriginalOwnerAfterBranchSwitch(t *testing.T) {
	for _, source := range []string{"event", "start-reference"} {
		t.Run(source, func(t *testing.T) {
			p, inspector, reference := transcriptPageFixture(t, true, true)
			originalBranch := p.detail.Session.ActiveBranchID
			target := p.Service.DefaultTarget
			target.ModelID = "other-model"
			branch, err := p.Service.Store.Switch(t.Context(), p.selected, target, "next branch after cancelled tool")
			if err != nil {
				t.Fatal(err)
			}
			if err := p.Service.Store.BindNative(t.Context(), p.selected, branch.ID, "native-b"); err != nil {
				t.Fatal(err)
			}
			p.detail, err = p.Service.Store.Detail(t.Context(), p.selected)
			if err != nil {
				t.Fatal(err)
			}
			start := engine.Event{Kind: engine.ToolStart, ID: "tool", Name: "demo_echo"}
			if source == "event" {
				start.SessionID = reference.SessionID
			} else {
				start.Reference = &engine.Reference{SessionID: reference.SessionID, RecordID: "tool-start", Sequence: reference.Sequence}
			}
			replaceTranscript(t, p,
				hub.HistoryEntry{Sequence: 1, SessionID: p.selected, AttemptID: "attempt", Event: start},
				hub.HistoryEntry{Sequence: 2, SessionID: p.selected, AttemptID: "attempt", Event: engine.Event{Kind: engine.ToolEnd, ID: "tool", Text: "tool cancelled before settlement", IsError: true}},
			)
			p.applyDetail()
			tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
			connectionClick(t, tester, "Tool activity attempt/tool")
			connectionClick(t, tester, "Inspect tool result attempt/tool")
			settleTrace(t, tester, p)
			tool := p.transcriptTools["attempt/tool"]
			if p.detail.Session.ActiveBranchID != branch.ID || branch.ID == originalBranch || p.trace.target.BranchID != originalBranch || p.problem != "" || p.trace.problem != "" || p.trace.window == nil || p.trace.window.NativeID() != reference.SessionID {
				t.Fatal("unsettled tool trace followed the active branch instead of its native owner", p.problem, p.trace.problem, p.trace.target, p.detail.Session.ActiveBranchID)
			}
			if tool.hasRef || tool.reference != (engine.Reference{}) || p.trace.selected != "" || inspector.reads.Load() != 0 || p.Service.Runtime.HasActive() {
				t.Fatal("missing result reference fabricated a selection, body read or execution instead of the original branch tail", tool, p.trace.selected, inspector.reads.Load())
			}
		})
	}
}

func TestWorkspaceTranscriptUnknownToolOwnerNeverGuessesActiveBranch(t *testing.T) {
	p, inspector, _ := transcriptPageFixture(t, true, true)
	replaceTranscript(t, p, hub.HistoryEntry{Sequence: 1, SessionID: p.selected, AttemptID: "unknown-attempt", Event: engine.Event{Kind: engine.ToolEnd, ID: "tool", Name: "unavailable", Text: "failure without retained owner", IsError: true}})
	p.applyDetail()
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	connectionClick(t, tester, "Tool activity unknown-attempt/tool")
	connectionClick(t, tester, "Inspect tool result unknown-attempt/tool")
	tester.Frame()
	if p.trace.active || p.problem == "" || p.trace.target.BranchID != "" || inspector.pages.Load() != 0 || inspector.reads.Load() != 0 {
		t.Fatal("unknown tool owner guessed the currently active branch", p.trace.active, p.problem, p.trace.target, inspector.pages.Load(), inspector.reads.Load())
	}
}

func TestWorkspaceTranscriptLateTraceJumpIsDroppedAndReloaded(t *testing.T) {
	p, inspector, _ := transcriptPageFixture(t, false, false)
	inspector.entered = make(chan struct{}, 1)
	release := make(chan struct{})
	inspector.release = release
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	connectionClick(t, tester, "Tool activity attempt/tool")
	connectionClick(t, tester, "Inspect tool result attempt/tool")
	select {
	case <-inspector.entered:
	case <-time.After(time.Second):
		t.Fatal("explicit trace action did not start its metadata read")
	}
	p.generation++
	p.problem = "replacement problem"
	inspector.entered = nil
	close(release)
	// The stale jump result is dropped, never applied: the trace reloads its
	// tail under the new generation and selects nothing.
	settleTrace(t, tester, p)
	if inspector.reads.Load() != 0 || p.trace.window == nil || p.trace.generation != p.generation || p.trace.selected != "" || p.trace.loading || p.problem != "replacement problem" {
		t.Fatal("late trace jump read, selected or filled a later observation", inspector.reads.Load(), p.trace.window != nil, p.trace.selected, p.problem)
	}
}

func TestWorkspaceTranscriptMissingReferenceClearsPreviouslySelectedStep(t *testing.T) {
	p, inspector, _ := transcriptPageFixture(t, false, false)
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	connectionClick(t, tester, "Tool activity attempt/tool")
	connectionClick(t, tester, "Inspect tool result attempt/tool")
	settleTrace(t, tester, p)
	if p.trace.selected != "tool/run/tool" || p.trace.detail.Result != "exact retained original tool result" {
		t.Fatal("first tool did not select and read its actual step", p.trace.problem, p.trace.selected, p.trace.detail)
	}
	reads := inspector.reads.Load()
	connectionClick(t, tester, "Show conversation")
	appendTranscript(t, p, hub.HistoryEntry{Sequence: 4, SessionID: p.selected, AttemptID: "attempt", Event: engine.Event{Kind: engine.ToolEnd, SessionID: "native-transcript", ID: "without-ref", Name: "unavailable", Text: "Original failure without reference", IsError: true}})
	p.applyDetail()
	tester.Frame()
	connectionClick(t, tester, "Tool activity attempt/without-ref")
	connectionClick(t, tester, "Inspect tool result attempt/without-ref")
	settleTrace(t, tester, p)
	if !p.trace.active || p.trace.selected != "" || p.trace.detail.StepID != "" || inspector.reads.Load() != reads || tester.HasText("exact retained original tool result") {
		t.Fatal("missing-reference tool inherited the previous tool's selected step", p.trace.selected, p.trace.detail, inspector.reads.Load())
	}
}

func TestWorkspaceTranscriptLateDetailReadCannotRefillClearedSelection(t *testing.T) {
	p, inspector, _ := transcriptPageFixture(t, false, false)
	inspector.readEntered = make(chan struct{}, 1)
	release := make(chan struct{})
	inspector.readRelease = release
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	connectionClick(t, tester, "Tool activity attempt/tool")
	connectionClick(t, tester, "Inspect tool result attempt/tool")
	deadline := time.Now().Add(time.Second)
	entered := false
	for !entered && time.Now().Before(deadline) {
		tester.Frame()
		select {
		case <-inspector.readEntered:
			entered = true
		default:
			time.Sleep(time.Millisecond)
		}
	}
	if !entered {
		close(release)
		t.Fatal("explicit action never reached the owned detail read")
	}
	inspector.readEntered = nil
	p.clearTraceSelection()
	p.trace.problem = "replacement problem"
	close(release)
	settleTrace(t, tester, p)
	if p.trace.selected != "" || p.trace.detail.StepID != "" || p.trace.problem != "replacement problem" {
		t.Fatal("late detail read refilled a cleared or replaced step selection", p.trace.selected, p.trace.detail, p.trace.problem)
	}
}

func TestWorkspaceTranscriptCanonicalMessagesInBothAppearances(t *testing.T) {
	for _, dark := range []bool{false, true} {
		t.Run(map[bool]string{false: "light", true: "dark"}[dark], func(t *testing.T) {
			p, _, _ := transcriptPageFixture(t, false, false)
			preferences := transcriptPreferences(t, p)
			size := 16
			if _, err := preferences.Update(t.Context(), hub.PreferenceUpdate{ExpectedRevision: preferences.Snapshot().Revision, FontSize: &size}); err != nil {
				t.Fatal(err)
			}
			tester := ui.NewTester(workspaceStaticView(p), 1440, 960)
			tester.SetDark(dark)
			bubble, bubbleOK := tester.Find("User message turn/input")
			turn, turnOK := tester.Find("User turn turn/input")
			user, userOK := tester.Find("User paragraph")
			assistant, assistantOK := tester.Find("Assistant paragraph")
			avatar, avatarOK := tester.Find("Message avatar attempt/reply")
			tool, toolOK := tester.Find("Tool activity attempt/tool")
			if !bubbleOK || !turnOK || !userOK || !assistantOK || !avatarOK || !toolOK || bubble.W != 560 || turn.W != 760 || math.Abs(float64(bubble.X+bubble.W-turn.X-turn.W)) > .01 ||
				math.Abs(float64(user.X-bubble.X-12)) > .01 || math.Abs(float64(user.Y-bubble.Y-12)) > .01 || math.Abs(float64(bubble.H-user.H-24)) > .01 ||
				avatar.W != 32 || avatar.H != 32 || math.Abs(float64(assistant.X-avatar.X-44)) > .01 || math.Abs(float64(tool.X-assistant.X)) > .01 {
				t.Fatal("right-aligned user bubble or avatar/reply/tool columns differ from formal22", turn, bubble, user, avatar, assistant, tool)
			}
			header, headerOK := tester.Find("Workspace header")
			transcript, transcriptOK := tester.Find("Conversation transcript")
			history, historyOK := tester.Find("Message history")
			if !headerOK || !transcriptOK || !historyOK || transcript.Y != header.Y+header.H+workspaceRegionGap || history.Y != transcript.Y+16 || turn.Y != history.Y ||
				math.Abs(float64(tool.Y-turn.Y-turn.H-24)) > .01 || math.Abs(float64(avatar.Y-tool.Y-tool.H-24)) > .01 {
				t.Fatal("conversation lost formal22 top16 and row24 spacing below the unchanged header", header, transcript, history, turn, tool, avatar)
			}
			want := ui.Hex("#F5F6F8") // Formal22 UserMessage binds ui.color.subtle.
			if dark {
				want = ui.Hex("#252932")
			}
			got := tester.Image().RGBAAt(int(bubble.X+20), int(bubble.Y+3))
			if got.R != want.R || got.G != want.G || got.B != want.B {
				t.Fatal("user bubble does not use canonical subtle", got, want)
			}
			if math.Abs(float64(user.H-assistant.H)) > .01 {
				t.Fatal("plain user text and ordinary assistant Markdown have different body typography", user, assistant)
			}
			reference := ui.NewTester(func(c *ui.Context) {
				p.applyPreferences(c)
				ui.Text(c, "Body reference").FontSize(16).FontWeight(400).LineHeight(1.75)
			}, 760, 100)
			bodyReference, ok := reference.Find("Body reference")
			if !ok || math.Abs(float64(user.H-bodyReference.H)) > .01 {
				t.Fatal("message body differs from native sixteen point, 1.75 line-height text", user, bodyReference)
			}
			messageHeader, ok := tester.Find("Message header attempt/reply")
			if !ok || messageHeader.H != 32 || messageHeader.Y != avatar.Y || messageHeader.X != assistant.X || math.Abs(float64(assistant.Y-messageHeader.Y-messageHeader.H-12)) > .01 {
				t.Fatal("assistant header or body gap differs from the canonical layout", messageHeader, assistant)
			}
		})
	}
}

func TestWorkspaceTranscriptSavedEngineIdentityNeverUsesComposerTarget(t *testing.T) {
	for _, target := range []hub.Target{
		{EngineID: "codex", ModelID: "engine-default", ProviderID: "engine-default", AccountSource: "fixture", BillingRoute: "fixture"},
		{EngineID: "pi", ModelID: "saved-pi-model", ProviderID: "saved-provider", AccountSource: "fixture", BillingRoute: "fixture"},
		{EngineID: "alva", ModelID: "saved-alva-model", ProviderID: "saved-provider", AccountSource: "fixture", BillingRoute: "fixture"},
		{EngineID: strings.Repeat("engine", 25), ModelID: strings.Repeat("saved-model", 20), ProviderID: "saved-provider", AccountSource: "fixture", BillingRoute: "fixture"},
	} {
		t.Run(target.EngineID, func(t *testing.T) {
			p, inspector, _ := transcriptPageFixture(t, false, false)
			branch, err := p.Service.Store.Switch(t.Context(), p.selected, target, "saved reply fixture")
			if err != nil {
				t.Fatal(err)
			}
			if err := p.Service.Store.BindNative(t.Context(), p.selected, branch.ID, "saved-native"); err != nil {
				t.Fatal(err)
			}
			if _, err := p.Service.Store.Switch(t.Context(), p.selected, p.Service.DefaultTarget, "next-turn fixture branch"); err != nil {
				t.Fatal(err)
			}
			p.detail, err = p.Service.Store.Detail(t.Context(), p.selected)
			if err != nil {
				t.Fatal(err)
			}
			p.composer.target = hub.Target{EngineID: "claude", ModelID: "unrelated-next-model"}
			replaceTranscript(t, p,
				hub.HistoryEntry{Sequence: 1, SessionID: p.selected, AttemptID: "saved-attempt", Event: engine.Event{Kind: engine.SessionStarted, SessionID: "saved-native"}},
				hub.HistoryEntry{Sequence: 2, SessionID: p.selected, AttemptID: "saved-attempt", Event: engine.Event{Kind: engine.Message, ID: "reply", Text: "Actual saved response"}},
			)
			p.applyDetail()
			tester := ui.NewTester(workspaceStaticView(p), 1440, 960)
			author, model := p.transcriptAuthor(p.transcriptRows[0])
			wantAuthor := map[string]string{"codex": "Codex", "pi": "Pi", "alva": "Alva"}[target.EngineID]
			if wantAuthor == "" {
				wantAuthor = target.EngineID
			}
			if author != wantAuthor || model != strings.ReplaceAll(target.ModelID, "engine-default", "") || tester.HasText("unrelated-next-model") || tester.HasText("engine-default") {
				t.Fatal("reply identity came from next-turn target or a fabricated CLI model", author, model, tester.Texts())
			}
			if _, ok := tester.Find("Message model saved-attempt/reply"); ok != (target.ModelID != "engine-default") {
				t.Fatal("actual model availability does not control its identity label")
			}
			header, _ := tester.Find("Message header saved-attempt/reply")
			authorBox, _ := tester.Find("Message author saved-attempt/reply")
			actions, _ := tester.Find("Message actions saved-attempt/reply")
			if authorBox.X < header.X || authorBox.X+authorBox.W > actions.X || actions.W != 28 || actions.H != 28 || actions.X+actions.W > header.X+header.W+.01 {
				t.Fatal("long saved identity displaced the content action or left its header", header, authorBox, actions)
			}
			replaceTranscript(t, p, hub.HistoryEntry{Sequence: 3, SessionID: p.selected, AttemptID: "unknown-attempt", Event: engine.Event{Kind: engine.Message, ID: "reply", Text: "No retained origin"}})
			p.applyDetail()
			tester.Frame()
			if _, ok := tester.Find("Message model unknown-attempt/reply"); ok || inspector.pages.Load() != 0 || inspector.reads.Load() != 0 || p.Service.Runtime.HasActive() {
				t.Fatal("missing native identity fabricated a model or started an evidence read")
			}
		})
	}
}

func TestWorkspaceTranscriptStreamingCopyAndErrorStayInReplyColumn(t *testing.T) {
	p, inspector, _ := transcriptPageFixture(t, false, false)
	replaceTranscript(t, p,
		hub.HistoryEntry{Sequence: 1, SessionID: p.selected, TurnID: "turn", Input: strings.Repeat("长消息 mixed text ", 90)},
		hub.HistoryEntry{Sequence: 2, SessionID: p.selected, AttemptID: "attempt", Event: engine.Event{Kind: engine.SessionStarted, SessionID: "native-transcript"}},
		hub.HistoryEntry{Sequence: 3, SessionID: p.selected, AttemptID: "attempt", Event: engine.Event{Kind: engine.MessageDelta, ID: "reply", Text: "Streaming paragraph"}},
	)
	p.applyDetail()
	tester := ui.NewTester(workspaceStaticView(p), 960, 1200)
	avatar, avatarOK := tester.Find("Message avatar attempt/reply")
	if !avatarOK {
		t.Fatal("streaming reply has no identity column")
	}
	appendTranscript(t, p, hub.HistoryEntry{Sequence: 4, SessionID: p.selected, AttemptID: "attempt", Event: engine.Event{Kind: engine.MessageDelta, ID: "reply", Text: " continued"}})
	p.applyDetail()
	tester.Frame()
	updated, ok := tester.Find("Message avatar attempt/reply")
	if !ok || updated.X != avatar.X {
		t.Fatal("stream append replaced its keyed reply column", avatar, updated)
	}
	connectionClick(t, tester, "Copy message attempt/reply")
	if tester.Clipboard() != "Streaming paragraph continued" {
		t.Fatal("copy did not use the current retained reply content", tester.Clipboard())
	}
	connectionClick(t, tester, "Message actions attempt/reply")
	if err := tester.ChooseMenuItem("复制消息"); err != nil {
		t.Fatal(err)
	}
	appendTranscript(t, p, hub.HistoryEntry{Sequence: 5, SessionID: p.selected, AttemptID: "attempt", Event: engine.Event{Kind: engine.Error, Text: "Actual engine error"}})
	p.applyDetail()
	tester.Frame()
	errorBody, found := tester.Find("Actual engine error")
	if !found || errorBody.X != updated.X+44 || inspector.pages.Load() != 0 || inspector.reads.Load() != 0 || p.Service.Runtime.HasActive() {
		t.Fatal("error lost reply-column geometry or presentation performed I/O", errorBody, updated)
	}
}

func TestWorkspaceTranscriptRendererRetainsSharedMarkdownOptions(t *testing.T) {
	p, _, _ := transcriptPageFixture(t, false, false)
	theme := &markdown.Theme{FontSize: 29, LineHeight: 1.2}
	calls := 0
	options := &markdown.Options{Theme: theme, Code: map[string]func(*ui.Context, *markdown.CodeBlockView){"fixture": func(c *ui.Context, block *markdown.CodeBlockView) {
		calls++
		ui.Text(c, "Registered module result")
	}}}
	render := newWorkspaceTranscriptRenderer(p, options)
	tester := ui.NewTester(func(c *ui.Context) {
		ui.Column(c).Fill().Children(func() {
			render(c, view.Message{ID: "module", Role: "Alva", Text: "```fixture\nactual\n```", Status: view.Completed})
		})
	}, 760, 400)
	if !tester.HasText("Registered module result") || calls == 0 || options.Theme != theme || theme.FontSize != 29 || theme.LineHeight != 1.2 {
		t.Fatal("Hub presentation replaced shared Markdown modules or mutated caller options")
	}
}

func TestWorkspaceTranscriptBodyFollowsSavedGeneralSizeAndSystemScale(t *testing.T) {
	p, inspector, _ := transcriptPageFixture(t, false, false)
	preferences := transcriptPreferences(t, p)
	tester := ui.NewTester(workspaceStaticView(p), 1440, 960)
	assertBodySize := func(size float32) {
		t.Helper()
		for _, label := range []string{"User paragraph", "Assistant paragraph"} {
			body, ok := tester.Find(label)
			if !ok || math.Abs(float64(body.H-size*1.75)) > .01 {
				t.Fatal("actual transcript did not consume saved body size and system scale", size, label, body)
			}
		}
	}
	assertBodySize(14)
	openPreferencePage(t, p, tester)
	choosePreference(t, tester, "Body size", "16")
	connectionClick(t, tester, "Save preferences")
	settlePreferences(t, p, tester)
	if preferences.Snapshot().FontSize != 16 {
		t.Fatal("General did not save the selected body size")
	}
	connectionClick(t, tester, "Back to workspace")
	assertBodySize(16)
	tester.SetPreferences(ui.Preferences{TextScale: 1.5})
	assertBodySize(24)
	if inspector.pages.Load() != 0 || inspector.reads.Load() != 0 || p.Service.Runtime.HasActive() {
		t.Fatal("body preference presentation read evidence or started execution")
	}
}
