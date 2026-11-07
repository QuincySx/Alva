// INPUT: Real product stores, a local bidirectional engine fixture and MyGo controls
// OUTPUT: Native callback panels, explicit multi-question answers, masking and late receipt isolation regressions
// POS: [L7 native interaction tests] Uses no installed CLI, credentials, external model or OS window
//go:build desktop

package hubdesktop

import (
	"context"
	"errors"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/egoist/mygo/ui"
)

type workspaceInteractionEngine struct {
	ready           chan struct{}
	resolved        chan string
	answers         chan engine.UserInputResponse
	permissionCalls atomic.Int32
	questionCalls   atomic.Int32
	rejectAnswer    atomic.Bool
}

type workspaceEngineInteractionReadFailure struct{ product.ServiceStore }

func (s workspaceEngineInteractionReadFailure) Detail(context.Context, string) (hub.Detail, error) {
	return hub.Detail{}, errors.New("fixture native request read failure")
}

func (e *workspaceInteractionEngine) Execute(ctx context.Context, _ engine.Request, emit func(engine.Event) error) error {
	for _, event := range []engine.Event{
		{Kind: engine.SessionStarted, SessionID: "native-ui"},
		{Kind: engine.PermissionRequest, SessionID: "native-ui", ID: "permission", Name: "fileChange", Input: []byte(`{"path":"fixture.txt"}`)},
		{Kind: engine.UserInputRequested, SessionID: "native-ui", ID: "question", Name: "requestUserInput", Question: &engine.UserInputRequest{Questions: []engine.UserInputQuestion{
			{ID: "color", Header: "Color", Question: "Choose a color", IsOther: true, Options: []engine.UserInputOption{{Label: "Blue"}, {Label: "Green"}}},
			{ID: "secret", Header: "Secret", Question: "Enter a private answer", IsSecret: true},
		}}},
	} {
		if err := emit(event); err != nil {
			return err
		}
	}
	close(e.ready)
	for range 2 {
		select {
		case kind := <-e.resolved:
			id := "permission"
			if kind == "user_input" {
				id = "question"
			}
			if err := emit(engine.Event{Kind: engine.InteractionResolved, SessionID: "native-ui", ID: id, Name: kind}); err != nil {
				return err
			}
		case <-ctx.Done():
			return ctx.Err()
		}
	}
	return emit(engine.Event{Kind: engine.Completed, SessionID: "native-ui", Outcome: engine.Succeeded})
}
func (*workspaceInteractionEngine) Cancel(context.Context, string) error { return nil }
func (e *workspaceInteractionEngine) RespondPermission(context.Context, string, string, engine.PermissionDecision) error {
	e.permissionCalls.Add(1)
	e.resolved <- "permission"
	return nil
}
func (e *workspaceInteractionEngine) RespondUserInput(_ context.Context, _, _ string, response engine.UserInputResponse) error {
	if e.rejectAnswer.Swap(false) {
		return errors.New("fixture answer was rejected")
	}
	e.questionCalls.Add(1)
	e.answers <- response
	e.resolved <- "user_input"
	return nil
}
func (*workspaceInteractionEngine) Capabilities() engine.Capabilities {
	return engine.Capabilities{Streaming: true, Resume: true, Cancel: true, PermissionCallback: true, UserInputCallback: true}
}

func workspaceInteractionPage(t *testing.T) (*NativePage, *workspaceInteractionEngine) {
	t.Helper()
	s, _ := fixture(t)
	owner, err := s.Create(t.Context(), "native UI fixture", "")
	if err != nil {
		t.Fatal(err)
	}
	target := s.DefaultTarget
	target.EngineID = "native-ui-fixture"
	s.Targets = append(s.Targets, target)
	if _, err := s.Store.Switch(t.Context(), owner.ID, target, "local callback fixture"); err != nil {
		t.Fatal(err)
	}
	e := &workspaceInteractionEngine{ready: make(chan struct{}), resolved: make(chan string, 2), answers: make(chan engine.UserInputResponse, 1)}
	if err := s.Runtime.Register(target.EngineID, e); err != nil {
		t.Fatal(err)
	}
	if _, _, err := s.Runtime.Start(t.Context(), owner.ID, "local fixture"); err != nil {
		t.Fatal(err)
	}
	select {
	case <-e.ready:
	case <-time.After(time.Second):
		t.Fatal("no native interactions")
	}
	p := NewNativePage(s)
	p.selected = owner.ID
	p.detail, err = s.Store.Detail(t.Context(), owner.ID)
	if err != nil {
		t.Fatal(err)
	}
	p.applyDetail()
	items, err := s.PendingEngineInteractions(t.Context(), owner.ID)
	if err != nil || len(items) != 2 {
		t.Fatal(items, err)
	}
	v := p.prepareEngineInteractionScope()
	v.items = items
	v.current = true
	return p, e
}

func clickEngineQuestion(t *testing.T, tester *ui.Tester, label string) {
	t.Helper()
	for range 10 {
		box, found := tester.Find(label)
		viewport, ok := tester.Find("Native human questions")
		if !found || !ok {
			t.Fatal("missing native question control", label)
		}
		dy := float32(0)
		if box.Y < viewport.Y {
			dy = box.Y - viewport.Y - 8
		} else if box.Y+box.H > viewport.Y+viewport.H {
			dy = box.Y + box.H - viewport.Y - viewport.H + 8
		} else {
			if err := tester.Click(label); err != nil {
				t.Fatal(err)
			}
			return
		}
		tester.Scroll(viewport.X+viewport.W-12, viewport.Y+16, 0, dy)
	}
	t.Fatal("native question control could not be reached", label)
}

func TestWorkspaceNativeQuestionsAndPermissionUseSharedActualControls(t *testing.T) {
	p, e := workspaceInteractionPage(t)
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	for _, pane := range []string{"tools", "mcp", "inspector"} {
		p.pane = pane
		tester.Frame()
		if _, found := tester.Find("Allow native once"); !found {
			t.Fatal("native approval hidden in pane", pane)
		}
		if !tester.HasText("Native questions · native-ui-fixture") {
			t.Fatal("native question hidden in pane", pane)
		}
	}
	p.pane = "tools"
	tester.Frame()
	if err := tester.Click("Deny native once"); err != nil {
		t.Fatal(err)
	}
	waitWorkspace(t, p, tester, func() bool { return len(p.engineInteractions.submitting) == 0 && e.permissionCalls.Load() == 1 })
	if e.questionCalls.Load() != 0 {
		t.Fatal("denial answered human questions")
	}
	clickEngineQuestion(t, tester, "Blue")
	if e.questionCalls.Load() != 0 {
		t.Fatal("choosing a label submitted an incomplete question group")
	}
	clickEngineQuestion(t, tester, "Native answer · Secret")
	tester.Type("private-human-answer")
	if tester.HasText("private-human-answer") {
		t.Fatal("secret answer was exposed as visible UI text")
	}
	clickEngineQuestion(t, tester, "Send native answers")
	waitWorkspace(t, p, tester, func() bool { return len(p.engineInteractions.submitting) == 0 && !p.Service.Runtime.HasActive() })
	select {
	case answer := <-e.answers:
		if len(answer.Answers) != 2 || answer.Answers["color"][0] != "Blue" || answer.Answers["secret"][0] != "private-human-answer" {
			t.Fatal("human answers changed", answer)
		}
	case <-time.After(time.Second):
		t.Fatal("native answers not sent")
	}
	if e.permissionCalls.Load() != 1 || e.questionCalls.Load() != 1 {
		t.Fatal("native controls duplicated or crossed semantics")
	}
}

func TestWorkspaceNativeQuestionsCanBeExplicitlySkipped(t *testing.T) {
	p, e := workspaceInteractionPage(t)
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	clickEngineQuestion(t, tester, "Skip native questions")
	waitWorkspace(t, p, tester, func() bool { return e.questionCalls.Load() == 1 && len(p.engineInteractions.submitting) == 0 })
	answer := <-e.answers
	if len(answer.Answers) != 0 || e.permissionCalls.Load() != 0 {
		t.Fatal("skip granted permission or invented answers", answer)
	}
}

func TestWorkspaceNativeInteractionBusyLateReceiptAndReadErrorKeepView(t *testing.T) {
	p, e := workspaceInteractionPage(t)
	wrapped := &workspaceDelayedDetail{ServiceStore: p.Service.Store, entered: make(chan struct{}), release: make(chan struct{})}
	p.Service.Store = wrapped
	wrapped.block.Store(true)
	var release sync.Once
	defer release.Do(func() { close(wrapped.release) })
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	if err := tester.Click("Allow native once"); err != nil {
		t.Fatal(err)
	}
	select {
	case <-wrapped.entered:
	case <-time.After(time.Second):
		t.Fatal("response did not validate product binding")
	}
	if err := tester.Click("Allow native once"); err != nil {
		t.Fatal(err)
	}
	if e.permissionCalls.Load() != 0 {
		t.Fatal("busy duplicate reached engine")
	}
	p.generation++
	v := p.prepareEngineInteractionScope()
	v.problem = "new view notice"
	v.current = false
	release.Do(func() { close(wrapped.release) })
	deadline := time.Now().Add(time.Second)
	for time.Now().Before(deadline) && e.permissionCalls.Load() == 0 {
		tester.Frame()
		time.Sleep(time.Millisecond)
	}
	for range 3 {
		tester.Frame()
	}
	if e.permissionCalls.Load() != 1 || p.engineInteractions.problem != "new view notice" || p.engineInteractions.current {
		t.Fatal("late response replaced new view", e.permissionCalls.Load(), p.engineInteractions.problem)
	}
	// A failed refresh retains current drafts and disables stale response controls.
	items, err := p.Service.PendingEngineInteractions(t.Context(), p.selected)
	if err != nil {
		t.Fatal(err)
	}
	v = p.prepareEngineInteractionScope()
	v.items = items
	v.current = true
	var question product.EngineInteraction
	for _, item := range items {
		if item.Event.Kind == engine.UserInputRequested {
			question = item
		}
	}
	draft := p.engineQuestionDraft(question)
	text := "preserved secret draft"
	draft.text["secret"] = &text
	p.Service.Store = workspaceEngineInteractionReadFailure{p.Service.Store}
	p.pollEngineInteractions()
	deadline = time.Now().Add(time.Second)
	for time.Now().Before(deadline) && !strings.Contains(p.engineInteractions.problem, "fixture native request read failure") {
		tester.Frame()
		time.Sleep(time.Millisecond)
	}
	if !tester.HasText("fixture native request read failure") || p.engineInteractions.current || len(p.engineInteractions.items) != len(items) || *p.engineInteractions.drafts[question.Handle.ID()].text["secret"] != "preserved secret draft" {
		t.Fatal("read failure was hidden")
	}
	if err := tester.Click("Skip native questions"); err != nil {
		t.Fatal(err)
	}
	if e.questionCalls.Load() != 0 {
		t.Fatal("stale view submitted a response")
	}
}

func TestWorkspaceNativeOtherAnswerRejectsBlankAndRetainsFailedDraft(t *testing.T) {
	p, e := workspaceInteractionPage(t)
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	clickEngineQuestion(t, tester, "Blue")
	clickEngineQuestion(t, tester, "Other · Color")
	clickEngineQuestion(t, tester, "Native answer · Color")
	tester.Type("   ")
	clickEngineQuestion(t, tester, "Native answer · Secret")
	tester.Type("private-other-answer")
	clickEngineQuestion(t, tester, "Send native answers")
	if e.questionCalls.Load() != 0 || len(p.engineInteractions.submitting) != 0 {
		t.Fatal("blank Other submitted a previous offered choice")
	}
	clickEngineQuestion(t, tester, "Native answer · Color")
	tester.Key(ui.Cmd, ui.KeyA)
	tester.Type("A user supplied alternative")
	e.rejectAnswer.Store(true)
	clickEngineQuestion(t, tester, "Send native answers")
	waitWorkspace(t, p, tester, func() bool {
		return len(p.engineInteractions.submitting) == 0 && strings.Contains(p.engineInteractions.problem, "fixture answer was rejected")
	})
	if e.questionCalls.Load() != 0 || e.permissionCalls.Load() != 0 || tester.HasText("private-other-answer") {
		t.Fatal("failed clarification granted permission, consumed the answer or exposed its secret")
	}
	clickEngineQuestion(t, tester, "Send native answers")
	waitWorkspace(t, p, tester, func() bool { return len(p.engineInteractions.submitting) == 0 && e.questionCalls.Load() == 1 })
	answer := <-e.answers
	if answer.Answers["color"][0] != "A user supplied alternative" || answer.Answers["secret"][0] != "private-other-answer" || e.permissionCalls.Load() != 0 {
		t.Fatal("retry lost the explicit Other or secret draft", answer)
	}
}

func TestWorkspaceNativeLongPermissionReviewScrollsWithoutMovingActions(t *testing.T) {
	p, e := workspaceInteractionPage(t)
	for i := range p.engineInteractions.items {
		if p.engineInteractions.items[i].Event.Kind == engine.PermissionRequest {
			p.engineInteractions.items[i].Event.Text = strings.Repeat("Review this native requested change before deciding.\n", 40)
		}
	}
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	before, ok := tester.Find("Allow native once")
	viewport, exists := tester.Find("Native permission request details")
	input, found := tester.Find("Native requested input")
	if !ok || !exists || !found || before.Y+before.H > 850 || input.Y < viewport.Y+viewport.H {
		t.Fatal("native long permission has no bounded review with reachable consent", before, viewport, input)
	}
	tester.Scroll(viewport.X+viewport.W-12, viewport.Y+16, 0, 10000)
	input, found = tester.Find("Native requested input")
	after, present := tester.Find("Allow native once")
	if !found || input.Y < viewport.Y || input.Y+input.H > viewport.Y+viewport.H || !present || before != after || e.permissionCalls.Load() != 0 || e.questionCalls.Load() != 0 {
		t.Fatal("native request could not be reviewed independently from its actions", viewport, input, before, after)
	}
}
