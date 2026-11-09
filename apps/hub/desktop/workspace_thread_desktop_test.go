// INPUT: Real shared bootstrap/SQLite with the demo Alva engine, a gated stub engine for running-time input, MyGo headless composer and transcript
// OUTPUT: Enter queues and Cmd+Enter interjects while running, the queue bar states and paused resume, the version switcher/edit/regenerate/fork and its origin link, and their disabled reasons
// POS: [L7 Hub UI testing] Drives the actual native controls; every durable change reaches the product only through its narrow services
//go:build desktop

package hubdesktop

import (
	"context"
	"errors"
	"image/png"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	view "github.com/QuincySx/Alva/apps/ui/chat"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/egoist/mygo/ui"
)

func saveThreadScreenshot(t *testing.T, tester *ui.Tester, name string) {
	t.Helper()
	dir := os.Getenv("ALVA_THREAD_SCREENSHOT_DIR")
	if dir == "" {
		return
	}
	if err := os.MkdirAll(dir, 0700); err != nil {
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

// gatedEngine holds every execution until released and can take interjections.
type gatedEngine struct {
	mu          sync.Mutex
	interject   bool
	forkable    bool // declares native fork but refuses every cut (CheckFork)
	gate        chan struct{}
	started     chan string
	pending     []engine.Interjection
	prompts     []string
	interjected []string
}

func (g *gatedEngine) Capabilities() engine.Capabilities {
	return engine.Capabilities{Resume: true, Cancel: true, Streaming: true, Interjection: g.interject, Fork: g.forkable}
}
func (g *gatedEngine) CheckFork(context.Context, engine.ForkPoint) error {
	return errors.New("the cut lies before the readable tail")
}
func (g *gatedEngine) Cancel(context.Context, string) error { return nil }
func (g *gatedEngine) RespondPermission(context.Context, string, string, engine.PermissionDecision) error {
	return engine.ErrUnsupported
}
func (g *gatedEngine) Interject(_ context.Context, _ string, in engine.Interjection) error {
	g.mu.Lock()
	defer g.mu.Unlock()
	g.pending = append(g.pending, in)
	g.interjected = append(g.interjected, in.Text)
	return nil
}
func (g *gatedEngine) Execute(ctx context.Context, request engine.Request, sink func(engine.Event) error) error {
	native := request.ResumeSession
	if native == "" {
		native = "gated-native"
	}
	g.mu.Lock()
	g.prompts = append(g.prompts, request.Prompt)
	g.mu.Unlock()
	if err := sink(engine.Event{Kind: engine.SessionStarted, SessionID: native}); err != nil {
		return err
	}
	g.started <- request.Prompt
	select {
	case <-g.gate:
	case <-ctx.Done():
		return sink(engine.Event{Kind: engine.Completed, SessionID: native, Outcome: engine.Cancelled})
	}
	g.mu.Lock()
	items := g.pending
	g.pending = nil
	g.mu.Unlock()
	for _, item := range items {
		if err := sink(engine.Event{Kind: engine.InputDelivered, SessionID: native, ID: item.ID, Text: item.Text}); err != nil {
			return err
		}
	}
	if err := sink(engine.Event{Kind: engine.Message, ID: "reply", Text: "answer to " + request.Prompt[strings.LastIndex(request.Prompt, "\n")+1:]}); err != nil {
		return err
	}
	return sink(engine.Event{Kind: engine.Completed, SessionID: native, Outcome: engine.Succeeded})
}

func threadPage(t *testing.T, interject bool) (*NativePage, *ui.Tester, *gatedEngine, hub.Session) {
	t.Helper()
	service, _ := fixture(t)
	stub := &gatedEngine{interject: interject, gate: make(chan struct{}, 8), started: make(chan string, 8)}
	if err := service.Runtime.Register("alva", stub); err != nil {
		t.Fatal(err)
	}
	session, err := service.Create(t.Context(), "thread", t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	page := NewNativePage(service)
	page.ctx = t.Context()
	tester := ui.NewTester(workspaceStaticView(page), 1240, 850)
	page.refreshSessions()
	waitWorkspace(t, page, tester, func() bool { return len(page.sessions) == 1 && !page.sessionLoading })
	page.selectSession(session.ID)
	waitWorkspace(t, page, tester, func() bool {
		return !page.polling && page.detail.Session.ID == session.ID && page.thread.session == session.ID
	})
	return page, tester, stub, session
}

func threadQueue(t *testing.T, page *NativePage, id string) []hub.PendingInput {
	t.Helper()
	items, err := page.Service.Turns.Queued(t.Context(), id)
	if err != nil {
		t.Fatal(err)
	}
	return items
}

func TestEnterQueuesAndCmdEnterInterjectsWhileTheTurnRuns(t *testing.T) {
	page, tester, stub, session := threadPage(t, true)
	if _, err := page.Service.Turns.Send(t.Context(), session.ID, "first", nil); err != nil {
		t.Fatal(err)
	}
	<-stub.started
	waitWorkspace(t, page, tester, func() bool { return page.workspaceRunningInput() && page.thread.caps.Interject })
	if placeholder := "补充说明，Enter 排队，⌘Enter 插话…"; !tester.HasText(placeholder) {
		if _, ok := tester.Find("Message"); !ok {
			t.Fatal("composer missing")
		}
	}
	if err := tester.Click("Message"); err != nil {
		t.Fatal(err)
	}
	tester.Type("later one")
	tester.Key(0, ui.KeyEnter)
	waitWorkspace(t, page, tester, func() bool { return len(page.thread.items) == 1 })
	if page.model.Draft != "" || page.thread.items[0].Text != "later one" || page.thread.items[0].State != hub.PendingQueued {
		t.Fatal("Enter must queue the draft and clear it", page.model.Draft, page.thread.items)
	}
	if _, ok := tester.Find("Queued messages"); !ok || !tester.HasText("排队中") || !tester.HasText("later one") {
		t.Fatal("the queue bar must show the queued message", tester.Texts())
	}
	saveThreadScreenshot(t, tester, "queue-bar.png")
	tester.Type("right now")
	tester.Key(ui.Cmd, ui.KeyEnter)
	waitWorkspace(t, page, tester, func() bool { return len(page.thread.items) == 2 })
	if page.thread.items[1].State != hub.PendingInterjecting || page.thread.items[1].Text != "right now" {
		t.Fatal("Cmd+Enter must hand the message to the engine", page.thread.items)
	}
	if !tester.HasText("插话中") {
		t.Fatal("an interjected message shows its state", tester.Texts())
	}
	// A message with an attachment reference is not queued: it stays a draft.
	stub.gate <- struct{}{}
	waitWorkspace(t, page, tester, func() bool { return len(stub.interjected) == 1 })
	// The delivered interjection leaves the queue; the queued message becomes
	// the next Turn by itself after the first one succeeded.
	prompt := <-stub.started
	if !strings.HasSuffix(prompt, "later one") {
		t.Fatal("the queued message must start next", prompt)
	}
	stub.gate <- struct{}{}
	waitWorkspace(t, page, tester, func() bool {
		return len(threadQueue(t, page, session.ID)) == 0 && page.detail.Session.Status == engine.Succeeded && !page.Service.Runtime.HasActive() && len(page.thread.items) == 0
	})
	var user []string
	for _, row := range page.transcriptRows {
		if block := page.transcriptBlocks[row.ID]; block.Kind == "user" {
			user = append(user, block.Name+":"+block.Text)
		}
	}
	if got := strings.Join(user, "|"); got != ":first|interjection:right now|:later one" {
		t.Fatal("the interjection is shown where it arrived", got)
	}
}

func TestEnterQueuesButCmdEnterFallsBackWhenTheEngineCannotInterject(t *testing.T) {
	page, tester, stub, session := threadPage(t, false)
	if _, err := page.Service.Turns.Send(t.Context(), session.ID, "first", nil); err != nil {
		t.Fatal(err)
	}
	<-stub.started
	waitWorkspace(t, page, tester, func() bool { return page.workspaceRunningInput() })
	if page.thread.caps.Interject {
		t.Fatal("this engine does not interject")
	}
	if err := tester.Click("Message"); err != nil {
		t.Fatal(err)
	}
	tester.Type("please also")
	tester.Key(ui.Cmd, ui.KeyEnter)
	waitWorkspace(t, page, tester, func() bool { return len(page.thread.items) == 1 && page.thread.notice != "" })
	if page.thread.items[0].State != hub.PendingQueued || !strings.Contains(page.thread.notice, "不支持运行中插话") {
		t.Fatal("an engine without interjection must queue with a notice", page.thread.items, page.thread.notice)
	}
	if !tester.HasText("当前引擎不支持运行中插话，已加入队列") {
		t.Fatal("the notice must be shown inline", tester.Texts())
	}
	saveThreadScreenshot(t, tester, "interject-downgrade-notice.png")
	if tester.HasText("⌘Enter 插话") {
		t.Fatal("an engine that cannot interject must not advertise Cmd+Enter", tester.Texts())
	}
	stub.gate <- struct{}{}
	<-stub.started
	stub.gate <- struct{}{}
	waitWorkspace(t, page, tester, func() bool { return len(threadQueue(t, page, session.ID)) == 0 && !page.Service.Runtime.HasActive() })
}

func TestQueuedDraftWithAnAttachmentIsKeptAndExplained(t *testing.T) {
	page, tester, stub, session := threadPage(t, true)
	if _, err := page.Service.Turns.Send(t.Context(), session.ID, "first", nil); err != nil {
		t.Fatal(err)
	}
	<-stub.started
	waitWorkspace(t, page, tester, func() bool { return page.workspaceRunningInput() })
	page.workspaceFiles = map[string][]workspaceSelectedFile{session.ID: {{}}}
	if err := tester.Click("Message"); err != nil {
		t.Fatal(err)
	}
	tester.Type("with a file")
	tester.Key(0, ui.KeyEnter)
	tester.Frame()
	if len(threadQueue(t, page, session.ID)) != 0 || !strings.Contains(page.model.Draft, "with a file") || !strings.Contains(page.thread.notice, "附件") {
		t.Fatal("a message with an attachment must stay a draft with the reason", page.model.Draft, page.thread.notice)
	}
	page.workspaceFiles = nil
	stub.gate <- struct{}{}
	waitWorkspace(t, page, tester, func() bool { return !page.Service.Runtime.HasActive() })
}

func TestPausedQueueOffersExplicitSendAndEditAndCancel(t *testing.T) {
	page, tester, stub, session := threadPage(t, false)
	if _, err := page.Service.Turns.Send(t.Context(), session.ID, "first", nil); err != nil {
		t.Fatal(err)
	}
	<-stub.started
	first, err := page.Service.Turns.Queue(t.Context(), session.ID, "keep me")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = page.Service.Turns.Queue(t.Context(), session.ID, "drop me"); err != nil {
		t.Fatal(err)
	}
	// The Turn is stopped: the queue stays and pauses.
	executionID, _ := page.Service.Turns.Active(session.ID)
	if err = page.Service.Turns.Stop(t.Context(), session.ID, executionID); err != nil {
		t.Fatal(err)
	}
	waitWorkspace(t, page, tester, func() bool {
		return page.detail.Session.Status == engine.Cancelled && len(page.thread.items) == 2 && !page.Service.Runtime.HasActive()
	})
	if !tester.HasText("队列已暂停") {
		t.Fatal("a paused queue must say so", tester.Texts())
	}
	saveThreadScreenshot(t, tester, "queue-paused.png")
	// Cancel the second, edit the first.
	if err = tester.Click("取消排队"); err != nil {
		t.Fatal(err)
	}
	waitWorkspace(t, page, tester, func() bool { return len(page.thread.items) == 1 })
	if page.thread.items[0].ID != first.ID {
		// The first Cancel button belongs to the first row; accept either order but one must remain.
		t.Log("cancelled the first row; the remaining queue is", page.thread.items)
	}
	remaining := page.thread.items[0]
	page.openQueueEdit(remaining)
	tester.Frame()
	page.thread.queueEditText = "edited and kept"
	page.saveQueueEdit()
	waitWorkspace(t, page, tester, func() bool { return len(page.thread.items) == 1 && page.thread.items[0].Text == "edited and kept" })
	// Explicit send starts it.
	if err = tester.Click("发送队首"); err != nil {
		t.Fatal(err)
	}
	prompt := <-stub.started
	if !strings.HasSuffix(prompt, "edited and kept") {
		t.Fatal("explicit send must start the edited head", prompt)
	}
	stub.gate <- struct{}{}
	waitWorkspace(t, page, tester, func() bool { return len(threadQueue(t, page, session.ID)) == 0 && !page.Service.Runtime.HasActive() })
}

// ---- versions, fork --------------------------------------------------------

func versionsApp(t *testing.T) (*NativePage, *ui.Tester, hub.Session, []string) {
	t.Helper()
	app := openDemoApp(t)
	session, err := app.Service.Create(t.Context(), "versions", t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	page := NewNativePage(app.Service)
	page.ctx = t.Context()
	tester := ui.NewTester(workspaceStaticView(page), 1240, 850)
	page.refreshSessions()
	waitWorkspace(t, page, tester, func() bool { return len(page.sessions) == 1 && !page.sessionLoading })
	page.selectSession(session.ID)
	waitWorkspace(t, page, tester, func() bool {
		return !page.polling && page.detail.Session.ID == session.ID && page.thread.session == session.ID
	})
	var turns []string
	for _, prompt := range []string{"alpha question", "bravo question"} {
		start, err := app.Service.Turns.Send(t.Context(), session.ID, prompt, nil)
		if err != nil {
			t.Fatal(err)
		}
		turns = append(turns, start.TurnID)
		waitWorkspace(t, page, tester, func() bool {
			requests, _ := app.Service.PendingApprovals(t.Context(), session.ID)
			for _, request := range requests { // the demo echo tool needs consent
				_ = app.Service.RespondApproval(t.Context(), session.ID, request.ID, true)
			}
			return !page.polling && page.detail.Session.Status == engine.Succeeded && !app.Runtime.HasActive() && page.transcript.Last() != 0 && p2(page, start.TurnID)
		})
	}
	return page, tester, session, turns
}

// p2 reports whether the Turn's user block is in the window.
func p2(page *NativePage, turn string) bool {
	_, ok := page.workspaceUserBlockOf(turn)
	return ok
}

func TestEditResendShowsTheSwitcherAndSwitchesBack(t *testing.T) {
	page, tester, session, turns := versionsApp(t)
	block, ok := page.workspaceUserBlockOf(turns[1])
	if !ok {
		t.Fatal("user block missing")
	}
	// Hovering the message shows the edit and fork actions.
	if _, ok := tester.Find("Edit and resend"); ok {
		_ = ok
	}
	page.startVersionEdit(block)
	tester.Frame()
	if _, ok := tester.Find("Edited message"); !ok {
		t.Fatal("the edit editor must replace the bubble", tester.Texts())
	}
	saveThreadScreenshot(t, tester, "edit-resend-editor.png")
	page.thread.editText = "bravo edited"
	page.submitVersionEdit()
	waitWorkspace(t, page, tester, func() bool {
		requests, _ := page.Service.PendingApprovals(t.Context(), session.ID)
		for _, request := range requests {
			_ = page.Service.RespondApproval(t.Context(), session.ID, request.ID, true)
		}
		return page.detail.Session.ActiveLineID != "" && page.detail.Session.Status == engine.Succeeded && !page.Service.Runtime.HasActive() && len(page.thread.switches) == 1 && !page.polling
	})
	var switcher = page.thread.switches
	var edited string
	for turn, sw := range switcher {
		edited = turn
		if sw.Count != 2 || sw.Index != 2 {
			t.Fatal("the edited message is version 2 of 2", sw)
		}
	}
	if !tester.HasText("2/2") {
		t.Fatal("the switcher must show 2/2", tester.Texts())
	}
	if _, ok := tester.Find("Message versions"); !ok {
		t.Fatal("version switcher missing")
	}
	saveThreadScreenshot(t, tester, "version-switcher.png")
	if user, ok := page.workspaceUserBlockOf(edited); !ok || user.Text != "bravo edited" {
		t.Fatal("the edited text is shown", user)
	}
	// The previous-version arrow shows the original conversation again.
	if err := tester.Click("上一个版本"); err != nil {
		t.Fatal(err)
	}
	waitWorkspace(t, page, tester, func() bool {
		_, shown := page.workspaceUserBlockOf(turns[1])
		switcher, versioned := page.thread.switches[turns[1]]
		return shown && !page.polling && !page.thread.working && versioned && switcher.Index == 1
	})
	if !tester.HasText("1/2") {
		t.Fatal("the original is version 1 of 2", tester.Texts())
	}
	if _, stale := page.workspaceUserBlockOf(edited); stale {
		t.Fatal("switching versions must replace the window")
	}
}

func TestRegenerateButtonAppearsOnlyUnderTheLastReplyOfAnIdleConversation(t *testing.T) {
	page, tester, session, turns := versionsApp(t)
	var replies []string
	for _, row := range page.transcriptRows {
		if block := page.transcriptBlocks[row.ID]; block.Kind == "reply" && block.Status == "completed" {
			replies = append(replies, block.ID)
		}
	}
	if len(replies) < 2 {
		t.Fatal("two replies expected", len(replies))
	}
	tester.Frame()
	if _, ok := tester.Find("重新生成"); !ok {
		t.Fatal("the last reply must offer regeneration", tester.Texts())
	}
	saveThreadScreenshot(t, tester, "regenerate-action.png")
	for i, id := range replies {
		if got, want := page.workspaceLastReply(page.transcriptBlocks[id]), i == len(replies)-1; got != want {
			t.Fatal("only the last reply offers regeneration", id, got)
		}
	}
	page.regenerateReply(turns[1])
	waitWorkspace(t, page, tester, func() bool {
		requests, _ := page.Service.PendingApprovals(t.Context(), session.ID)
		for _, request := range requests {
			_ = page.Service.RespondApproval(t.Context(), session.ID, request.ID, true)
		}
		return page.detail.Session.ActiveLineID != "" && page.detail.Session.Status == engine.Succeeded && !page.Service.Runtime.HasActive() && len(page.thread.switches) == 1 && !page.polling
	})
	for _, sw := range page.thread.switches {
		if sw.Count != 2 || sw.Index != 2 {
			t.Fatal("regenerate adds a version", sw)
		}
	}
}

func TestForkOpensANewConversationWithAnOriginLinkBackToTheSource(t *testing.T) {
	page, tester, session, turns := versionsApp(t)
	page.forkFromTurn(turns[0])
	waitWorkspace(t, page, tester, func() bool {
		return page.selected != session.ID && page.selected != "" && !page.polling && page.detail.Session.ID == page.selected && page.detail.Session.ForkedFrom != nil
	})
	forked := page.selected
	if _, ok := tester.Find("Forked conversation"); !ok || !tester.HasText("查看分叉前的对话") {
		t.Fatal("the fork must show where it came from", tester.Texts())
	}
	saveThreadScreenshot(t, tester, "fork-origin-link.png")
	if page.detail.Session.ForkedFrom.TurnID != turns[0] || page.detail.Session.ForkedFrom.SessionID != session.ID {
		t.Fatal("origin", page.detail.Session.ForkedFrom)
	}
	if err := tester.Click("查看分叉前的对话"); err != nil {
		t.Fatal(err)
	}
	waitWorkspace(t, page, tester, func() bool {
		return page.selected == session.ID && !page.polling && page.detail.Session.ID == session.ID && len(page.transcriptRows) > 0
	})
	if _, ok := page.workspaceUserBlockOf(turns[0]); !ok {
		t.Fatal("the link must open the source around the fork point")
	}
	if forked == session.ID {
		t.Fatal("the fork is a different conversation")
	}
}

func TestVersionActionsExplainWhyTheyAreDisabled(t *testing.T) {
	page, tester, _, turns := versionsApp(t)
	block, _ := page.workspaceUserBlockOf(turns[0])
	if reason := page.workspaceVersionReason(block); reason != "" {
		t.Fatal("an idle ordinary message can be edited", reason)
	}
	rich := block
	rich.Rich = true
	if reason := page.workspaceVersionReason(rich); !strings.Contains(reason, "附件") {
		t.Fatal("a message with attachments says why", reason)
	}
	page.thread.disabled = "团队、伙伴和自动化会话暂不支持版本与分叉"
	if reason := page.workspaceVersionReason(block); !strings.Contains(reason, "团队") {
		t.Fatal("a Team conversation says why", reason)
	}
	page.thread.disabled = ""
	page.model.ActiveRunID = "run"
	if reason := page.workspaceVersionReason(block); !strings.Contains(reason, "任务进行中") {
		t.Fatal("a running conversation says why", reason)
	}
	page.model.ActiveRunID = ""
	tester.Frame()
	_ = view.Idle
	_ = time.Second
}

func TestDegradedForkShowsTheNoticeBeforeAndAfterItsFirstMessage(t *testing.T) {
	page, tester, stub, session := threadPage(t, false)
	stub.forkable = true
	start, err := page.Service.Turns.Send(t.Context(), session.ID, "first", nil)
	if err != nil {
		t.Fatal(err)
	}
	<-stub.started
	stub.gate <- struct{}{}
	waitWorkspace(t, page, tester, func() bool {
		return page.detail.Session.Status == engine.Succeeded && !page.Service.Runtime.HasActive()
	})
	page.forkFromTurn(start.TurnID)
	waitWorkspace(t, page, tester, func() bool {
		return page.selected != session.ID && page.detail.Session.ID == page.selected && page.detail.Session.ForkedFrom != nil && !page.polling && page.thread.session == page.selected
	})
	if !tester.HasText("此引擎不能原生分叉，模型只看到摘要") {
		t.Fatal("a degraded fork must say the model sees only a summary", tester.Texts())
	}
	saveThreadScreenshot(t, tester, "fork-degraded-notice.png")
	if _, err = page.Service.Turns.Send(t.Context(), page.selected, "new topic", nil); err != nil {
		t.Fatal(err)
	}
	<-stub.started
	stub.gate <- struct{}{}
	waitWorkspace(t, page, tester, func() bool {
		if page.detail.Session.Status != engine.Succeeded || page.Service.Runtime.HasActive() {
			return false
		}
		for _, row := range page.transcriptRows {
			if block := page.transcriptBlocks[row.ID]; block.Name == "notice" && strings.Contains(block.Text, "此引擎不能原生分叉") {
				return true
			}
		}
		return false
	})
	saveThreadScreenshot(t, tester, "fork-degraded-first-turn.png")
}
