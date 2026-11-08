// INPUT: Real product stores with local Alva-broker and native-callback engines, synthetic committed history and headless MyGo controls
// OUTPUT: Card texts per state, in-card actions reaching the right product method, hidden pinned panels in the conversation, late-receipt drops, failure recovery, the pending jump/entry, usage lines with and without cache and Markdown tables
// POS: [L7 Hub UI tests] Interaction cards and usage lines without external processes, accounts, models or OS windows; ALVA_INTERACTION_REVIEW_DIR optionally saves headless PNGs
//go:build desktop

package hubdesktop

import (
	"image/png"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/egoist/mygo/ui"
)

func allowed(value bool) *bool { return &value }
func count(n uint64) *uint64   { return &n }

// interactionHistory is one attempt per settled request plus usage reports.
func interactionHistory(session string) []hub.HistoryEntry {
	var entries []hub.HistoryEntry
	seq := uint64(0)
	add := func(attempt string, event engine.Event) {
		seq++
		entries = append(entries, hub.HistoryEntry{Sequence: seq, SessionID: session, TurnID: "turn-" + attempt, AttemptID: attempt, Event: event})
	}
	command := &engine.ToolDisplay{Kind: engine.ToolCommand, Command: "go test ./..."}
	edit := &engine.ToolDisplay{Kind: engine.ToolEdit, Path: "a.go"}
	question := &engine.UserInputRequest{Questions: []engine.UserInputQuestion{{ID: "pick", Question: "A or B?", Options: []engine.UserInputOption{{Label: "A"}, {Label: "B"}}}}}
	free := &engine.UserInputRequest{Questions: []engine.UserInputQuestion{{ID: "secret", Question: "Token?", IsSecret: true}}}
	add("a1", engine.Event{Kind: engine.PermissionRequest, ID: "p1", Name: "exec", Display: command})
	add("a1", engine.Event{Kind: engine.InteractionResolved, ID: "p1", Name: "permission", Resolution: &engine.InteractionResolution{Allowed: allowed(true)}})
	add("a1", engine.Event{Kind: engine.PermissionRequest, ID: "p2", Name: "write", Display: edit})
	add("a1", engine.Event{Kind: engine.InteractionResolved, ID: "p2", Name: "permission", Resolution: &engine.InteractionResolution{Allowed: allowed(false)}})
	add("a1", engine.Event{Kind: engine.UserInputRequested, ID: "q1", Question: question})
	add("a1", engine.Event{Kind: engine.InteractionResolved, ID: "q1", Name: "user_input", Resolution: &engine.InteractionResolution{Answers: map[string][]string{"pick": {"B"}}}})
	add("a1", engine.Event{Kind: engine.UserInputRequested, ID: "q2", Question: free})
	add("a1", engine.Event{Kind: engine.InteractionResolved, ID: "q2", Name: "user_input", Resolution: &engine.InteractionResolution{Answers: map[string][]string{"secret": {"[REDACTED]"}}}})
	add("a1", engine.Event{Kind: engine.UserInputRequested, ID: "q3", Question: question})
	add("a1", engine.Event{Kind: engine.InteractionResolved, ID: "q3", Name: "user_input", Resolution: &engine.InteractionResolution{}})
	add("a1", engine.Event{Kind: engine.UserInputRequested, ID: "q4", Question: free})
	add("a1", engine.Event{Kind: engine.InteractionResolved, ID: "q4", Name: "user_input", Resolution: &engine.InteractionResolution{Answers: map[string][]string{"secret": {}}}})
	add("a1", engine.Event{Kind: engine.PermissionRequest, ID: "p3", Name: "exec"})
	add("a1", engine.Event{Kind: engine.InteractionResolved, ID: "p3", Name: "permission"})
	add("a1", engine.Event{Kind: engine.Message, ID: "reply", Text: "Done."})
	add("a1", engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded, Usage: &engine.Usage{InputTokens: 1000, OutputTokens: 50, CachedInputTokens: count(800)}})
	add("a2", engine.Event{Kind: engine.PermissionRequest, ID: "p4", Name: "exec", Display: command})
	add("a2", engine.Event{Kind: engine.Message, ID: "reply", Text: "Stopped."})
	add("a2", engine.Event{Kind: engine.Completed, Outcome: engine.Failed, Usage: &engine.Usage{InputTokens: 200, OutputTokens: 10}})
	return entries
}

func TestWorkspaceInteractionCardTextsAndUsageLines(t *testing.T) {
	s, _ := fixture(t)
	session, err := s.Create(t.Context(), "interaction texts", "")
	if err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(s)
	p.selected = session.ID
	if p.detail, err = s.Store.Detail(t.Context(), session.ID); err != nil {
		t.Fatal(err)
	}
	replaceTranscript(t, p, interactionHistory(session.ID)...)
	p.applyDetail()
	tester := ui.NewTester(workspaceStaticView(p), 1240, 2400)
	for _, text := range []string{"已允许 · 运行命令 `go test ./...`", "已拒绝 · 修改 a.go", "已回答 · 选择了 B", "已回答 · 回答：[REDACTED]", "已跳过 · A or B?", "已回答 · Token?", "已处理 · 使用 exec", "已过期 · 运行命令 `go test ./...`", "输入 1,000 · 输出 50 · 缓存命中 80%", "输入 200 · 输出 10"} {
		if !tester.HasText(text) {
			t.Fatal("card or usage text missing", text, tester.Texts())
		}
	}
	for _, text := range tester.Texts() {
		if strings.HasPrefix(text, "输入 200") && strings.Contains(text, "缓存") {
			t.Fatal("unreported cache shown as a hit rate", text)
		}
	}
	if _, found := tester.Find("允许"); found {
		t.Fatal("a settled request still offers its action")
	}
}

func TestWorkspaceAlvaApprovalCardDecidesThroughTheProduct(t *testing.T) {
	for _, allow := range []bool{true, false} {
		label := map[bool]string{true: "允许", false: "拒绝"}[allow]
		t.Run(label, func(t *testing.T) {
			p, owner := nativeApprovalPage(t)
			p.pane = ""
			p.projectTranscript()
			tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
			if !tester.HasText("需要你的允许") || !tester.HasText("使用 write_file") {
				t.Fatal("waiting Alva approval is not a card in the stream", tester.Texts())
			}
			if _, pinned := tester.Find("Allow once"); pinned {
				t.Fatal("the conversation still repeats the request in the pinned panel")
			}
			card, ok := tester.Find(label)
			if !ok || card.H != 32 {
				t.Fatal("card action missing", card)
			}
			if err := tester.Click(label); err != nil {
				t.Fatal(err)
			}
			waitWorkspace(t, p, tester, func() bool { return len(p.interactionCards.busy) == 0 && !p.Service.Runtime.HasActive() })
			select {
			case decision := <-owner.decided:
				if decision.Allowed != allow || decision.Scope != security.Once || owner.calls.Load() != 1 {
					t.Fatal("card decision changed or repeated", decision, owner.calls.Load())
				}
			case <-time.After(time.Second):
				t.Fatal("card decision never reached the execution owner")
			}
			if tester.HasText("需要你的允许") || len(p.approvals) != 0 || len(p.approvalSubmitting) != 0 {
				t.Fatal("decided request still waits", p.approvals)
			}
		})
	}
}

func TestWorkspaceNativeEngineCardsAnswerThroughTheProduct(t *testing.T) {
	p, e := workspaceInteractionPage(t)
	p.pane = ""
	tail, err := p.Service.Transcripts.Tail(t.Context(), p.selected)
	if err != nil {
		t.Fatal(err)
	}
	p.transcript.Reset(tail)
	p.projectTranscript()
	tester := ui.NewTester(workspaceStaticView(p), 1240, 1400)
	if !tester.HasText("需要你的允许") || !tester.HasText("需要你的回答") || !tester.HasText("Choose a color") {
		t.Fatal("native requests are not cards at their history blocks", tester.Texts())
	}
	if _, pinned := tester.Find("Allow native once"); pinned {
		t.Fatal("the conversation still repeats the native request in the pinned panel")
	}
	if err := tester.Click("拒绝"); err != nil {
		t.Fatal(err)
	}
	waitWorkspace(t, p, tester, func() bool { return e.permissionCalls.Load() == 1 && len(p.interactionCards.busy) == 0 })
	if e.questionCalls.Load() != 0 {
		t.Fatal("a permission click answered the question")
	}
	if err := tester.Click("Blue"); err != nil {
		t.Fatal(err)
	}
	if err := tester.Click("回答 · " + p.interactionRowID(t, "question")); err != nil {
		t.Fatal(err)
	}
	tester.Type("private")
	e.rejectAnswer.Store(true)
	if err := tester.Click("提交回答"); err != nil {
		t.Fatal(err)
	}
	waitWorkspace(t, p, tester, func() bool { return len(p.interactionCards.busy) == 0 && len(p.interactionCards.problem) == 1 })
	if !tester.HasText("fixture answer was rejected") || !tester.HasText("✓ Blue") {
		t.Fatal("failure was not shown on the card with its draft kept", tester.Texts())
	}
	if err := tester.Click("提交回答"); err != nil {
		t.Fatal(err)
	}
	select {
	case response := <-e.answers:
		if strings.Join(response.Answers["color"], ",") != "Blue" || strings.Join(response.Answers["secret"], ",") != "private" {
			t.Fatal("answers changed", response.Answers)
		}
	case <-time.After(time.Second):
		t.Fatal("answer never reached the native owner")
	}
	waitWorkspace(t, p, tester, func() bool { return !p.Service.Runtime.HasActive() && len(p.interactionCards.busy) == 0 })
	tail, err = p.Service.Transcripts.Tail(t.Context(), p.selected)
	if err != nil {
		t.Fatal(err)
	}
	p.transcript.Reset(tail)
	p.projectTranscript()
	tester.Frame()
	if tester.HasText("需要你的回答") || !tester.HasText("已处理 · 使用 fileChange") {
		t.Fatal("settled native requests did not collapse", tester.Texts())
	}
}

// interactionRowID is the row of the first card of a kind.
func (p *NativePage) interactionRowID(t *testing.T, kind string) string {
	t.Helper()
	for id, item := range p.interactionCards.actions {
		if string(item.View.Kind) == kind {
			return id
		}
	}
	t.Fatal("no card of kind", kind)
	return ""
}

func TestWorkspaceInteractionCardDropsLateReceiptAfterSwitching(t *testing.T) {
	p, owner := nativeApprovalPage(t)
	p.pane = ""
	p.projectTranscript()
	wrapped := &workspaceDelayedDetail{ServiceStore: p.Service.Store, entered: make(chan struct{}), release: make(chan struct{})}
	p.Service.Store = wrapped
	wrapped.block.Store(true)
	var release sync.Once
	defer release.Do(func() { close(wrapped.release) })
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	if err := tester.Click("拒绝"); err != nil {
		t.Fatal(err)
	}
	select {
	case <-wrapped.entered:
	case <-time.After(time.Second):
		t.Fatal("card response did not reach product validation")
	}
	if box, ok := tester.Find("拒绝"); !ok || !tester.Focused("拒绝") || box.H != 32 {
		t.Fatal("in-flight card action lost its geometry or focus", box)
	}
	// Switching the branch starts another card scope; its state must not see
	// the old receipt.
	p.detail.Session.ActiveBranchID = "another-branch"
	p.problem = "current view message"
	tester.Frame()
	if p.interactionCards.scope.branch != "another-branch" || len(p.interactionCards.busy) != 0 {
		t.Fatal("switching the branch kept the old card state")
	}
	release.Do(func() { close(wrapped.release) })
	waitWorkspace(t, p, tester, func() bool { return len(p.approvalSubmitting) == 0 })
	if decision := <-owner.decided; decision.Allowed {
		t.Fatal("decision changed", decision)
	}
	if len(p.interactionCards.problem) != 0 || len(p.interactionCards.busy) != 0 || p.problem != "current view message" {
		t.Fatal("a late card receipt changed the switched view", p.interactionCards.problem, p.problem)
	}
}

func TestWorkspaceInteractionJumpAndOtherViewsEntry(t *testing.T) {
	p, _ := nativeApprovalPage(t)
	p.pane = ""
	var history []hub.HistoryEntry
	for i := 1; i <= 60; i++ {
		history = append(history, hub.HistoryEntry{Sequence: uint64(i), SessionID: p.selected, TurnID: "t", AttemptID: "a", Event: engine.Event{Kind: engine.Message, ID: "m" + strings.Repeat("x", i), Text: "paragraph " + strings.Repeat("words ", 30)}})
	}
	replaceTranscript(t, p, history...)
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	p.chat.ScrollToRow(0)
	tester.Frame()
	tester.Frame()
	if _, ok := tester.Find("Jump to the waiting request"); !ok {
		t.Fatal("an out-of-view waiting card offers no jump", tester.Texts())
	}
	if first, _ := p.chat.VisibleRows(); first != 0 {
		t.Fatal("a new request scrolled the list by itself", first)
	}
	if err := tester.Click("Jump to the waiting request"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	tester.Frame()
	if !tester.HasText("需要你的允许") {
		t.Fatal("jump did not bring the card into view")
	}
	p.trace.active = true
	tester.Frame()
	if _, pinned := tester.Find("Allow once"); pinned || !tester.HasText("有 1 个请求等待处理") {
		t.Fatal("the trace view lacks its one-line entry or repeats the panel", tester.Texts())
	}
	if err := tester.Click("Open waiting requests in the conversation"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if p.trace.active || !tester.HasText("需要你的允许") {
		t.Fatal("the entry did not return to the card")
	}
	for _, pane := range []string{"waits", "tools"} {
		p.pane = pane
		tester.Frame()
		if _, pinned := tester.Find("Allow once"); !pinned || !tester.HasText("有 1 个请求等待处理") {
			t.Fatal("another view lost its entry to the request", pane)
		}
	}
}

func TestWorkspaceQuestionCardChoices(t *testing.T) {
	p, broker, q, answered := nativeQuestionPage(t, "multiple")
	p.pane = ""
	p.projectTranscript()
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	if !tester.HasText("需要你的回答") || !tester.HasText("Choose explicitly") {
		t.Fatal("Alva question is not a card", tester.Texts())
	}
	if _, skip := tester.Find("跳过"); skip {
		t.Fatal("an Alva question offered a skip it cannot take")
	}
	for _, label := range []string{"First", "Second"} {
		if err := tester.Click(label); err != nil {
			t.Fatal(err)
		}
		tester.Frame()
	}
	if err := tester.Click("提交回答"); err != nil {
		t.Fatal(err)
	}
	select {
	case answer := <-answered:
		if len(answer.Selected) != 2 || answer.Selected[0] != 1 || answer.Selected[1] != 2 || answer.Text != nil {
			t.Fatal("multiple choice changed", answer)
		}
	case <-time.After(time.Second):
		t.Fatal("answer never reached the broker", broker.PendingQuestionsSnapshot(), q.ID)
	}
	waitWorkspace(t, p, tester, func() bool { return len(p.interactionCards.busy) == 0 && len(p.questions) == 0 })
	if tester.HasText("需要你的回答") {
		t.Fatal("answered question still waits")
	}
}

// TestWorkspaceInteractionCardsBothStates shows each card type waiting next
// to its settled line; ALVA_INTERACTION_REVIEW_DIR saves the headless frames.
func TestWorkspaceInteractionCardsBothStates(t *testing.T) {
	settled := func(session string, kind engine.Kind) []hub.HistoryEntry {
		var out []hub.HistoryEntry
		for _, entry := range interactionHistory(session) {
			id := entry.Event.ID
			switch {
			case kind == engine.PermissionRequest && (id == "p1" || id == "p2"), kind == engine.UserInputRequested && (id == "q1" || id == "q2"), id == "reply" && entry.AttemptID == "a1", entry.Event.Kind == engine.Completed && entry.AttemptID == "a1":
				out = append(out, entry)
			}
		}
		return out
	}
	approval, _ := nativeApprovalPage(t)
	approval.pane = ""
	replaceTranscript(t, approval, settled(approval.selected, engine.PermissionRequest)...)
	tester := ui.NewTester(workspaceStaticView(approval), 1000, 640)
	tester.Frame()
	if !tester.HasText("已允许 · 运行命令 `go test ./...`") || !tester.HasText("需要你的允许") {
		t.Fatal("approval states missing", tester.Texts())
	}
	saveInteractionReview(t, tester, "approval-card-waiting-and-settled.png")
	question, _, _, _ := nativeQuestionPage(t, "single")
	question.pane = ""
	replaceTranscript(t, question, settled(question.selected, engine.UserInputRequested)...)
	tester = ui.NewTester(workspaceStaticView(question), 1000, 720)
	tester.Frame()
	if !tester.HasText("已回答 · 选择了 B") || !tester.HasText("需要你的回答") {
		t.Fatal("question states missing", tester.Texts())
	}
	saveInteractionReview(t, tester, "question-card-waiting-and-settled.png")
}

func saveInteractionReview(t *testing.T, tester *ui.Tester, name string) {
	t.Helper()
	dir := os.Getenv("ALVA_INTERACTION_REVIEW_DIR")
	if dir == "" {
		return
	}
	if err := os.MkdirAll(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	f, err := os.Create(filepath.Join(dir, name))
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	if err := png.Encode(f, tester.Image()); err != nil {
		t.Fatal(err)
	}
}

func TestWorkspaceMarkdownTablesRenderInReplies(t *testing.T) {
	s, _ := fixture(t)
	session, err := s.Create(t.Context(), "tables", "")
	if err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(s)
	p.selected = session.ID
	if p.detail, err = s.Store.Detail(t.Context(), session.ID); err != nil {
		t.Fatal(err)
	}
	table := "| Engine | Cache |\n|---|---|\n| first-cell | 80% |\n| second-cell | none |\n"
	replaceTranscript(t, p, hub.HistoryEntry{Sequence: 1, SessionID: session.ID, TurnID: "t", AttemptID: "a", Event: engine.Event{Kind: engine.Message, ID: "reply", Text: table}})
	p.applyDetail()
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	for _, cell := range []string{"Engine", "Cache", "first-cell", "80%", "second-cell", "none"} {
		if !tester.HasText(cell) {
			t.Fatal("table cell not rendered", cell, tester.Texts())
		}
	}
	if tester.HasText("|---|---|") || tester.HasText("| first-cell | 80% |") {
		t.Fatal("table shown as raw Markdown")
	}
}

// nativeCardPage is the native engine fixture in the conversation view with
// its committed history and pending requests bound into rows.
func nativeCardPage(t *testing.T) (*NativePage, *workspaceInteractionEngine, *ui.Tester) {
	t.Helper()
	p, e := workspaceInteractionPage(t)
	p.pane = ""
	tail, err := p.Service.Transcripts.Tail(t.Context(), p.selected)
	if err != nil {
		t.Fatal(err)
	}
	p.transcript.Reset(tail)
	p.projectTranscript()
	return p, e, ui.NewTester(workspaceStaticView(p), 1240, 1400)
}

func waitNativeRead(t *testing.T, p *NativePage, tester *ui.Tester, done func() bool) {
	t.Helper()
	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		tester.Frame()
		if done() && !p.engineInteractions.polling {
			return
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("native read did not settle", p.engineInteractions.problem)
}

func TestWorkspaceConversationShowsPendingReadProblemsWithRetry(t *testing.T) {
	p, _, tester := nativeCardPage(t)
	healthy := p.Service.Store
	p.Service.Store = workspaceEngineInteractionReadFailure{healthy}
	p.pollEngineInteractions()
	waitNativeRead(t, p, tester, func() bool { return p.engineInteractions.problem != "" })
	if !tester.HasText("Native requests could not be refreshed: fixture native request read failure") {
		t.Fatal("the conversation hid a pending read failure", tester.Texts())
	}
	if _, retry := tester.Find("重新读取请求"); !retry {
		t.Fatal("no retry beside the read failure")
	}
	p.Service.Store = healthy
	if err := tester.Click("重新读取请求"); err != nil {
		t.Fatal(err)
	}
	waitNativeRead(t, p, tester, func() bool { return p.engineInteractions.current })
	if tester.HasText("fixture native request read failure") || !tester.HasText("需要你的回答") {
		t.Fatal("retry did not restore the requests", tester.Texts())
	}
	p.approvalReadProblem = "Pending permissions could not be refreshed: fixture"
	tester.Frame()
	if !tester.HasText("Pending permissions could not be refreshed: fixture") {
		t.Fatal("the conversation hid the approval read failure")
	}
}

func TestWorkspaceCardDraftSurvivesAFailedRead(t *testing.T) {
	p, e, tester := nativeCardPage(t)
	question := p.interactionRowID(t, "question")
	item := p.interactionCards.actions[question]
	if err := tester.Click("Blue"); err != nil {
		t.Fatal(err)
	}
	if err := tester.Click("回答 · " + question); err != nil {
		t.Fatal(err)
	}
	tester.Type("private draft")
	draft := func() string {
		d := p.interactionCards.drafts[item.Key()]
		if d == nil || d.text["secret"] == nil {
			return ""
		}
		return *d.text["secret"]
	}
	healthy := p.Service.Store
	p.Service.Store = workspaceEngineInteractionReadFailure{healthy}
	p.pollEngineInteractions()
	waitNativeRead(t, p, tester, func() bool { return p.engineInteractions.problem != "" })
	// Newer history arrives while the failed read leaves the request out.
	p.projectTranscript()
	tester.Frame()
	if draft() != "private draft" || len(p.interactionCards.drafts[item.Key()].choices["color"]) != 1 {
		t.Fatal("a failed read discarded the typed answer")
	}
	p.Service.Store = healthy
	p.pollEngineInteractions()
	waitNativeRead(t, p, tester, func() bool { return p.engineInteractions.current })
	if draft() != "private draft" || !tester.HasText("✓ Blue") {
		t.Fatal("the restored card lost its draft")
	}
	// A successful read without the request does drop the draft.
	if err := p.Service.RespondEngineUserInput(t.Context(), p.selected, mustEngineHandle(t, item), engine.UserInputResponse{}); err != nil {
		t.Fatal(err)
	}
	<-e.answers
	p.pollEngineInteractions()
	waitNativeRead(t, p, tester, func() bool { return p.interactionCards.drafts[item.Key()] == nil })
}

func mustEngineHandle(t *testing.T, item product.Interaction) product.EngineInteractionHandle {
	t.Helper()
	handle, ok := item.Engine()
	if !ok {
		t.Fatal("not a native request")
	}
	return handle
}

func TestWorkspaceCardSharesThePanelsInFlightState(t *testing.T) {
	p, owner := nativeApprovalPage(t)
	p.pane = ""
	p.projectTranscript()
	p.approvalSubmitting = map[string]bool{p.approvals[0].ID: true}
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	if _, loading := tester.Find("Loading indicator"); !loading {
		t.Fatal("the card did not show the panel's in-flight request as busy")
	}
	if err := tester.Click("允许"); err != nil {
		t.Fatal(err)
	}
	time.Sleep(20 * time.Millisecond)
	if owner.calls.Load() != 0 || len(p.interactionCards.busy) != 0 {
		t.Fatal("the card submitted a request already in flight from its panel")
	}
}

func TestWorkspaceUnchangedNativeReadKeepsRows(t *testing.T) {
	p, _, tester := nativeCardPage(t)
	p.engineInteractions.current = true
	before := &p.transcriptRows[0]
	p.pollEngineInteractions()
	waitNativeRead(t, p, tester, func() bool { return true })
	if &p.transcriptRows[0] != before {
		t.Fatal("an unchanged pending read projected the rows again")
	}
}

func TestWorkspaceAlvaQuestionDraftSurvivesAFailedRead(t *testing.T) {
	p, _, q, _ := nativeQuestionPage(t, "text")
	p.pane = ""
	p.questionsCurrent = true
	p.projectTranscript()
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	row := ""
	for id := range p.interactionCards.actions {
		row = id
	}
	if err := tester.Click("回答 · " + row); err != nil {
		t.Fatal(err)
	}
	tester.Type("typed before the failure")
	key := "question/" + q.ID
	// A failed question read leaves the request out of the rows.
	p.questions, p.questionsCurrent, p.questionReadProblem = nil, false, "Pending questions could not be refreshed: fixture"
	p.projectTranscript()
	tester.Frame()
	if d := p.interactionCards.drafts[key]; d == nil || *d.text[q.ID] != "typed before the failure" {
		t.Fatal("a failed question read discarded the draft")
	}
	if !tester.HasText("Pending questions could not be refreshed: fixture") {
		t.Fatal("the failure was not shown in the conversation")
	}
	// A successful read without the request drops it.
	p.questionsCurrent, p.questionReadProblem = true, ""
	p.projectTranscript()
	if p.interactionCards.drafts[key] != nil {
		t.Fatal("a draft outlived its answered request")
	}
}
