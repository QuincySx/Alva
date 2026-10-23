// INPUT: Native button events, delayed real service admission and a cancellation-controlled engine fixture.
// OUTPUT: Draft snapshot acknowledgement and Stop ownership across workspace navigation regressions.
// POS: [L7 Hub UI testing] Verifies design execution/submit contracts without external model accounts.
//go:build desktop

package hubdesktop

import (
	"context"
	"errors"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub/product"
	view "github.com/QuincySx/Alva/apps/ui/chat"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/egoist/mygo/ui"
)

type workspaceStoppingEngine struct {
	started, cleanup, release chan struct{}
	cancels                   atomic.Int32
	failCancel                atomic.Bool
}

func (e *workspaceStoppingEngine) Execute(ctx context.Context, _ engine.Request, emit func(engine.Event) error) error {
	if err := emit(engine.Event{Kind: engine.SessionStarted, SessionID: "native-stopping-fixture"}); err != nil {
		return err
	}
	close(e.started)
	<-ctx.Done()
	close(e.cleanup)
	<-e.release
	return ctx.Err()
}
func (e *workspaceStoppingEngine) Cancel(context.Context, string) error {
	e.cancels.Add(1)
	if e.failCancel.CompareAndSwap(true, false) {
		return errors.New("cancel delivery failed while execution cleanup is pending")
	}
	return nil
}
func (*workspaceStoppingEngine) RespondPermission(context.Context, string, string, engine.PermissionDecision) error {
	return engine.ErrUnsupported
}
func (*workspaceStoppingEngine) Capabilities() engine.Capabilities {
	return engine.Capabilities{Streaming: true, Resume: true, Cancel: true, ToolControl: true}
}

func TestWorkspaceStopSurvivesNavigationUntilActualCleanup(t *testing.T) {
	service, _ := fixture(t)
	owner := &workspaceStoppingEngine{started: make(chan struct{}), cleanup: make(chan struct{}), release: make(chan struct{})}
	var release sync.Once
	defer release.Do(func() { close(owner.release) })
	if err := service.Runtime.Register(service.DefaultTarget.EngineID, owner); err != nil {
		t.Fatal(err)
	}
	first, err := service.Create(t.Context(), "stop this task", "")
	if err != nil {
		t.Fatal(err)
	}
	other, err := service.Create(t.Context(), "another task", "")
	if err != nil {
		t.Fatal(err)
	}
	page := NewNativePage(service)
	tester := ui.NewTester(page.Build, 1240, 850)
	page.selectSession(first.ID)
	waitWorkspace(t, page, tester, func() bool { return page.detail.Session.ID == first.ID && !page.chat.Busy })
	page.model.Draft = "local execution"
	if err := tester.Click("Send"); err != nil {
		t.Fatal(err)
	}
	select {
	case <-owner.started:
	case <-time.After(5 * time.Second):
		t.Fatal("actual engine did not start")
	}
	waitWorkspace(t, page, tester, func() bool { return !page.busy && page.model.Status == view.Running })
	if err := tester.Click("Stop"); err != nil {
		t.Fatal(err)
	}
	select {
	case <-owner.cleanup:
	case <-time.After(5 * time.Second):
		t.Fatal("Stop did not reach actual execution owner")
	}
	page.selectSession(other.ID)
	waitWorkspace(t, page, tester, func() bool { return page.detail.Session.ID == other.ID && !page.chat.Busy })
	page.selectSession(first.ID)
	waitWorkspace(t, page, tester, func() bool { return page.detail.Session.ID == first.ID && !page.chat.Busy })
	if page.model.Status != view.Stopping || !service.Runtime.SessionActive(first.ID) {
		t.Fatal("navigation discarded pending Stop before physical cleanup")
	}
	if err := tester.Click("Stop"); err != nil {
		t.Fatal(err)
	}
	if owner.cancels.Load() != 1 {
		t.Fatal("pending Stop was submitted again after navigation")
	}
	release.Do(func() { close(owner.release) })
	waitWorkspace(t, page, tester, func() bool { return !service.Runtime.SessionActive(first.ID) && page.model.Status == view.Cancelled })
	if page.stopRequests[first.ID] != "" || page.model.ActiveRunID != "" {
		t.Fatal("actual cancellation acknowledgement retained pending execution")
	}
}

func TestWorkspaceStopErrorKeepsCleanupStateAndRequiresExplicitRetry(t *testing.T) {
	service, _ := fixture(t)
	owner := &workspaceStoppingEngine{started: make(chan struct{}), cleanup: make(chan struct{}), release: make(chan struct{})}
	owner.failCancel.Store(true)
	var release sync.Once
	defer release.Do(func() { close(owner.release) })
	if err := service.Runtime.Register(service.DefaultTarget.EngineID, owner); err != nil {
		t.Fatal(err)
	}
	session, err := service.Create(t.Context(), "retry stop", "")
	if err != nil {
		t.Fatal(err)
	}
	page := NewNativePage(service)
	tester := ui.NewTester(page.Build, 1240, 850)
	page.selectSession(session.ID)
	waitWorkspace(t, page, tester, func() bool { return !page.chat.Busy })
	page.model.Draft = "hold cleanup"
	page.send()
	select {
	case <-owner.started:
	case <-time.After(time.Second):
		t.Fatal("actual engine did not start")
	}
	waitWorkspace(t, page, tester, func() bool { return !page.busy && page.model.Status == view.Running })
	if err := tester.Click("Stop"); err != nil {
		t.Fatal(err)
	}
	waitWorkspace(t, page, tester, func() bool { return page.stopRetry[session.ID] })
	if page.model.Status != view.Stopping || !service.Runtime.SessionActive(session.ID) || owner.cancels.Load() != 1 {
		t.Fatal("Cancel error hid actual pending cleanup or retried automatically")
	}
	if err := tester.Click("Stop"); err != nil {
		t.Fatal(err)
	}
	waitWorkspace(t, page, tester, func() bool { return owner.cancels.Load() == 2 })
	if page.model.Status != view.Stopping || page.stopRetry[session.ID] || !service.Runtime.SessionActive(session.ID) {
		t.Fatal("explicit retry claimed cancellation was already complete")
	}
	release.Do(func() { close(owner.release) })
	waitWorkspace(t, page, tester, func() bool { return !service.Runtime.SessionActive(session.ID) && page.model.Status == view.Cancelled })
	if page.stopRequests[session.ID] != "" || page.stopRetry[session.ID] {
		t.Fatal("actual cancellation acknowledgement retained retry state")
	}
}

type workspaceExecutionSequence struct {
	owners []*workspaceStoppingEngine
	next   atomic.Int32
}

func (s *workspaceExecutionSequence) Execute(ctx context.Context, request engine.Request, emit func(engine.Event) error) error {
	return s.owners[s.next.Add(1)-1].Execute(ctx, request, emit)
}
func (s *workspaceExecutionSequence) Cancel(ctx context.Context, id string) error {
	return s.owners[s.next.Load()-1].Cancel(ctx, id)
}
func (*workspaceExecutionSequence) RespondPermission(context.Context, string, string, engine.PermissionDecision) error {
	return engine.ErrUnsupported
}
func (*workspaceExecutionSequence) Capabilities() engine.Capabilities {
	return engine.Capabilities{Streaming: true, Resume: true, Cancel: true, ToolControl: true}
}

func TestWorkspaceOldStopDoesNotBlockReplacementExecution(t *testing.T) {
	service, _ := fixture(t)
	first := &workspaceStoppingEngine{started: make(chan struct{}), cleanup: make(chan struct{}), release: make(chan struct{})}
	second := &workspaceStoppingEngine{started: make(chan struct{}), cleanup: make(chan struct{}), release: make(chan struct{})}
	var releaseFirst, releaseSecond sync.Once
	defer releaseFirst.Do(func() { close(first.release) })
	defer releaseSecond.Do(func() { close(second.release) })
	if err := service.Runtime.Register(service.DefaultTarget.EngineID, &workspaceExecutionSequence{owners: []*workspaceStoppingEngine{first, second}}); err != nil {
		t.Fatal(err)
	}
	session, err := service.Create(t.Context(), "replacement execution", "")
	if err != nil {
		t.Fatal(err)
	}
	other, err := service.Create(t.Context(), "away from first task", "")
	if err != nil {
		t.Fatal(err)
	}
	page := NewNativePage(service)
	tester := ui.NewTester(page.Build, 1240, 850)
	page.selectSession(session.ID)
	waitWorkspace(t, page, tester, func() bool { return !page.chat.Busy })
	page.model.Draft = "first run"
	page.send()
	select {
	case <-first.started:
	case <-time.After(time.Second):
		t.Fatal("first execution did not start")
	}
	waitWorkspace(t, page, tester, func() bool { return !page.busy && page.model.Status == view.Running })
	page.stop()
	select {
	case <-first.cleanup:
	case <-time.After(time.Second):
		t.Fatal("first execution did not start cancellation")
	}
	page.selectSession(other.ID)
	waitWorkspace(t, page, tester, func() bool { return page.detail.Session.ID == other.ID && !page.chat.Busy })
	releaseFirst.Do(func() { close(first.release) })
	deadline := time.Now().Add(time.Second)
	for service.Runtime.SessionActive(session.ID) && time.Now().Before(deadline) {
		time.Sleep(time.Millisecond)
	}
	if _, err = service.Submit(t.Context(), session.ID, "started by another window"); err != nil {
		t.Fatal(err)
	}
	select {
	case <-second.started:
	case <-time.After(time.Second):
		t.Fatal("replacement execution did not start")
	}
	page.selectSession(session.ID)
	waitWorkspace(t, page, tester, func() bool { return page.detail.Session.ID == session.ID && !page.chat.Busy })
	if page.model.Status != view.Running || page.stopRequests[session.ID] != "" {
		t.Fatal("the previous execution's Stop leaked into its replacement")
	}
	if err = tester.Click("Stop"); err != nil {
		t.Fatal(err)
	}
	select {
	case <-second.cleanup:
	case <-time.After(time.Second):
		t.Fatal("replacement execution lost its real Stop action")
	}
}

func TestWorkspaceAdmissionKeepsEditsMadeAfterSend(t *testing.T) {
	for _, kind := range []string{"plain", "prepared"} {
		t.Run(kind, func(t *testing.T) {
			service, _ := fixture(t)
			session, err := service.Create(t.Context(), "snapshot draft", "")
			if err != nil {
				t.Fatal(err)
			}
			page := NewNativePage(service)
			page.selected = session.ID
			page.detail, err = service.Store.Detail(t.Context(), session.ID)
			if err != nil {
				t.Fatal(err)
			}
			page.applyDetail()
			page.model.Draft = "first version"
			if kind == "prepared" {
				selection, err := service.PrepareComposerSelection(t.Context(), session.ID, product.ComposerSelectionRequest{EntryID: "builtin:review"})
				if err != nil {
					t.Fatal(err)
				}
				page.composerSelections = map[string][]product.ComposerPreparation{session.ID: {selection}}
			}
			wrapped := &workspaceDelayedDetail{ServiceStore: service.Store, entered: make(chan struct{}), release: make(chan struct{})}
			service.Store = wrapped
			tester := ui.NewTester(page.Build, 1240, 850)
			wrapped.block.Store(true)
			if err := tester.Click("Send"); err != nil {
				t.Fatal(err)
			}
			var release sync.Once
			defer release.Do(func() { close(wrapped.release) })
			select {
			case <-wrapped.entered:
			case <-time.After(time.Second):
				t.Fatal("admission did not reach the delayed service")
			}
			if err := tester.Click("Message"); err != nil {
				t.Fatal(err)
			}
			tester.Type(" + next version")
			updated := page.model.Draft
			if updated == "first version" {
				t.Fatal("submission prevented editing the next draft")
			}
			release.Do(func() { close(wrapped.release) })
			waitWorkspace(t, page, tester, func() bool { return !page.busy })
			if page.model.Draft != updated {
				t.Fatal("an old admission acknowledgement erased newer input")
			}
			history, err := service.Store.History(t.Context(), session.ID, 0, 128)
			if err != nil {
				t.Fatal(err)
			}
			var inputs []string
			for _, entry := range history {
				if entry.Input != "" {
					inputs = append(inputs, entry.Input)
				}
			}
			if len(inputs) != 1 || inputs[0] != "first version" {
				t.Fatalf("submission changed its frozen prompt: %q", inputs)
			}

		})
	}
}
