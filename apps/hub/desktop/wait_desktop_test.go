// INPUT: Actual shared bootstrap, private stores, local SSE tools and MyGo headless controls.
// OUTPUT: Opt-in, cross-window wait observation, bound drafts, stale-read exclusion and explicit continuation acceptance.
// POS: [L7 native UI acceptance] Tests real execution ownership through widgets; OS window/pixel QA remains separate.
//go:build desktop

package hubdesktop

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/harness/durable"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/llm/provider"
	"github.com/egoist/mygo/ui"
)

type waitReadFaultStore struct {
	product.ServiceStore
	unavailable atomic.Bool
}

func (s *waitReadFaultStore) Settings(ctx context.Context, id string) (hub.Settings, error) {
	if s.unavailable.Load() {
		return hub.Settings{}, errors.New("checkpoint settings temporarily unavailable")
	}
	return s.ServiceStore.Settings(ctx, id)
}

func waitNativePage(t *testing.T, tester *ui.Tester, page *NativePage, done func() bool) {
	t.Helper()
	deadline := time.Now().Add(8 * time.Second)
	for time.Now().Before(deadline) {
		tester.Frame()
		if !page.busy && !page.paneBusy && !page.polling && done() {
			return
		}
		page.poll()
		time.Sleep(3 * time.Millisecond)
	}
	t.Fatal("native wait control did not settle", page.problem, page.detail.Session.Status)
}
func TestNativeWaitWidgetsSaveWithoutExecutingAndExplicitlyContinue(t *testing.T) {
	var calls atomic.Int32
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		n := calls.Add(1)
		delta := map[string]any{"content": "continued through native UI"}
		finish := "stop"
		if n%2 == 1 {
			delta = map[string]any{"tool_calls": []any{map[string]any{"index": 0, "id": "native-wait", "type": "function", "function": map[string]string{"name": "await_external", "arguments": `{"request":{"job":"native-ui-wait"}}`}}}}
			finish = "tool_calls"
		}
		data, _ := json.Marshal(map[string]any{"choices": []any{map[string]any{"index": 0, "delta": delta, "finish_reason": finish}}})
		w.Header().Set("Content-Type", "text/event-stream")
		fmt.Fprintf(w, "data: %s\n\ndata: [DONE]\n\n", data)
	}))
	defer upstream.Close()
	app, err := bootstrap.Open(t.Context(), bootstrap.Config{OpenStores: bootstrap.LocalStores, DataDirectory: filepath.Join(t.TempDir(), "private"), ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: upstream.URL, Model: "fixture"}})
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		if err := app.Close(ctx); err != nil {
			t.Error(err)
		}
	}()
	observedStore := &waitReadFaultStore{ServiceStore: app.Service.Store}
	app.Service.Store = observedStore
	s, err := app.Service.Create(t.Context(), "native waiting", "")
	if err != nil {
		t.Fatal(err)
	}
	page := NewNativePage(app.Service)
	page.ctx = t.Context()
	page.selected = s.ID
	page.detail, err = app.Store.Detail(t.Context(), s.ID)
	if err != nil {
		t.Fatal(err)
	}
	page.applyDetail()
	tester := ui.NewTester(page.Build, 1240, 850)
	click := func(label string) {
		t.Helper()
		if err := tester.Click(label); err != nil {
			t.Fatal(label, err)
		}
	}
	click("Waiting tasks")
	settlePage(t, tester, page)
	click("Enable durable waits next Turn")
	settlePage(t, tester, page)
	if page.problem != "" || !page.wait.View.Enabled || calls.Load() != 0 {
		t.Fatal(page.problem, page.wait.View, calls.Load())
	}
	click("Chat")
	click("Message")
	tester.Type("one explicit native input")
	click("Send")
	waitNativePage(t, tester, page, func() bool { return page.detail.Session.Status == engine.Waiting && !app.Runtime.SessionActive(s.ID) })
	if page.model.ActiveRunID != "" || !page.chat.Busy {
		t.Fatal("waiting pretended execution was running or allowed input", page.model)
	}
	page.model.Draft = "retained draft for later"
	tester.Frame()
	click("Send")
	click("Stop")
	if calls.Load() != 1 || page.model.Draft != "retained draft for later" {
		t.Fatal("waiting composer executed/lost the draft", calls.Load(), page.model.Draft)
	}
	click("Waiting tasks")
	settlePage(t, tester, page)
	turn := page.wait.View.TurnID
	if page.wait.View.CanResume || len(page.wait.View.Snapshot.State.Requests) != 1 {
		t.Fatal(page.wait.View)
	}
	observer := NewNativePage(app.Service)
	observer.ctx, observer.selected, observer.detail = t.Context(), s.ID, page.detail
	observer.applyDetail()
	other := ui.NewTester(observer.Build, 1240, 850)
	if err := other.Click("Waiting tasks"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, other, observer)
	if err := other.Click("Explicit result JSON (at most 16 KiB)"); err != nil {
		t.Fatal(err)
	}
	other.Type(`{"value":"unsubmitted in another window"}`)
	request := observer.wait.View.Snapshot.State.Requests[0]
	observer.wait.Confirm = turn
	observer.poll()
	waitNativePage(t, other, observer, func() bool { return observer.wait.Current })
	if observer.wait.Answers[request.ID] != `{"value":"unsubmitted in another window"}` || observer.wait.Confirm != turn {
		t.Fatal("unchanged checkpoint observation erased operator input")
	}
	click("Explicit result JSON (at most 16 KiB)")
	tester.Type(`{"value":"native-result"}`)
	click("Save bound result without executing")
	settlePage(t, tester, page)
	if page.problem != "" || !page.wait.View.CanResume || calls.Load() != 1 || app.Runtime.SessionActive(s.ID) {
		t.Fatal("result save executed or failed", page.problem, page.wait.View, calls.Load())
	}
	// The independent observer notices a result without Refresh and can inspect
	// what was actually saved, instead of mistaking its local draft for facts.
	observer.poll()
	waitNativePage(t, other, observer, func() bool { return observer.wait.View.CanResume })
	if observer.wait.Confirm != "" || len(observer.wait.Answers) != 0 || !other.HasText("Complete saved result — untrusted data") || calls.Load() != 1 {
		t.Fatal("cross-window save retained stale review/draft or executed work", observer.wait, calls.Load())
	}
	observedStore.unavailable.Store(true)
	observer.poll()
	waitNativePage(t, other, observer, func() bool { return !observer.wait.Current && observer.wait.Problem != "" })
	if err := other.Click("Continue this waiting Turn"); err != nil {
		t.Fatal(err)
	}
	if calls.Load() != 1 || app.Runtime.SessionActive(s.ID) {
		t.Fatal("unavailable checkpoint left Continue enabled")
	}
	observedStore.unavailable.Store(false)
	observer.poll()
	waitNativePage(t, other, observer, func() bool { return observer.wait.Current && observer.wait.Problem == "" })
	// Hold one already-completed background read across another window's
	// continuation and a newer explicit Refresh, then deliver it last.
	observer.poll()
	var stale func()
	select {
	case stale = <-observer.updates:
	case <-time.After(3 * time.Second):
		t.Fatal("observer read did not finish")
	}
	click("Continue this waiting Turn")
	waitNativePage(t, tester, page, func() bool { return page.detail.Session.Status == engine.Succeeded && !app.Runtime.SessionActive(s.ID) })
	actual, err := app.Store.BranchTurn(t.Context(), s.ID, page.wait.View.BranchID)
	if err != nil || actual.ID != turn || calls.Load() != 2 || page.model.Draft != "retained draft for later" {
		t.Fatal(actual, calls.Load(), page.model.Draft, err)
	}
	observer.openWaits()
	settlePage(t, other, observer)
	stale()
	other.Frame()
	if !observer.wait.Current || observer.wait.View.Snapshot.State.Status != durable.Idle || observer.wait.View.CanResume || observer.wait.View.Status != engine.Succeeded || calls.Load() != 2 {
		t.Fatal("stale read resurrected a delivered checkpoint", observer.wait, calls.Load())
	}
	click("Chat")
	click("Send") // This is a new, explicit Turn using the retained draft.
	waitNativePage(t, tester, page, func() bool { return page.detail.Session.Status == engine.Waiting && !app.Runtime.SessionActive(s.ID) })
	click("Waiting tasks")
	settlePage(t, tester, page)
	click("Abandon reviewed checkpoint")
	if calls.Load() != 3 || page.wait.View.Snapshot.State.Status != durable.Waiting {
		t.Fatal("unconfirmed abandonment changed facts")
	}
	click("Reviewed Turn ID for abandonment")
	tester.Type(page.wait.View.TurnID)
	click("Abandon reviewed checkpoint")
	waitNativePage(t, tester, page, func() bool {
		return page.detail.Session.Status == engine.Interrupted && page.wait.View.Snapshot.State.Status == durable.Idle
	})
	if page.problem != "" || calls.Load() != 3 {
		t.Fatal("abandonment executed a model", page.problem, calls.Load())
	}
	observer.poll()
	waitNativePage(t, other, observer, func() bool {
		return observer.wait.Current && observer.wait.View.Status == engine.Interrupted && observer.wait.View.Snapshot.State.Status == durable.Idle
	})
	if observer.wait.View.CanResume || observer.wait.View.TurnID == turn || calls.Load() != 3 {
		t.Fatal("observer retained an old Turn after explicit new work and abandonment", observer.wait, calls.Load())
	}
}

func TestWaitObservationRetainsOnlyTheSameUnresolvedBinding(t *testing.T) {
	page := &NativePage{}
	initial := product.WaitView{BranchID: "branch", TurnID: "turn", Snapshot: durable.Snapshot{Revision: 1, State: durable.State{Status: durable.Waiting, Requests: []durable.Request{
		{ID: "first", Binding: "first-binding"}, {ID: "second", Binding: "second-binding"},
	}}}}
	page.applyWaitView(initial)
	page.wait.Answers["first"], page.wait.Answers["second"] = "first draft", "retained draft"
	page.wait.Confirm = "turn"
	updated := initial
	updated.Snapshot.Revision++
	updated.Snapshot.State.Requests = append([]durable.Request(nil), initial.Snapshot.State.Requests...)
	updated.Snapshot.State.Requests[0].Result = json.RawMessage(`{"saved":"elsewhere"}`)
	page.applyWaitView(updated)
	if len(page.wait.Answers) != 1 || page.wait.Answers["second"] != "retained draft" || page.wait.Confirm != "" {
		t.Fatal("another result erased an unrelated draft or retained an old review", page.wait)
	}
	updated.Snapshot.State.Requests = append([]durable.Request(nil), updated.Snapshot.State.Requests...)
	updated.Snapshot.State.Requests[1].Binding = "different-action"
	page.applyWaitView(updated)
	if len(page.wait.Answers) != 0 {
		t.Fatal("draft followed a different binding", page.wait)
	}
}
