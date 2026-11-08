// INPUT: MyGo Tester, real product settings and an owned human-question broker
// OUTPUT: Original-view configuration reads/receipts and clarification controls remain separate from execution/approval
// POS: [L7 Hub product] Headless native control acceptance; no claim of OS window or accessibility verification.
//go:build desktop

package hubdesktop

import (
	"context"
	"errors"
	"reflect"
	"strings"
	"sync"
	"sync/atomic"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/extension/interaction"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/host"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/egoist/mygo/ui"
	"testing"
	"time"
)

type settingsLifecycleStore struct {
	product.ServiceStore
	entered, release chan struct{}
	read, fail       atomic.Bool
	once             sync.Once
}

func (s *settingsLifecycleStore) delay(ctx context.Context) error {
	s.once.Do(func() { close(s.entered) })
	select {
	case <-s.release:
	case <-ctx.Done():
		return ctx.Err()
	}
	if s.fail.Load() {
		return errors.New("delayed settings fixture failure")
	}
	return nil
}

// Settings is read by every settings form load and before every save.
func (s *settingsLifecycleStore) Settings(ctx context.Context, id string) (hub.Settings, error) {
	value, err := s.ServiceStore.Settings(ctx, id)
	if s.read.Load() && err == nil {
		err = s.delay(ctx)
	}
	return value, err
}

func (s *settingsLifecycleStore) SetSkillsEnabled(ctx context.Context, id string, value bool) error {
	if err := s.delay(ctx); err != nil {
		return err
	}
	return s.ServiceStore.SetSkillsEnabled(ctx, id, value)
}

func settingsLifecyclePage(t *testing.T) (*NativePage, *settingsLifecycleStore, *ui.Tester) {
	t.Helper()
	s, _ := fixture(t)
	session, err := s.Create(t.Context(), "settings lifecycle", "")
	if err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(s)
	p.selected = session.ID
	p.detail, err = s.Store.Detail(t.Context(), session.ID)
	if err != nil {
		t.Fatal(err)
	}
	p.applyDetail()
	store := &settingsLifecycleStore{ServiceStore: s.Store, entered: make(chan struct{}), release: make(chan struct{})}
	s.Store = store
	t.Cleanup(func() {
		select {
		case <-store.release:
		default:
			close(store.release)
		}
	})
	return p, store, ui.NewTester(workspaceStaticView(p), 1240, 850)
}

func awaitSettingsLifecycle(t *testing.T, store *settingsLifecycleStore) {
	t.Helper()
	select {
	case <-store.entered:
	case <-time.After(3 * time.Second):
		t.Fatal("configuration operation did not reach its actual store")
	}
}

func awaitSettingsForm(t *testing.T, tester *ui.Tester, p *NativePage) {
	t.Helper()
	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		tester.Frame()
		if p.settingsForm.loaded && !p.settingsForm.saving && !p.paneBusy {
			return
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("settings form did not load", p.settingsForm.problem)
}

var settingsLifecyclePanes = []struct {
	name string
	open func(*NativePage)
}{
	{"overview", (*NativePage).openSettings},
	{"mcp", (*NativePage).openMCPSettings},
	{"tools", (*NativePage).openTools},
	{"plugins", (*NativePage).openAEP},
	{"commands", (*NativePage).openCommands},
	{"browser", (*NativePage).openBrowserSettings},
	{"media", (*NativePage).openMediaSettings},
}

func TestNativeSettingsReadCannotPublishAfterLeavingPane(t *testing.T) {
	for _, test := range settingsLifecyclePanes {
		for _, leave := range []string{"different-pane", "reopened-session"} {
			t.Run(test.name+"/"+leave, func(t *testing.T) {
				p, store, tester := settingsLifecyclePage(t)
				store.read.Store(true)
				test.open(p)
				tester.Frame()
				awaitSettingsLifecycle(t, store)
				if leave == "different-pane" {
					p.setPane("routing")
				} else {
					p.generation++ // The same session was left and reopened.
				}
				p.problem = "current view message"
				close(store.release)
				deadline := time.Now().Add(200 * time.Millisecond)
				for time.Now().Before(deadline) {
					tester.Frame()
					time.Sleep(time.Millisecond)
				}
				if leave == "different-pane" && (p.pane != "routing" || p.settingsForm.loaded) || p.problem != "current view message" {
					t.Fatal("old configuration read changed the later view", p.pane, p.problem, p.settingsForm.loaded)
				}
				if leave == "reopened-session" && p.settingsForm.generation != p.generation {
					t.Fatal("reopened session kept a form owned by the previous generation")
				}
			})
		}
	}
}

func TestNativeSettingsReceiptStaysWithOriginalView(t *testing.T) {
	for _, test := range []struct {
		name string
		open func(*NativePage)
		run  func(*NativePage)
		ok   func(hub.Settings) bool
	}{
		{"switch", (*NativePage).openTools, func(p *NativePage) { p.toggleSettingsItem(hub.CapabilityPlanning, "", "planning", true) }, func(v hub.Settings) bool { return v.Tools.Planning }},
		{"memory", (*NativePage).openSettings, func(p *NativePage) { p.toggleSettingsItem(hub.CapabilityMemory, "", "memory", true) }, func(v hub.Settings) bool { return v.PersistentMemory }},
		{"browser", (*NativePage).openBrowserSettings, func(p *NativePage) {
			*p.settingsForm.editor.str("executable") = "/Applications/Fixture"
			p.saveBrowser()
		}, func(v hub.Settings) bool { return v.Browser.Executable == "/Applications/Fixture" }},
		{"mcp", (*NativePage).openMCPSettings, func(p *NativePage) {
			p.editMCP(-1)
			*p.settingsForm.editor.str("id"), *p.settingsForm.editor.str("command") = "fixture", "/bin/echo"
			p.saveMCPServer()
		}, func(v hub.Settings) bool { return len(v.MCP) == 1 && !v.MCP[0].Enabled }},
	} {
		for _, leave := range []string{"different-pane", "reopened-session"} {
			for _, fail := range []bool{false, true} {
				t.Run(test.name+"/"+leave+"/"+map[bool]string{false: "committed", true: "failed"}[fail], func(t *testing.T) {
					p, store, tester := settingsLifecyclePage(t)
					test.open(p)
					awaitSettingsForm(t, tester, p)
					store.read.Store(true)
					store.fail.Store(fail)
					test.run(p)
					awaitSettingsLifecycle(t, store)
					if leave == "different-pane" {
						p.setPane("routing")
					} else {
						p.generation++
					}
					pane := p.pane
					p.problem = "current view message"
					close(store.release)
					settlePage(t, tester, p)
					if p.pane != pane || p.problem != "current view message" || leave == "different-pane" && p.settingsForm.notice != "" {
						t.Fatal("old configuration receipt changed the later view", p.pane, p.problem)
					}
					store.read.Store(false)
					value, err := p.Service.EffectiveSettings(t.Context(), p.selected)
					if err != nil || test.ok(value) == fail {
						t.Fatal("ignoring a stale UI receipt changed its actual committed fact", err, p.settingsForm.problem, p.settingsForm.errors)
					}
					if p.Service.Runtime.HasActive() {
						t.Fatal("configuration receipt started execution")
					}
				})
			}
		}
	}
}

func TestNativeOptionalToolsAndHumanClarification(t *testing.T) {
	s, _ := fixture(t)
	ctx := context.Background()
	product, err := s.Create(ctx, "tools", "")
	if err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(s)
	p.selected = product.ID
	p.detail, err = s.Store.Detail(ctx, product.ID)
	if err != nil {
		t.Fatal(err)
	}
	p.applyDetail()
	tester := ui.NewTester(workspaceStaticView(p), 1400, 1600)
	if err = workspaceTestMenu(tester, "会话设置"); err != nil {
		t.Fatal(err)
	}
	// The settings entries render only once the form has loaded, which
	// settlePage (pane work only) does not wait for.
	awaitSettingsForm(t, tester, p)
	if err = tester.Click("Open 工具"); err != nil {
		t.Fatal(err)
	}
	awaitSettingsForm(t, tester, p)
	if err = tester.Click("Toggle interaction"); err != nil {
		t.Fatal(err)
	}
	awaitSettingsForm(t, tester, p)
	if effective, err := s.EffectiveSettings(ctx, product.ID); err != nil || !effective.Tools.Interaction || s.Runtime.HasActive() {
		t.Fatal("opt-in did not persist cold", err, p.settingsForm.problem)
	}
	broker, err := host.NewQuestionBroker(8)
	if err != nil {
		t.Fatal(err)
	}
	defer broker.Close()
	s.Questions = broker
	native := "question-native"
	if _, err = s.Repository.Create(ctx, product.ID, native, ""); err != nil {
		t.Fatal(err)
	}
	if err = s.Store.BindNative(ctx, product.ID, p.detail.Session.ActiveBranchID, native); err != nil {
		t.Fatal(err)
	}
	if _, err = s.Store.BeginTurn(ctx, product.ID, "actual question owner"); err != nil {
		t.Fatal(err)
	}
	q := interaction.Question{ID: "question-fixture", SessionID: native, RunID: "run-fixture", CallID: "call-fixture", Prompt: "Pick one real answer", Options: []interaction.Choice{{Label: "First choice"}, {Label: "Second choice"}}}
	answered := make(chan interaction.Answer, 1)
	wait, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	go func() { a, _ := broker.Ask(wait, q); answered <- a }()
	deadline := time.Now().Add(time.Second)
	for len(broker.PendingQuestionsSnapshot()) == 0 && time.Now().Before(deadline) {
		time.Sleep(time.Millisecond)
	}
	p.questions, err = s.PendingQuestions(ctx, product.ID)
	if err != nil || len(p.questions) != 1 {
		t.Fatal("question not product-owned", err)
	}
	tester.Frame()
	if err = tester.Click("2. Second choice"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, p)
	select {
	case a := <-answered:
		if len(a.Selected) != 1 || a.Selected[0] != 2 {
			t.Fatal("actual broker answer lost", a)
		}
	case <-wait.Done():
		t.Fatal("UI never answered actual question")
	}
	approvals, err := s.PendingApprovals(ctx, product.ID)
	if err != nil || len(approvals) != 0 {
		t.Fatal("clarification created permission authority")
	}
}

type nativeQuestionReadStore struct {
	product.ServiceStore
	calls  atomic.Int32
	failAt int32
}

func (s *nativeQuestionReadStore) Detail(ctx context.Context, id string) (hub.Detail, error) {
	if s.calls.Add(1) == s.failAt {
		return hub.Detail{}, errors.New("independent interaction read failed")
	}
	return s.ServiceStore.Detail(ctx, id)
}

func nativeQuestionPage(t *testing.T, kind string) (*NativePage, *host.QuestionBroker, interaction.Question, <-chan interaction.Answer) {
	t.Helper()
	s, _ := fixture(t)
	session, err := s.Create(t.Context(), "independent questions", "")
	if err != nil {
		t.Fatal(err)
	}
	native := "native-clarification"
	if _, err = s.Repository.Create(t.Context(), session.ID, native, ""); err != nil {
		t.Fatal(err)
	}
	detail, err := s.Store.Detail(t.Context(), session.ID)
	if err != nil {
		t.Fatal(err)
	}
	if err = s.Store.BindNative(t.Context(), session.ID, detail.Session.ActiveBranchID, native); err != nil {
		t.Fatal(err)
	}
	if _, err = s.Store.BeginTurn(t.Context(), session.ID, "explicit clarification fixture"); err != nil {
		t.Fatal(err)
	}
	broker, err := host.NewQuestionBroker(8)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(broker.Close)
	s.Questions = broker
	q := interaction.Question{ID: "pending-question", SessionID: native, RunID: "run", CallID: "question-call", Header: "Question fixture", Prompt: "Choose explicitly", Options: []interaction.Choice{{Label: "First"}, {Label: "Second"}}}
	if kind == "multiple" {
		q.MultiSelect = true
	} else if kind == "text" {
		q.Options = nil
	}
	answered := make(chan interaction.Answer, 1)
	ctx, cancel := context.WithCancel(t.Context())
	t.Cleanup(cancel)
	go func() {
		a, _ := broker.Ask(ctx, q)
		answered <- a
	}()
	deadline := time.Now().Add(time.Second)
	for len(broker.PendingQuestionsSnapshot()) == 0 && time.Now().Before(deadline) {
		time.Sleep(time.Millisecond)
	}
	if len(broker.PendingQuestionsSnapshot()) != 1 {
		t.Fatal("actual question did not become pending")
	}
	p := NewNativePage(s)
	p.selected = session.ID
	p.detail, err = s.Store.Detail(t.Context(), session.ID)
	if err != nil {
		t.Fatal(err)
	}
	p.applyDetail()
	// The pinned panel is the entry outside the conversation.
	p.pane = "waits"
	p.questions = []interaction.Question{q}
	return p, broker, q, answered
}

func TestNativeQuestionAndApprovalReadFailuresStayIndependent(t *testing.T) {
	for _, failAt := range []int32{2, 3} {
		t.Run(map[int32]string{2: "approval", 3: "question"}[failAt], func(t *testing.T) {
			p, _, q, _ := nativeQuestionPage(t, "single")
			approvals := host.NewApprovalBroker(8)
			t.Cleanup(func() { _ = approvals.Close() })
			p.Service.Approvals = approvals
			go func() {
				_, _ = approvals.Approve(t.Context(), security.ApprovalRequest{SessionID: q.SessionID, RunID: q.RunID, CallID: "separate-approval", ToolName: "write_file"})
			}()
			deadline := time.Now().Add(time.Second)
			for len(approvals.PendingSnapshot()) == 0 && time.Now().Before(deadline) {
				time.Sleep(time.Millisecond)
			}
			p.approvals = approvals.PendingSnapshot()
			if len(p.approvals) != 1 {
				t.Fatal("actual permission did not become pending")
			}
			p.detail.Session.Status = engine.Interrupted // A stale view must still refresh.
			wrapped := &nativeQuestionReadStore{ServiceStore: p.Service.Store, failAt: failAt}
			p.Service.Store = wrapped
			tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
			p.poll()
			for p.polling && time.Now().Before(deadline) {
				tester.Frame()
				time.Sleep(time.Millisecond)
			}
			if p.polling || p.detail.Session.Status != hub.Running {
				t.Fatal("one interaction error blocked actual execution status", p.problem)
			}
			if failAt == 2 && (len(p.approvals) != 0 || len(p.questions) != 1) || failAt == 3 && (len(p.questions) != 0 || len(p.approvals) != 1) {
				t.Fatal("failed chain retained clickable data or hid the independent chain", p.approvals, p.questions)
			}
			if p.approvalReadProblem == "" && p.questionReadProblem == "" || p.Service.Runtime.HasActive() {
				t.Fatal("read failure was hidden or observation executed an engine")
			}
		})
	}
}

func TestNativeQuestionDoubleClickSubmitsOnce(t *testing.T) {
	p, _, _, answered := nativeQuestionPage(t, "single")
	wrapped := &workspaceDelayedDetail{ServiceStore: p.Service.Store, entered: make(chan struct{}), release: make(chan struct{})}
	p.Service.Store = wrapped
	wrapped.block.Store(true)
	var release sync.Once
	defer release.Do(func() { close(wrapped.release) })
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	if err := tester.Click("2. Second"); err != nil {
		t.Fatal(err)
	}
	select {
	case <-wrapped.entered:
	case <-time.After(time.Second):
		t.Fatal("question submission never reached its actual owner")
	}
	if err := tester.Click("2. Second"); err != nil {
		t.Fatal(err)
	}
	// The second click must not answer while the first owner is still validating.
	select {
	case <-answered:
		t.Fatal("duplicate click submitted a second answer before the first returned")
	case <-time.After(20 * time.Millisecond):
	}
	release.Do(func() { close(wrapped.release) })
	waitWorkspace(t, p, tester, func() bool { return len(p.questionSubmitting) == 0 })
	select {
	case a := <-answered:
		if !reflect.DeepEqual(a.Selected, []int{2}) || len(p.questions) != 0 || p.problem != "" {
			t.Fatal("successful answer was duplicated or restored", a, p.problem)
		}
	case <-time.After(time.Second):
		t.Fatal("first explicit answer never reached the broker")
	}
}

func clickNativeQuestion(t *testing.T, tester *ui.Tester, label string) {
	t.Helper()
	for i := 0; i < 8; i++ {
		box, found := tester.Find(label)
		viewport, ok := tester.Find("Human clarification")
		if !found || !ok {
			t.Fatal("question control or viewport missing", label)
		}
		dy := float32(0)
		if box.Y < viewport.Y {
			dy = box.Y - viewport.Y - 8
		} else if box.Y+box.H > viewport.Y+viewport.H {
			dy = box.Y + box.H - viewport.Y - viewport.H + 8
		} else {
			if box.Y < 0 || box.Y+box.H > 850 {
				t.Fatal("question control outside the default viewport", label, box)
			}
			if err := tester.Click(label); err != nil {
				t.Fatal(err)
			}
			return
		}
		tester.Scroll(viewport.X+viewport.W-12, viewport.Y+20, 0, dy)
	}
	t.Fatal("question control could not be reached by scrolling", label)
}

func TestNativeQuestionAnswerFormatsUseActualControls(t *testing.T) {
	for _, kind := range []string{"single", "multiple", "other", "text"} {
		t.Run(kind, func(t *testing.T) {
			p, broker, _, answered := nativeQuestionPage(t, kind)
			tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
			var expected interaction.Answer
			switch kind {
			case "single":
				expected.Selected = []int{2}
				clickNativeQuestion(t, tester, "2. Second")
			case "multiple":
				clickNativeQuestion(t, tester, "2. Second")
				clickNativeQuestion(t, tester, "1. First")
				clickNativeQuestion(t, tester, "✓ 2. Second")
				if len(broker.PendingQuestionsSnapshot()) != 1 {
					t.Fatal("selecting choices implicitly submitted")
				}
				expected.Selected = []int{1}
				clickNativeQuestion(t, tester, "Send selected choices")
			default:
				text := "我的其他答案，不授予文件或网络权限"
				expected.Text = &text
				clickNativeQuestion(t, tester, "Clarification answer")
				tester.Type(text)
				if len(broker.PendingQuestionsSnapshot()) != 1 {
					t.Fatal("editing an answer implicitly submitted")
				}
				clickNativeQuestion(t, tester, "Send clarification answer")
			}
			waitWorkspace(t, p, tester, func() bool { return len(p.questionSubmitting) == 0 })
			select {
			case actual := <-answered:
				if !reflect.DeepEqual(actual, expected) || p.Service.Runtime.HasActive() {
					t.Fatal("actual answer changed or started execution", actual, expected)
				}
			case <-time.After(time.Second):
				t.Fatal("explicit answer never reached the actual question broker", p.problem)
			}
		})
	}
}

func TestNativeQuestionRejectedAnswerKeepsDraftForExplicitCorrection(t *testing.T) {
	p, broker, _, answered := nativeQuestionPage(t, "text")
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	p.questionText = strings.Repeat("x", (64<<10)+1)
	tester.Frame()
	clickNativeQuestion(t, tester, "Send clarification answer")
	waitWorkspace(t, p, tester, func() bool { return len(p.questionSubmitting) == 0 })
	if len(p.questionText) != (64<<10)+1 || p.problem == "" || len(broker.PendingQuestionsSnapshot()) != 1 {
		t.Fatal("rejected answer lost its draft or consumed the pending question", p.problem)
	}
	select {
	case <-answered:
		t.Fatal("invalid answer reached the execution owner")
	default:
	}
	clickNativeQuestion(t, tester, "Clarification answer")
	tester.Key(ui.Cmd, ui.KeyA)
	tester.Type("corrected explicit answer")
	clickNativeQuestion(t, tester, "Send clarification answer")
	waitWorkspace(t, p, tester, func() bool { return len(p.questionSubmitting) == 0 })
	select {
	case a := <-answered:
		if a.Text == nil || *a.Text != "corrected explicit answer" || p.problem != "" {
			t.Fatal("explicit correction did not reach the owner", a, p.problem)
		}
	case <-time.After(time.Second):
		t.Fatal("corrected answer never reached the broker")
	}
}

func TestNativeQuestionLateReceiptCannotChangeReopenedView(t *testing.T) {
	p, _, _, _ := nativeQuestionPage(t, "text")
	wrapped := &workspaceDelayedDetail{ServiceStore: p.Service.Store, entered: make(chan struct{}), release: make(chan struct{})}
	p.Service.Store = wrapped
	wrapped.block.Store(true)
	var release sync.Once
	defer release.Do(func() { close(wrapped.release) })
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	p.questionText = strings.Repeat("x", (64<<10)+1)
	clickNativeQuestion(t, tester, "Send clarification answer")
	select {
	case <-wrapped.entered:
	case <-time.After(time.Second):
		t.Fatal("answer never reached product validation")
	}
	p.generation++ // The same session has been left and reopened.
	p.problem = "current view message"
	p.questionText = "new view draft"
	release.Do(func() { close(wrapped.release) })
	waitWorkspace(t, p, tester, func() bool { return len(p.questionSubmitting) == 0 })
	if p.problem != "current view message" || p.questionText != "new view draft" {
		t.Fatal("old answer receipt changed the new view", p.problem, p.questionText)
	}
}
