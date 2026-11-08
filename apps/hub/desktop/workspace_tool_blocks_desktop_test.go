// INPUT: Actual private native records and artifacts, product TranscriptWindow/TraceService/WorkspaceFileTab, a cold Inspector adapter and headless MyGo controls.
// OUTPUT: Per-type collapsed rows, expanded bodies (tail, diff, results, reference read), file/child/trace jumps, late-read rejection, superseded todos, kept expansion while scrolling and an opt-in tool-row frame-time gate.
// POS: [L7 Hub UI tests] Exercises typed tool blocks over retained evidence without launching a model, process or OS window.
//go:build desktop

package hubdesktop

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
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

type toolBlocksFixture struct {
	p         *NativePage
	session   hub.Session
	native    string
	log       storage.Session
	inspector *transcriptInspector
	workspace string
	sequence  uint64
}

func newToolBlocksFixture(t *testing.T) *toolBlocksFixture {
	t.Helper()
	s, _ := fixture(t)
	port, ok := s.Store.(hub.WorkspaceViewStore)
	if !ok {
		t.Fatal("fixture workspace view capability missing")
	}
	var err error
	if s.WorkspaceViews, err = product.NewWorkspaceViewsService(port); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = s.WorkspaceViews.Close(context.Background()) })
	workspace, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	if err = os.MkdirAll(filepath.Join(workspace, "src"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(workspace, "src", "main.go"), []byte("package main\n\nfunc main() {}\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	session, err := s.Create(t.Context(), "Tools", workspace)
	if err != nil {
		t.Fatal(err)
	}
	native := "native-tools"
	log, err := s.Repository.Create(t.Context(), session.ID, native, "")
	if err != nil {
		t.Fatal(err)
	}
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
	if p.detail, err = s.Store.Detail(t.Context(), session.ID); err != nil {
		t.Fatal(err)
	}
	return &toolBlocksFixture{p: p, session: session, native: native, log: log, inspector: inspector, workspace: workspace}
}

func (f *toolBlocksFixture) records(t *testing.T, records ...abi.SessionRecord) []abi.SessionRecord {
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

// call commits a native intent for a tool call and returns its reference.
func (f *toolBlocksFixture) intent(t *testing.T, run, id, name, arguments string) *engine.Reference {
	t.Helper()
	record := f.records(t, abi.SessionRecord{Kind: abi.RecordToolIntent, ToolIntent: &abi.ToolIntent{RunID: run, Call: wire.ToolCall{ID: id, Name: name, Arguments: json.RawMessage(arguments)}}})[0]
	return &engine.Reference{SessionID: f.native, RecordID: record.ID, Sequence: record.Sequence}
}

func (f *toolBlocksFixture) outcome(t *testing.T, run, id, text string, failed bool) *engine.Reference {
	t.Helper()
	status := abi.OutcomeSucceeded
	if failed {
		status = abi.OutcomeFailed
	}
	record := f.records(t, abi.SessionRecord{Kind: abi.RecordToolOutcome, ToolOutcome: &abi.ToolOutcome{RunID: run, CallID: id, Status: status, Result: wire.ToolResult{CallID: id, IsError: failed, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: text}}}}})[0]
	return &engine.Reference{SessionID: f.native, RecordID: record.ID, Sequence: record.Sequence}
}

// show bounds a display the way an adapter does before emitting it.
func show(display engine.ToolDisplay) *engine.ToolDisplay { return engine.BoundToolDisplay(display) }

func n(value int) *int { return &value }

// entries builds product history for one call as an adapter emits it: start
// and end displays and the result text, owned by the fixture's native session.
func (f *toolBlocksFixture) entries(turn, id, name string, started, ended *engine.ToolDisplay, text string, failed bool, start, end *engine.Reference) []hub.HistoryEntry {
	f.sequence += 2
	if ended == nil {
		ended = started
	}
	return []hub.HistoryEntry{
		{Sequence: f.sequence - 1, SessionID: f.session.ID, TurnID: turn, AttemptID: "attempt-" + turn, Event: engine.Event{Kind: engine.ToolStart, SessionID: f.native, ID: id, Name: name, Reference: start, Display: started}},
		{Sequence: f.sequence, SessionID: f.session.ID, TurnID: turn, AttemptID: "attempt-" + turn, Event: engine.Event{Kind: engine.ToolEnd, SessionID: f.native, ID: id, Name: name, Text: text, IsError: failed, Reference: end, Display: ended}},
	}
}

func waitToolBlocks(t *testing.T, tester *ui.Tester, p *NativePage, ready func() bool) {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		tester.Frame()
		if ready() {
			tester.Frame()
			return
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("tool block did not settle", p.problem, p.trace.problem)
}

func TestWorkspaceToolBlocksSummariesAndExpandedBodies(t *testing.T) {
	f := newToolBlocksFixture(t)
	var history []hub.HistoryEntry
	history = append(history, f.entries("t1", "cmd", "shell-a", show(engine.ToolDisplay{Kind: engine.ToolCommand, Command: "go test ./...\nsecond line"}), show(engine.ToolDisplay{Kind: engine.ToolCommand, Command: "go test ./...\nsecond line", ExitCode: n(2)}), `{"job_id":"j","status":{"exit_code":2},"output":"see frames"}`, true, nil, nil)...)
	history = append(history, f.entries("t1", "codex", "shell-b", show(engine.ToolDisplay{Kind: engine.ToolCommand, Command: "ls"}), show(engine.ToolDisplay{Kind: engine.ToolCommand, Command: "ls", ExitCode: n(0)}), "first\nmiddle\nLAST LINE\n", false, nil, nil)...)
	history = append(history, f.entries("t1", "edit", "editor", show(engine.ToolDisplay{Kind: engine.ToolEdit, Path: "src/main.go", Files: 1, Added: n(1), Removed: n(1), Diff: &engine.ToolDiff{Old: "func main() {}", New: "func main() { run() }", Partial: true}}), nil, `{"written_bytes":30}`, false, nil, nil)...)
	history = append(history, f.entries("t1", "find", "finder", show(engine.ToolDisplay{Kind: engine.ToolSearch, Query: "TODO", Path: "."}), show(engine.ToolDisplay{Kind: engine.ToolSearch, Query: "TODO", Path: ".", Matches: n(2), MoreMatches: true, Results: []engine.ToolMatch{{Path: "src/main.go", Text: "// TODO wire"}, {Path: "b.go", Text: "TODO two"}}}), `{"matches":[]}`, false, nil, nil)...)
	history = append(history, f.entries("t1", "echo", "demo_echo", show(engine.ToolDisplay{Kind: engine.ToolOther}), nil, "Echo result body", false, nil, nil)...)
	replaceTranscript(t, f.p, history...)
	f.p.applyDetail()
	tester := ui.NewTester(workspaceStaticView(f.p), 1440, 960)
	for _, text := range []string{"go test ./... … · 退出码 2", "ls · 退出码 0", "修改 src/main.go · +1 −1", "搜索 TODO · 2 处以上", "工具 · demo_echo · 已完成"} {
		if !tester.HasText(text) {
			t.Fatal("collapsed tool row text missing", text, tester.Texts())
		}
	}
	if tester.HasText("Echo result body") || tester.HasText("LAST LINE") || f.inspector.pages.Load() != 0 || f.inspector.reads.Load() != 0 {
		t.Fatal("collapsed tool rows showed bodies or read evidence")
	}
	if icon, ok := tester.Find("Tool activity icon attempt-t1/cmd"); !ok || icon.W != 16 {
		t.Fatal("failed command lost its icon", icon)
	}
	for _, id := range []string{"cmd", "codex", "edit", "find", "echo"} {
		connectionClick(t, tester, "Tool activity attempt-t1/"+id)
	}
	tester.Frame()
	if _, ok := tester.Find("Tool output attempt-t1/codex"); !ok || !tester.HasText("first\nmiddle\nLAST LINE") {
		t.Fatal("command output tail is not shown in its monospace body", tester.Texts())
	}
	if diff, ok := tester.Find("Tool diff attempt-t1/edit"); !ok || diff.H < 20 {
		t.Fatal("edit block did not render its diff viewer", diff)
	}
	if _, ok := tester.Find("Open search result attempt-t1/find 1"); !ok || !tester.HasText("b.go · TODO two") {
		t.Fatal("search results are not a clickable list", tester.Texts())
	}
	if !tester.HasText("Echo result body") {
		t.Fatal("other tool lost its result text")
	}
	for _, id := range []string{"edit"} {
		if _, ok := tester.Find("Open tool file attempt-t1/" + id); !ok {
			t.Fatal("file jump missing", id)
		}
	}
	for _, id := range []string{"cmd", "codex", "find", "echo"} {
		if _, ok := tester.Find("Open tool file attempt-t1/" + id); ok {
			t.Fatal("block without a file offered a file jump", id)
		}
		if _, ok := tester.Find("Inspect tool result attempt-t1/" + id); !ok {
			t.Fatal("trace jump missing", id)
		}
	}
	if f.inspector.reads.Load() != 0 || f.p.Service.Runtime.HasActive() {
		t.Fatal("text bodies read native evidence or executed")
	}
	// Opening a search result pins its file in the resource panel.
	connectionClick(t, tester, "Open search result attempt-t1/find 0")
	waitToolBlocks(t, tester, f.p, func() bool {
		tab := f.p.workspaceActiveResource()
		return tab != nil && tab.Kind == "file" && tab.File != nil && tab.File.Path == "src/main.go"
	})
}

func TestWorkspaceToolBlocksReadBodyAndFileJump(t *testing.T) {
	f := newToolBlocksFixture(t)
	backend, err := f.p.Service.Repository.BoundReferences(t.Context(), f.session.ID, f.native)
	if err != nil {
		t.Fatal(err)
	}
	snippet := "package main\n\n" + strings.Repeat("// retained file line\n", 600)
	reference, err := backend.StoreText(t.Context(), snippet)
	if err != nil {
		t.Fatal(err)
	}
	locator, _ := json.Marshal(map[string]wire.Reference{"reference": reference})
	start := f.intent(t, "run", "read", "read_file", `{"path":"src/main.go"}`)
	end := f.outcome(t, "run", "read", string(locator), false)
	history := f.entries("t1", "read", "reader", show(engine.ToolDisplay{Kind: engine.ToolRead, Path: "src/main.go"}), show(engine.ToolDisplay{Kind: engine.ToolRead, Path: "src/main.go", Output: reference.URI}), string(locator), false, start, end)
	history = append(history, f.entries("t1", "gone", "reader", show(engine.ToolDisplay{Kind: engine.ToolRead, Path: "src/missing.go"}), nil, "File is unavailable", true, nil, nil)...)
	replaceTranscript(t, f.p, history...)
	f.p.applyDetail()
	tester := ui.NewTester(workspaceStaticView(f.p), 1440, 960)
	if !tester.HasText("读取 src/main.go") || f.inspector.reads.Load() != 0 {
		t.Fatal("read row is not its path or read before expansion", tester.Texts())
	}
	connectionClick(t, tester, "Tool activity attempt-t1/read")
	waitToolBlocks(t, tester, f.p, func() bool {
		body := f.p.toolBlocks.bodies["attempt-t1/read"]
		return body != nil && !body.loading
	})
	body := f.p.toolBlocks.bodies["attempt-t1/read"]
	if body.problem != "" || !strings.HasPrefix(snippet, body.text) || len(body.text) > toolBodyPages*2048 || !body.more || !tester.HasText("这里只显示开头部分；完整内容可在轨迹中查看。") {
		t.Fatal("read body is not a bounded prefix of the retained file", body.problem, len(body.text), body.more)
	}
	if _, ok := tester.Find("Tool output attempt-t1/read"); !ok {
		t.Fatal("read snippet is not shown")
	}
	reads := f.inspector.reads.Load()
	connectionClick(t, tester, "Tool activity attempt-t1/read")
	connectionClick(t, tester, "Tool activity attempt-t1/read")
	tester.Frame()
	if f.inspector.reads.Load() != reads {
		t.Fatal("re-expanding read the same body again")
	}
	connectionClick(t, tester, "Open tool file attempt-t1/read")
	waitToolBlocks(t, tester, f.p, func() bool {
		tab := f.p.workspaceActiveResource()
		return tab != nil && tab.Kind == "file" && tab.File.Path == "src/main.go" && f.p.resources.page.Body.Title == "src/main.go"
	})
	if string(f.p.resources.page.Body.Data) != "package main\n\nfunc main() {}\n" {
		t.Fatal("resource panel did not read the tool's file", string(f.p.resources.page.Body.Data))
	}
	// A path the workspace no longer has opens the project file search instead.
	connectionClick(t, tester, "Tool activity attempt-t1/gone")
	if !tester.HasText("File is unavailable") {
		t.Fatal("failed read lost its error text")
	}
	connectionClick(t, tester, "Open tool file attempt-t1/gone")
	waitToolBlocks(t, tester, f.p, func() bool {
		tab := f.p.workspaceActiveResource()
		return tab != nil && tab.Kind == "files" && tab.Query == "missing.go"
	})
}

func TestWorkspaceToolBlocksLateBodyReadIsDropped(t *testing.T) {
	f := newToolBlocksFixture(t)
	backend, err := f.p.Service.Repository.BoundReferences(t.Context(), f.session.ID, f.native)
	if err != nil {
		t.Fatal(err)
	}
	reference, err := backend.StoreText(t.Context(), "late body text")
	if err != nil {
		t.Fatal(err)
	}
	locator, _ := json.Marshal(map[string]wire.Reference{"reference": reference})
	history := f.entries("t1", "read", "reader", show(engine.ToolDisplay{Kind: engine.ToolRead, Path: "src/main.go"}), show(engine.ToolDisplay{Kind: engine.ToolRead, Path: "src/main.go", Output: reference.URI}), string(locator), false, nil, nil)
	replaceTranscript(t, f.p, history...)
	f.p.applyDetail()
	f.inspector.readEntered = make(chan struct{}, 1)
	release := make(chan struct{})
	f.inspector.readRelease = release
	tester := ui.NewTester(workspaceStaticView(f.p), 1240, 850)
	connectionClick(t, tester, "Tool activity attempt-t1/read")
	select {
	case <-f.inspector.readEntered:
	case <-time.After(time.Second):
		t.Fatal("expanding did not start the bounded body read")
	}
	if !tester.HasText("正在读取…") {
		tester.Frame()
	}
	f.p.detail.Session.ActiveBranchID = "other-branch"
	f.inspector.readEntered = nil
	close(release)
	deadline := time.Now().Add(time.Second)
	for time.Now().Before(deadline) && len(f.p.updates) == 0 {
		time.Sleep(time.Millisecond)
	}
	tester.Frame()
	tester.Frame()
	if f.p.toolBlocks.expanded["attempt-t1/read"] || f.p.toolBlocks.bodies["attempt-t1/read"] != nil || tester.HasText("late body text") {
		t.Fatal("a body read from an older observation landed in the new one")
	}
}

func TestWorkspaceToolBlocksAgentCardEntersChildAndReturns(t *testing.T) {
	f := newToolBlocksFixture(t)
	user := wire.TextMessage(wire.RoleUser, "delegate")
	f.records(t, abi.SessionRecord{Kind: abi.RecordConversation, Message: &user})
	start := f.intent(t, "run", "spawn", "agent", `{"task":"Review the open diff"}`)
	child, err := f.p.Service.Repository.CreateChild(t.Context(), f.session.ID, f.native, "native-child", "run", "spawn", "scope")
	if err != nil {
		t.Fatal(err)
	}
	childUser := wire.TextMessage(wire.RoleUser, "Review the open diff")
	if _, err = child.Session.AppendRecords(t.Context(), []abi.SessionRecord{abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: &childUser})}); err != nil {
		t.Fatal(err)
	}
	result := `{"child_session_id":"native-child","outcome":"completed","is_error":false}`
	end := f.outcome(t, "run", "spawn", result, false)
	history := f.entries("t1", "spawn", "delegate", show(engine.ToolDisplay{Kind: engine.ToolSubagent, Task: "Review the open diff"}), show(engine.ToolDisplay{Kind: engine.ToolSubagent, Task: "Review the open diff", ChildSessionID: "native-child", Status: "completed"}), result, false, start, end)
	history = append(history, f.entries("t1", "fail", "delegate", show(engine.ToolDisplay{Kind: engine.ToolSubagent, Task: "Second child"}), show(engine.ToolDisplay{Kind: engine.ToolSubagent, Task: "Second child", Status: "failed"}), `{"outcome":"failed"}`, true, nil, nil)...)
	replaceTranscript(t, f.p, history...)
	f.p.applyDetail()
	tester := ui.NewTester(workspaceStaticView(f.p), 1440, 960)
	if _, ok := tester.Find("Agent card attempt-t1/spawn"); !ok || !tester.HasText("Review the open diff") || !tester.HasText("完成") || !tester.HasText("失败") {
		t.Fatal("sub-Agent card lacks task or status", tester.Texts())
	}
	connectionClick(t, tester, "Tool activity attempt-t1/spawn")
	if !tester.HasText(result) {
		t.Fatal("agent card without a summary reference lost its result text")
	}
	connectionClick(t, tester, "Enter agent session attempt-t1/spawn")
	settleTrace(t, tester, f.p)
	if !f.p.trace.active || len(f.p.trace.frames) != 1 || len(f.p.trace.target.Path) != 1 || f.p.trace.target.Path[0].LinkSequence != child.Link.Sequence || f.p.trace.window == nil || f.p.trace.window.NativeID() != "native-child" || f.p.trace.problem != "" {
		t.Fatal("agent card did not enter its child session record", f.p.trace.active, f.p.trace.target, f.p.trace.problem)
	}
	connectionClick(t, tester, "Return to parent trace")
	settleTrace(t, tester, f.p)
	step, selected := f.p.traceSelectedStep()
	if len(f.p.trace.target.Path) != 0 || f.p.trace.window.NativeID() != f.native || !selected || step.CallID != "spawn" {
		t.Fatal("returning did not restore the parent trace at the agent call", f.p.trace.target, step)
	}
	// A failed call without a child cannot be entered.
	connectionClick(t, tester, "Show conversation")
	if _, ok := tester.Find("Enter agent session attempt-t1/fail"); !ok {
		t.Fatal("failed agent card lost its actions")
	}
	_ = tester.Click("Enter agent session attempt-t1/fail")
	tester.Frame()
	if f.p.trace.active {
		t.Fatal("agent call without a child session was entered")
	}
	if f.p.Service.Runtime.HasActive() {
		t.Fatal("observation executed")
	}
}

func TestWorkspaceToolBlocksSupersededTodoCollapses(t *testing.T) {
	f := newToolBlocksFixture(t)
	var history []hub.HistoryEntry
	history = append(history, f.entries("t1", "plan-1", "planner", show(engine.ToolDisplay{Kind: engine.ToolTodo, Todos: engine.Checklist("- [ ] OLD FIRST ITEM\n- [ ] second")}), nil, `{"saved":true}`, false, nil, nil)...)
	history = append(history, f.entries("t1", "plan-2", "planner", show(engine.ToolDisplay{Kind: engine.ToolTodo, Todos: engine.Checklist("- [x] NEW FIRST ITEM\n- [~] second\n- [ ] third")}), nil, `{"saved":true}`, false, nil, nil)...)
	replaceTranscript(t, f.p, history...)
	f.p.applyDetail()
	tester := ui.NewTester(workspaceStaticView(f.p), 1440, 960)
	if !tester.HasText("待办已更新") || tester.HasText("OLD FIRST ITEM") || !tester.HasText("NEW FIRST ITEM") || !tester.HasText("third") {
		t.Fatal("earlier todo of the Turn is not one collapsed line, or the latest is incomplete", tester.Texts())
	}
	for i, status := range []string{"completed", "in_progress", "pending"} {
		if _, ok := tester.Find(fmt.Sprintf("Todo status attempt-t1/plan-2 %d %s", i, status)); !ok {
			t.Fatal("todo item status missing", i, status)
		}
	}
	connectionClick(t, tester, "Tool activity attempt-t1/plan-1")
	if !tester.HasText("OLD FIRST ITEM") {
		t.Fatal("superseded todo did not open")
	}
	connectionClick(t, tester, "Tool activity attempt-t1/plan-1")
	if tester.HasText("OLD FIRST ITEM") {
		t.Fatal("superseded todo did not collapse again")
	}
}

func TestWorkspaceToolBlocksKeepExpansionWhileScrolling(t *testing.T) {
	service, _ := fixture(t)
	session, err := service.Create(t.Context(), "Scrolling tools", "")
	if err != nil {
		t.Fatal(err)
	}
	for turn := 0; turn < 120; turn++ {
		claim, err := service.Store.BeginTurn(t.Context(), session.ID, fmt.Sprintf("Question %d", turn))
		if err != nil {
			t.Fatal(err)
		}
		for _, event := range []engine.Event{
			{Kind: engine.SessionStarted, SessionID: "native-scroll"},
			{Kind: engine.ToolStart, SessionID: "native-scroll", ID: "cmd", Name: "shell", Display: show(engine.ToolDisplay{Kind: engine.ToolCommand, Command: fmt.Sprintf("echo %d", turn)})},
			{Kind: engine.ToolEnd, SessionID: "native-scroll", ID: "cmd", Name: "shell", Text: fmt.Sprintf("output of turn %d\n", turn), Display: show(engine.ToolDisplay{Kind: engine.ToolCommand, Command: fmt.Sprintf("echo %d", turn), ExitCode: n(0)})},
			{Kind: engine.Message, SessionID: "native-scroll", ID: "assistant-0", Text: fmt.Sprintf("Answer %d. ", turn) + strings.Repeat("A committed paragraph keeps each row tall enough to scroll. ", 3)},
			{Kind: engine.Completed, SessionID: "native-scroll", Outcome: engine.Succeeded},
		} {
			if err := service.Store.Record(t.Context(), claim.Attempt.ID, event, false); err != nil {
				t.Fatal(err)
			}
		}
		if err := service.Store.FinishTurn(t.Context(), claim.Turn.ID, engine.Succeeded); err != nil {
			t.Fatal(err)
		}
	}
	p := NewNativePage(service)
	p.ctx = t.Context()
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	p.selectSession(session.ID)
	waitTranscript(t, tester, func() bool { return !p.polling && !p.chat.Busy && len(p.model.Messages) > 0 })
	last := ""
	for i := len(p.model.Messages) - 1; i >= 0 && last == ""; i-- {
		if _, tool := p.transcriptTools[p.model.Messages[i].ID]; tool {
			last = p.model.Messages[i].ID
		}
	}
	if !tester.HasText("echo 119") {
		t.Fatal("latest command row is not visible", tester.Texts())
	}
	connectionClick(t, tester, "Tool activity "+last)
	if !tester.HasText("output of turn 119") {
		t.Fatal("expanded command output missing")
	}
	loaded := len(p.model.Messages)
	anchor := ""
	wheelTranscript(t, tester, -240, func() bool {
		first, lastRow := p.chat.VisibleRows()
		if lastRow >= first && first < 5 {
			anchor = p.model.Messages[first].ID
			return true
		}
		return false
	})
	waitTranscript(t, tester, func() bool { return !p.polling && len(p.model.Messages) > loaded })
	tester.Frame()
	if first, lastRow := p.chat.VisibleRows(); lastRow < first || p.model.Messages[first].ID != anchor {
		t.Fatal("loading earlier history with an expanded block moved the first visible row", anchor)
	}
	visible := func(label string) bool {
		list, listed := tester.Find("Message history")
		box, shown := tester.Find(label)
		return listed && shown && box.H > 0 && box.Y < list.Y+list.H && box.Y+box.H > list.Y
	}
	if visible("Tool output " + last) {
		t.Fatal("scrolled-away row still shown; the test did not scroll it out")
	}
	if !p.toolBlocks.expanded[last] {
		t.Fatal("loading earlier history dropped the expansion state")
	}
	wheelTranscript(t, tester, 240, func() bool { return p.chat.AtEnd() })
	tester.Frame()
	if !visible("Tool output "+last) || !tester.HasText("output of turn 119") {
		t.Fatal("expansion was not kept after scrolling back", tester.Texts())
	}
}

// ALVA_UI_BENCH=1 measures headless build/layout/paint CPU while scrolling a
// window of typed tool rows (every tenth expanded, diffs included).
func TestWorkspaceTranscriptToolScrollFrameBudget(t *testing.T) {
	if os.Getenv("ALVA_UI_BENCH") != "1" {
		t.Skip("set ALVA_UI_BENCH=1 to run the frame-time gate")
	}
	service, _ := fixture(t)
	p := NewNativePage(service)
	p.ctx = t.Context()
	p.selected = "bench"
	tools := []struct {
		name    string
		display *engine.ToolDisplay
		text    string
	}{
		{"t1", show(engine.ToolDisplay{Kind: engine.ToolCommand, Command: "go test ./...", ExitCode: n(0)}), `{"job_id":"j","status":{"exit_code":0}}`},
		{"t2", show(engine.ToolDisplay{Kind: engine.ToolRead, Path: "src/main.go", Output: "alva-artifact://x"}), `{"reference":{"uri":"alva-artifact://x"}}`},
		{"t3", show(engine.ToolDisplay{Kind: engine.ToolEdit, Path: "src/main.go", Files: 1, Added: n(3), Removed: n(2), Diff: &engine.ToolDiff{Old: "a\nb\nc\nd", New: "a\nB\nc\nD\ne", Partial: true}}), `{"written_bytes":9}`},
		{"t4", show(engine.ToolDisplay{Kind: engine.ToolSearch, Query: "TODO", Path: ".", Matches: n(2), Results: []engine.ToolMatch{{Path: "a.go", Text: "TODO one"}, {Path: "b.go", Text: "TODO two"}}}), `{"matches":[]}`},
		{"t5", show(engine.ToolDisplay{Kind: engine.ToolTodo, Todos: engine.Checklist("- [x] one\n- [~] two\n- [ ] three")}), `{"saved":true}`},
		{"t6", show(engine.ToolDisplay{Kind: engine.ToolSubagent, Task: "Review", Status: "completed"}), `{"outcome":"completed"}`},
		{"demo_echo", show(engine.ToolDisplay{Kind: engine.ToolOther}), "echo"},
	}
	var entries []hub.HistoryEntry
	seq := uint64(0)
	for turn := 0; turn < 7000; turn++ {
		id, attempt := fmt.Sprintf("turn-%d", turn), fmt.Sprintf("attempt-%d", turn)
		tool := tools[turn%len(tools)]
		next := func() uint64 { seq++; return seq }
		entries = append(entries,
			hub.HistoryEntry{Sequence: next(), SessionID: "bench", TurnID: id, AttemptID: attempt, Input: fmt.Sprintf("第 %d 个问题", turn)},
			hub.HistoryEntry{Sequence: next(), SessionID: "bench", TurnID: id, AttemptID: attempt, Event: engine.Event{Kind: engine.ToolStart, ID: "call", Name: tool.name, Display: tool.display}},
			hub.HistoryEntry{Sequence: next(), SessionID: "bench", TurnID: id, AttemptID: attempt, Event: engine.Event{Kind: engine.ToolEnd, ID: "call", Name: tool.name, Text: tool.text, Display: tool.display}},
			hub.HistoryEntry{Sequence: next(), SessionID: "bench", TurnID: id, AttemptID: attempt, Event: engine.Event{Kind: engine.Message, ID: "assistant-0", Text: fmt.Sprintf("## 回答 %d\n\n", turn) + strings.Repeat("这是一段包含 **粗体** 和 `代码` 的正文。 ", 4+turn%12)}})
	}
	service.Transcripts = &product.TranscriptService{Store: fixedHistory(entries)}
	for p.transcript.Last() < uint64(len(entries)) {
		appendTranscript(t, p, entries...)
	}
	p.applyDetail()
	state := p.syncToolBlocks()
	for turn := 0; turn < 7000; turn += 10 {
		state.expanded[fmt.Sprintf("attempt-%d/call", turn)] = true
	}
	if len(p.model.Messages) != 21000 {
		t.Fatal("bench window does not hold 21k rows", len(p.model.Messages))
	}
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	list, ok := tester.Find("Message history")
	if !ok {
		t.Fatal("message list is not rendered")
	}
	var frames []time.Duration
	for _, delta := range []float32{-240, 240, -2400} {
		before, _ := p.chat.VisibleRows()
		for range 200 {
			start := time.Now()
			tester.Scroll(list.X+list.W/2, list.Y+list.H/2, 0, delta)
			tester.Frame()
			frames = append(frames, time.Since(start))
		}
		if after, _ := p.chat.VisibleRows(); after == before {
			t.Fatal("bench frames did not scroll the list", delta, before)
		}
	}
	sort.Slice(frames, func(i, j int) bool { return frames[i] < frames[j] })
	p95 := frames[len(frames)*95/100]
	t.Logf("21k rows with typed tool blocks headless scroll: p50=%s p95=%s max=%s n=%d", frames[len(frames)/2], p95, frames[len(frames)-1], len(frames))
	if p95 > 8*time.Millisecond {
		t.Fatal("scroll frame CPU p95 exceeds 8 ms", p95)
	}
}

func TestWorkspaceToolBlocksShellReceiptShowsStatusNotJSON(t *testing.T) {
	f := newToolBlocksFixture(t)
	receipt := `{"input":{"uri":"alva-artifact://in"},"job_id":"shell-1","output":"Use read_shell_output with this job_id for complete captured frame references","status":{"exit_code":2,"task":{"status":"failed"}}}`
	ended := show(engine.ToolDisplay{Kind: engine.ToolCommand, Command: "make test", ExitCode: n(2), Status: "failed", OutputInTrace: true})
	history := f.entries("t1", "job", "shell", show(engine.ToolDisplay{Kind: engine.ToolCommand, Command: "make test"}), ended, receipt, true, nil, nil)
	running := show(engine.ToolDisplay{Kind: engine.ToolCommand, Command: "sleep 100", Status: "running", OutputInTrace: true})
	history = append(history, f.entries("t1", "bg", "shell", running, nil, `{"job_id":"shell-2","status":{"task":{"status":"running"}}}`, false, nil, nil)...)
	replaceTranscript(t, f.p, history...)
	f.p.applyDetail()
	tester := ui.NewTester(workspaceStaticView(f.p), 1440, 960)
	if !tester.HasText("make test · 退出码 2") || !tester.HasText("sleep 100 · 运行中") {
		t.Fatal("receipt rows lack exit code or job status", tester.Texts())
	}
	connectionClick(t, tester, "Tool activity attempt-t1/job")
	if !tester.HasText("状态：失败 · 退出码 2\n命令输出保存在原始记录中，完整输出请在轨迹中查看。") {
		t.Fatal("receipt did not render as status text", tester.Texts())
	}
	for _, text := range tester.Texts() {
		if strings.Contains(text, "job_id") || strings.Contains(text, "read_shell_output") {
			t.Fatal("raw shell receipt JSON is shown", text)
		}
	}
	if _, ok := tester.Find("Inspect tool result attempt-t1/job"); !ok {
		t.Fatal("the full-output trace jump is gone")
	}
}

func TestWorkspaceToolBlocksFailedReadRetriesOnReexpand(t *testing.T) {
	f := newToolBlocksFixture(t)
	backend, err := f.p.Service.Repository.BoundReferences(t.Context(), f.session.ID, f.native)
	if err != nil {
		t.Fatal(err)
	}
	reference, err := backend.StoreText(t.Context(), "retried body text")
	if err != nil {
		t.Fatal(err)
	}
	history := f.entries("t1", "read", "reader", show(engine.ToolDisplay{Kind: engine.ToolRead, Path: "src/main.go"}), show(engine.ToolDisplay{Kind: engine.ToolRead, Path: "src/main.go", Output: reference.URI}), "{}", false, nil, nil)
	replaceTranscript(t, f.p, history...)
	f.p.applyDetail()
	f.inspector.failReads.Store(1)
	tester := ui.NewTester(workspaceStaticView(f.p), 1240, 850)
	connectionClick(t, tester, "Tool activity attempt-t1/read")
	waitToolBlocks(t, tester, f.p, func() bool {
		body := f.p.toolBlocks.bodies["attempt-t1/read"]
		return body != nil && !body.loading
	})
	if body := f.p.toolBlocks.bodies["attempt-t1/read"]; body.problem == "" || !tester.HasText("fixture read failure") {
		t.Fatal("the first read did not fail visibly", body)
	}
	reads := f.inspector.reads.Load()
	// A window update does not retry the failure on every poll.
	f.p.projectTranscript()
	tester.Frame()
	if f.inspector.reads.Load() != reads {
		t.Fatal("a window update retried a failed read")
	}
	connectionClick(t, tester, "Tool activity attempt-t1/read")
	connectionClick(t, tester, "Tool activity attempt-t1/read")
	waitToolBlocks(t, tester, f.p, func() bool {
		body := f.p.toolBlocks.bodies["attempt-t1/read"]
		return body != nil && !body.loading && body.problem == ""
	})
	if !tester.HasText("retried body text") || f.inspector.reads.Load() <= reads {
		t.Fatal("re-expanding did not retry the failed read", tester.Texts())
	}
}

func TestWorkspaceToolBlocksKeepStateAcrossGenerationButNotBranch(t *testing.T) {
	f := newToolBlocksFixture(t)
	backend, err := f.p.Service.Repository.BoundReferences(t.Context(), f.session.ID, f.native)
	if err != nil {
		t.Fatal(err)
	}
	reference, err := backend.StoreText(t.Context(), "kept body text")
	if err != nil {
		t.Fatal(err)
	}
	history := f.entries("t1", "read", "reader", show(engine.ToolDisplay{Kind: engine.ToolRead, Path: "src/main.go"}), show(engine.ToolDisplay{Kind: engine.ToolRead, Path: "src/main.go", Output: reference.URI}), "{}", false, nil, nil)
	replaceTranscript(t, f.p, history...)
	f.p.applyDetail()
	tester := ui.NewTester(workspaceStaticView(f.p), 1240, 850)
	connectionClick(t, tester, "Tool activity attempt-t1/read")
	waitToolBlocks(t, tester, f.p, func() bool { return tester.HasText("kept body text") })
	reads := f.inspector.reads.Load()
	for range 3 {
		f.p.generation++ // Send, Stop, permission and plan changes
		f.p.projectTranscript()
		tester.Frame()
	}
	if !f.p.toolBlocks.expanded["attempt-t1/read"] || !tester.HasText("kept body text") || f.inspector.reads.Load() != reads {
		t.Fatal("a generation change reset the expansion or re-read the body")
	}
	f.p.detail.Session.ActiveBranchID = "other-branch"
	tester.Frame()
	if f.p.toolBlocks.expanded["attempt-t1/read"] || f.p.toolBlocks.bodies["attempt-t1/read"] != nil {
		t.Fatal("a branch change kept the previous branch's tool state")
	}
}

func TestWorkspaceToolBlocksPendingTabDroppedOnFailedReadOrBranchChange(t *testing.T) {
	f := newToolBlocksFixture(t)
	tab := hub.WorkspaceTab{Kind: "files", Title: "项目文件", BranchID: f.p.detail.Session.ActiveBranchID, Query: "main.go"}
	tester := ui.NewTester(workspaceStaticView(f.p), 1440, 960)
	// A failed view read drops the requested tab.
	if err := f.p.Service.WorkspaceViews.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	f.p.openToolResource(tab)
	if f.p.resources.pending == nil {
		t.Fatal("tab requested before the view loaded was not kept pending")
	}
	waitToolBlocks(t, tester, f.p, func() bool {
		s := f.p.workspaceResourceSession()
		return s != nil && !s.loading
	})
	if f.p.resources.pending != nil {
		t.Fatal("a failed view read kept the pending tab")
	}
	// A branch change before the view loads drops it as well.
	port, _ := f.p.Service.Store.(hub.WorkspaceViewStore)
	var err error
	if f.p.Service.WorkspaceViews, err = product.NewWorkspaceViewsService(port); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = f.p.Service.WorkspaceViews.Close(context.Background()) })
	f.p.resources.sessions = nil
	f.p.resources.session = ""
	f.p.openToolResource(tab)
	f.p.detail.Session.ActiveBranchID = "other-branch"
	waitToolBlocks(t, tester, f.p, func() bool {
		s := f.p.workspaceResourceSession()
		return s != nil && s.loaded
	})
	if f.p.resources.pending != nil {
		t.Fatal("pending tab survived the load")
	}
	for _, open := range f.p.workspaceResourceSession().view.Tabs {
		if open.Kind == "files" && open.Query == "main.go" {
			t.Fatal("a tab requested on the previous branch was opened after the branch changed")
		}
	}
}
