// INPUT: Real private product/native stores, host approval broker and MyGo controls with a local engine fixture.
// OUTPUT: One-call consent/denial, independent clarification and bound asynchronous receipt regressions.
// POS: [L7 native approval tests] Exercises the actual Runtime permission owner without external processes or accounts.
//go:build desktop

package hubdesktop

import (
	"context"
	"errors"
	"fmt"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/extension/interaction"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/host"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/egoist/mygo/ui"
)

type nativeApprovalEngine struct {
	service *product.HubService
	native  string
	decided chan security.ApprovalDecision
	calls   atomic.Int32
}

func (e *nativeApprovalEngine) Execute(ctx context.Context, _ engine.Request, emit func(engine.Event) error) error {
	if err := emit(engine.Event{Kind: engine.SessionStarted, SessionID: e.native}); err != nil {
		return err
	}
	decision, err := e.service.Approvals.Approve(ctx, security.ApprovalRequest{SessionID: e.native, RunID: "approval-run", CallID: "approval-call", ToolName: "write_file", InputSummary: "Explicit local fixture; no file is written"})
	if err != nil {
		return err
	}
	e.decided <- decision
	return emit(engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded})
}
func (*nativeApprovalEngine) Cancel(context.Context, string) error { return nil }
func (e *nativeApprovalEngine) RespondPermission(ctx context.Context, root, request string, decision engine.PermissionDecision) error {
	e.calls.Add(1)
	return e.service.RespondNativeApproval(ctx, "alva", root, request, decision.Allow)
}
func (*nativeApprovalEngine) Capabilities() engine.Capabilities {
	return engine.Capabilities{Streaming: true, Cancel: true, PermissionCallback: true}
}

func nativeApprovalPage(t *testing.T) (*NativePage, *nativeApprovalEngine) {
	t.Helper()
	s, _ := fixture(t)
	s.Approvals = host.NewApprovalBroker(8)
	t.Cleanup(func() { _ = s.Approvals.Close() })
	session, err := s.Create(t.Context(), "actual permission owner", "")
	if err != nil {
		t.Fatal(err)
	}
	e := &nativeApprovalEngine{service: s, native: "native-permission-owner", decided: make(chan security.ApprovalDecision, 1)}
	if _, err = s.Repository.Create(t.Context(), session.ID, e.native, ""); err != nil {
		t.Fatal(err)
	}
	if err = s.Runtime.Register(s.DefaultTarget.EngineID, e); err != nil {
		t.Fatal(err)
	}
	if _, err = s.Submit(t.Context(), session.ID, "local permission fixture", nil); err != nil {
		t.Fatal(err)
	}
	deadline := time.Now().Add(time.Second)
	for len(s.Approvals.PendingSnapshot()) == 0 && time.Now().Before(deadline) {
		time.Sleep(time.Millisecond)
	}
	p := NewNativePage(s)
	p.selected = session.ID
	p.detail, err = s.Store.Detail(t.Context(), session.ID)
	if err != nil {
		t.Fatal(err)
	}
	p.applyDetail()
	// The pinned panel is the entry outside the conversation; inside it the
	// same request is a card in the stream (workspace_interactions tests).
	p.pane = "waits"
	p.approvals, err = s.PendingApprovals(t.Context(), session.ID)
	if err != nil || len(p.approvals) != 1 {
		t.Fatal("actual permission never became visible", err)
	}
	return p, e
}

func TestNativeApprovalDoubleClickSendsOneDecision(t *testing.T) {
	for _, allow := range []bool{true, false} {
		label := map[bool]string{true: "Allow once", false: "Deny once"}[allow]
		t.Run(label, func(t *testing.T) {
			p, owner := nativeApprovalPage(t)
			wrapped := &workspaceDelayedDetail{ServiceStore: p.Service.Store, entered: make(chan struct{}), release: make(chan struct{})}
			p.Service.Store = wrapped
			wrapped.block.Store(true)
			var release sync.Once
			defer release.Do(func() { close(wrapped.release) })
			tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
			before, ok := tester.Find(label)
			if box := before; !ok || box.H != 32 || box.Y < 0 || box.Y+box.H > 850 {
				t.Fatal("permission button outside the actual viewport", box)
			}
			if err := tester.Click(label); err != nil {
				t.Fatal(err)
			}
			select {
			case <-wrapped.entered:
			case <-time.After(time.Second):
				t.Fatal("permission did not reach product validation")
			}
			if after, present := tester.Find(label); !present || before != after || !tester.Focused(label) {
				t.Fatal("in-flight consent lost its geometry, name or focus", before, after)
			}
			if _, found := tester.Find("Loading indicator"); !found {
				t.Fatal("actual consent dispatch did not show shared Loading")
			}
			if err := tester.Click(label); err != nil {
				t.Fatal(err)
			}
			select {
			case <-owner.decided:
				t.Fatal("duplicate click decided permission while its first owner was still validating")
			case <-time.After(20 * time.Millisecond):
			}
			release.Do(func() { close(wrapped.release) })
			waitWorkspace(t, p, tester, func() bool { return len(p.approvalSubmitting) == 0 && !p.Service.Runtime.HasActive() })
			select {
			case decision := <-owner.decided:
				if decision.Allowed != allow || decision.Scope != security.Once || owner.calls.Load() != 1 || len(p.approvals) != 0 || p.problem != "" {
					t.Fatal("permission decision was changed or repeated", decision, owner.calls.Load(), p.problem)
				}
			case <-time.After(time.Second):
				t.Fatal("explicit permission never reached the execution owner")
			}
		})
	}
}

func TestNativeApprovalAndQuestionRejectEachOthersIdentity(t *testing.T) {
	p, owner := nativeApprovalPage(t)
	broker, err := host.NewQuestionBroker(8)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(broker.Close)
	p.Service.Questions = broker
	q := interaction.Question{ID: "separate-question", SessionID: owner.native, RunID: "question-run", CallID: "question-call", Prompt: "A clarification cannot grant permission"}
	answer := make(chan interaction.Answer, 1)
	go func() { a, _ := broker.Ask(t.Context(), q); answer <- a }()
	deadline := time.Now().Add(time.Second)
	for len(broker.PendingQuestionsSnapshot()) == 0 && time.Now().Before(deadline) {
		time.Sleep(time.Millisecond)
	}
	text := "clarification only"
	if err = p.Service.RespondQuestion(t.Context(), p.selected, p.approvals[0].ID, interaction.Answer{Text: &text}); !errors.Is(err, host.ErrQuestionExpired) {
		t.Fatal("permission identity was treated as a question", err)
	}
	if err = p.Service.RespondApproval(t.Context(), p.selected, q.ID, true); !errors.Is(err, host.ErrApprovalExpired) {
		t.Fatal("question identity was treated as permission", err)
	}
	p.questions, err = p.Service.PendingQuestions(t.Context(), p.selected)
	if err != nil || len(p.questions) != 1 {
		t.Fatal("rejected identities consumed an independent question", err)
	}
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	clickNativeQuestion(t, tester, "Clarification answer")
	tester.Type(text)
	clickNativeQuestion(t, tester, "Send clarification answer")
	waitWorkspace(t, p, tester, func() bool { return len(p.questionSubmitting) == 0 })
	select {
	case a := <-answer:
		if a.Text == nil || *a.Text != text || len(p.Service.Approvals.PendingSnapshot()) != 1 || !p.Service.Runtime.SessionActive(p.selected) {
			t.Fatal("clarification decided the separate permission", a)
		}
	case <-time.After(time.Second):
		t.Fatal("clarification was not delivered")
	}
	if err := tester.Click("Deny once"); err != nil {
		t.Fatal(err)
	}
	waitWorkspace(t, p, tester, func() bool { return !p.Service.Runtime.HasActive() })
	if decision := <-owner.decided; decision.Allowed || decision.Scope != security.Once {
		t.Fatal("explicit denial changed", decision)
	}
}

func TestNativeApprovalLateReceiptCannotChangeReopenedView(t *testing.T) {
	p, _ := nativeApprovalPage(t)
	wrapped := &workspaceDelayedDetail{ServiceStore: p.Service.Store, entered: make(chan struct{}), release: make(chan struct{})}
	p.Service.Store = wrapped
	wrapped.block.Store(true)
	var release sync.Once
	defer release.Do(func() { close(wrapped.release) })
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	if err := tester.Click("Deny once"); err != nil {
		t.Fatal(err)
	}
	select {
	case <-wrapped.entered:
	case <-time.After(time.Second):
		t.Fatal("permission did not reach product validation")
	}
	p.generation++ // Reopening the same session must reject its old receipt too.
	p.problem = "current view message"
	release.Do(func() { close(wrapped.release) })
	waitWorkspace(t, p, tester, func() bool { return len(p.approvalSubmitting) == 0 })
	if p.problem != "current view message" {
		t.Fatal("a late permission receipt changed the reopened view", p.problem)
	}
}

func TestNativeApprovalLongResourceReviewKeepsConsentReachable(t *testing.T) {
	p, _ := nativeApprovalPage(t)
	// The actual broker accepts this bounded metadata; only its presentation
	// snapshot is expanded here, and no permission decision is submitted.
	for i := 0; i < 40; i++ {
		p.approvals[0].Effect.Resources = append(p.approvals[0].Effect.Resources, abi.ResourceClaim{Resource: fmt.Sprintf("/workspace/review/file-%02d.txt", i), Access: abi.AccessWrite})
	}
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	before, ok := tester.Find("Allow once")
	for _, label := range []string{"Allow once", "Deny once"} {
		box, ok := tester.Find(label)
		if !ok || box.Y < 0 || box.Y+box.H > 850 {
			t.Fatal("long approval details put the only consent control outside the viewport", label, box)
		}
	}
	viewport, exists := tester.Find("Permission request details")
	last, found := tester.Find("write: /workspace/review/file-39.txt")
	if !ok || !exists || !found || last.Y < viewport.Y+viewport.H {
		t.Fatal("long resource review has no independently scrollable content", viewport, last)
	}
	tester.Scroll(viewport.X+viewport.W-12, viewport.Y+16, 0, 1000)
	last, found = tester.Find("write: /workspace/review/file-39.txt")
	after, present := tester.Find("Allow once")
	if !found || last.H == 0 || last.Y < viewport.Y || last.Y+last.H > viewport.Y+viewport.H || !present || before != after {
		t.Fatal("last resource cannot be reviewed without moving consent", viewport, last, before, after)
	}
}

type nativePendingReadStore struct {
	product.ServiceStore
	reply, snapshot                  atomic.Bool
	replyEntered, replyRelease       chan struct{}
	snapshotEntered, snapshotRelease chan struct{}
}

func (s *nativePendingReadStore) Detail(ctx context.Context, id string) (hub.Detail, error) {
	value, err := s.ServiceStore.Detail(ctx, id)
	if s.reply.CompareAndSwap(true, false) {
		close(s.replyEntered)
		select {
		case <-s.replyRelease:
		case <-ctx.Done():
		}
	}
	return value, err
}

func (s *nativePendingReadStore) BranchTurn(ctx context.Context, id, branch string) (hub.Turn, error) {
	if s.snapshot.CompareAndSwap(true, false) {
		close(s.snapshotEntered)
		select {
		case <-s.snapshotRelease:
		case <-ctx.Done():
		}
	}
	return s.ServiceStore.BranchTurn(ctx, id, branch)
}

func TestNativePendingSnapshotCannotRestoreSubmittedRequest(t *testing.T) {
	for _, kind := range []string{"question", "approval"} {
		t.Run(kind, func(t *testing.T) {
			var p *NativePage
			if kind == "question" {
				p, _, _, _ = nativeQuestionPage(t, "single")
			} else {
				p, _ = nativeApprovalPage(t)
			}
			wrapped := &nativePendingReadStore{ServiceStore: p.Service.Store, replyEntered: make(chan struct{}), replyRelease: make(chan struct{}), snapshotEntered: make(chan struct{}), snapshotRelease: make(chan struct{})}
			p.Service.Store = wrapped
			wrapped.reply.Store(true)
			wrapped.snapshot.Store(true)
			var releaseReply, releaseSnapshot sync.Once
			defer releaseReply.Do(func() { close(wrapped.replyRelease) })
			defer releaseSnapshot.Do(func() { close(wrapped.snapshotRelease) })
			p.pane = "waits"
			tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
			if kind == "question" {
				p.respondQuestion(p.questions[0].ID, interaction.Answer{Selected: []int{2}})
			} else {
				p.respondApproval(p.approvals[0].ID, false)
			}
			select {
			case <-wrapped.replyEntered:
			case <-time.After(time.Second):
				t.Fatal("submission did not reach validation")
			}
			p.poll()
			select {
			case <-wrapped.snapshotEntered:
			case <-time.After(time.Second):
				t.Fatal("observation did not capture the still-pending request")
			}
			releaseReply.Do(func() { close(wrapped.replyRelease) })
			waitWorkspace(t, p, tester, func() bool { return len(p.questionSubmitting) == 0 && len(p.approvalSubmitting) == 0 })
			releaseSnapshot.Do(func() { close(wrapped.snapshotRelease) })
			waitWorkspace(t, p, tester, func() bool { return !p.polling })
			if len(p.questions) != 0 || len(p.approvals) != 0 {
				t.Fatal("an observation started during submission restored an answered request", p.questions, p.approvals)
			}
		})
	}
}
