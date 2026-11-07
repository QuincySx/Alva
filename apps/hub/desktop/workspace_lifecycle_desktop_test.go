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

	"github.com/QuincySx/Alva/apps/hub"
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
	tester := ui.NewTester(workspaceStaticView(page), 1240, 850)
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
	tester := ui.NewTester(workspaceStaticView(page), 1240, 850)
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
	tester := ui.NewTester(workspaceStaticView(page), 1240, 850)
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
	if _, err = service.Submit(t.Context(), session.ID, "started by another window", nil); err != nil {
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
			tester := ui.NewTester(workspaceStaticView(page), 1240, 850)
			page.selectSession(session.ID)
			waitWorkspace(t, page, tester, func() bool { return !page.polling && page.detail.Session.ID == session.ID })
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

func TestWorkspaceComposerPressedStopDoesNotFollowReplacement(t *testing.T) {
	for _, workerScope := range []bool{false, true} {
		t.Run(map[bool]string{false: "ordinary", true: "worker_execution"}[workerScope], func(t *testing.T) {
			s, _ := fixture(t)
			first := &workspaceStoppingEngine{started: make(chan struct{}), cleanup: make(chan struct{}), release: make(chan struct{})}
			second := &workspaceStoppingEngine{started: make(chan struct{}), cleanup: make(chan struct{}), release: make(chan struct{})}
			var firstRelease, secondRelease sync.Once
			defer firstRelease.Do(func() { close(first.release) })
			defer secondRelease.Do(func() { close(second.release) })
			if err := s.Runtime.Register(s.DefaultTarget.EngineID, &workspaceExecutionSequence{owners: []*workspaceStoppingEngine{first, second}}); err != nil {
				t.Fatal(err)
			}
			lead, err := s.Create(t.Context(), "pressed stop fixture", "")
			if err != nil {
				t.Fatal(err)
			}
			id := lead.ID
			workerID := ""
			var team hub.TeamDetail
			if workerScope {
				team, err = s.Teams.Store.CreateTeam(t.Context(), lead.ID)
				if err != nil {
					t.Fatal(err)
				}
				worker, e := s.AddWorker(t.Context(), lead.ID, team.Team.ID, "held worker", s.DefaultTarget, true)
				if e != nil {
					t.Fatal(e)
				}
				id = worker.SessionID
				workerID = worker.ID
				team, err = s.Teams.Store.Team(t.Context(), lead.ID, team.Team.ID)
				if err != nil {
					t.Fatal(err)
				}
			}
			startRun := func(prompt string) (string, <-chan error, error) {
				if workerScope {
					if _, err := s.Teams.Store.EnqueueWorker(t.Context(), lead.ID, team.Team.ID, workerID, prompt, nil); err != nil {
						return "", nil, err
					}
					_, done, err := s.Runtime.StartWorkerTask(t.Context(), lead.ID, team.Team.ID, workerID)
					return "", done, err
				}
				return s.Runtime.Start(t.Context(), id, prompt)
			}
			_, done, err := startRun("first execution")
			if err != nil {
				t.Fatal(err)
			}
			select {
			case <-first.started:
			case <-time.After(time.Second):
				t.Fatal("first owner did not start")
			}
			p := NewNativePage(s)
			p.team = team
			tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
			p.selectSession(id)
			waitWorkspace(t, p, tester, func() bool { return !p.polling && !p.chat.Busy && p.model.Status == view.Running })
			button, found := tester.Find("Stop")
			if !found {
				t.Fatal("missing execution Stop")
			}
			tester.Press(button.X+button.W/2, button.Y+button.H/2)
			before := p.transcript.Last
			firstID, _ := s.Runtime.ActiveExecution(id)
			if err = s.Runtime.CancelExecution(t.Context(), id, firstID); err != nil {
				t.Fatal(err)
			}
			firstRelease.Do(func() { close(first.release) })
			select {
			case <-done:
			case <-time.After(time.Second):
				t.Fatal("first owner did not exit")
			}
			_, nextDone, err := startRun("replacement execution")
			if err != nil {
				t.Fatal(err)
			}
			select {
			case <-second.started:
			case <-time.After(time.Second):
				t.Fatal("replacement did not start")
			}
			nextID, _ := s.Runtime.ActiveExecution(id)
			if nextID == firstID {
				t.Fatal("execution identity was reused")
			}
			p.poll()
			waitWorkspace(t, p, tester, func() bool { return !p.polling && p.transcript.Last > before && p.model.Status == view.Running })
			tester.Release(button.X+button.W/2, button.Y+button.H/2)
			if p.stopRequests[id] == nextID {
				select {
				case <-second.cleanup:
				case <-time.After(time.Second):
					t.Fatal("replacement Stop did not reach physical owner")
				}
				t.Fatal("old pressed execution Stop canceled the replacement", second.cancels.Load())
			}
			if second.cancels.Load() != 0 {
				t.Fatal("replacement cancellation was delivered")
			}

			// A stale handler is also safe without relying on a reconciled widget key.
			p.stopExecution(id, firstID)
			if p.stopRequests[id] == nextID {
				t.Fatal("stale handler rebound to replacement")
			}
			if err := tester.Click("Stop"); err != nil {
				t.Fatal(err)
			}
			select {
			case <-second.cleanup:
			case <-time.After(time.Second):
				t.Fatal("fresh Stop did not reach replacement owner")
			}
			if second.cancels.Load() != 1 {
				t.Fatal("fresh Stop must cancel exactly its observed owner")
			}
			secondRelease.Do(func() { close(second.release) })
			select {
			case <-nextDone:
			case <-time.After(time.Second):
				t.Fatal("fixture cleanup did not finish")
			}
		})
	}
}
