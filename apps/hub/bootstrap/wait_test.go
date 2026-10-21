// INPUT: Actual shared Hub bootstrap, persistent product/native ports and local-only SSE tool fixtures.
// OUTPUT: Restarted same-Turn waits, once-only resolution/approval/question and immutable recipe acceptance.
// POS: [L7 integration] Verifies the product owner and native Controller together without paid models or replayed input.
package bootstrap

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/harness/durable"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/llm/provider"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

func waitReply(w http.ResponseWriter, tool, args, text string) {
	w.Header().Set("Content-Type", "text/event-stream")
	delta := map[string]any{"content": text}
	finish := "stop"
	if tool != "" {
		finish = "tool_calls"
		delta = map[string]any{"tool_calls": []any{map[string]any{"index": 0, "id": "wait-call", "type": "function", "function": map[string]string{"name": tool, "arguments": args}}}}
	}
	data, _ := json.Marshal(map[string]any{"choices": []any{map[string]any{"index": 0, "delta": delta, "finish_reason": finish}}})
	fmt.Fprintf(w, "data: %s\n\ndata: [DONE]\n\n", data)
}
func waitStatus(t *testing.T, a *Application, id string, status engine.Outcome) product.WaitView {
	t.Helper()
	deadline := time.Now().Add(10 * time.Second)
	for time.Now().Before(deadline) {
		d, err := a.Store.Detail(t.Context(), id)
		if err != nil {
			t.Fatal(err)
		}
		if !a.Runtime.SessionActive(id) && d.Session.Status != hub.Running {
			if d.Session.Status != status {
				h, _ := a.Store.History(t.Context(), id, 0, 128)
				raw, _ := json.Marshal(h)
				t.Fatalf("status %s want %s: %s", d.Session.Status, status, raw)
			}
			v, e := a.Service.Waits(t.Context(), id)
			if e != nil {
				t.Fatal(e)
			}
			return v
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("actual Hub owner did not exit")
	return product.WaitView{}
}
func waitConfig(t *testing.T, endpoint string) Config {
	return Config{OpenStores: testStores, OpenMemory: LocalMemory, OpenBindings: LocalBindings, DataDirectory: filepath.Join(t.TempDir(), "private"), ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: endpoint, Model: "fixture"}}
}
func waitTarget(v product.WaitView) product.WaitTarget {
	return product.WaitTarget{BranchID: v.BranchID, TurnID: v.TurnID, Revision: v.Snapshot.Revision}
}
func resolveFirst(t *testing.T, a *Application, id string, v product.WaitView, result string) {
	t.Helper()
	r := v.Snapshot.State.Requests[0]
	if err := a.Service.ResolveWait(t.Context(), id, product.WaitResolution{Target: waitTarget(v), RequestID: r.ID, Binding: r.Binding, Result: json.RawMessage(result)}); err != nil {
		t.Fatal(err)
	}
}
func TestHubWaitExternalRestartSameTurnAndRecipe(t *testing.T) {
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if calls.Add(1) == 1 {
			waitReply(w, "await_external", `{"request":{"job":"external-result"}}`, "")
			return
		}
		var body any
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Error(err)
		}
		data, _ := json.Marshal(body)
		if !bytes.Contains(data, []byte("resolved-external")) {
			t.Error("result not delivered", string(data))
		}
		waitReply(w, "", "", "continued")
	}))
	defer server.Close()
	config := waitConfig(t, server.URL)
	a, err := Open(t.Context(), config)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { closeApp(t, a) }()
	s, err := a.Service.Create(t.Context(), "durable wait", "")
	if err != nil {
		t.Fatal(err)
	}
	if err = a.Service.EnableDurableWaits(t.Context(), s.ID, true); err != nil {
		t.Fatal(err)
	}
	original, err := a.Service.Submit(t.Context(), s.ID, "one original prompt")
	if err != nil {
		t.Fatal(err)
	}
	view := waitStatus(t, a, s.ID, engine.Waiting)
	if view.TurnID != original || len(view.Snapshot.State.Requests) != 1 || view.CanResume || calls.Load() != 1 {
		t.Fatal(view, calls.Load())
	}
	if _, err = a.Service.Submit(t.Context(), s.ID, "bypass"); err == nil {
		t.Fatal("new input bypassed wait")
	}
	d, _ := a.Store.Detail(t.Context(), s.ID)
	if _, err = a.Store.Switch(t.Context(), s.ID, d.Branches[0].Target, "cannot switch waiting"); err == nil {
		t.Fatal("waiting route changed")
	}
	if err = a.Store.SetPersistentMemory(t.Context(), s.ID, true); err == nil {
		t.Fatal("waiting recipe mutated")
	}
	closeApp(t, a)
	changed := config
	changed.Provider.Headers = map[string]string{"X-Recipe": "changed"}
	a, err = Open(t.Context(), changed)
	if err != nil {
		t.Fatal(err)
	}
	view, err = a.Service.Waits(t.Context(), s.ID)
	if err != nil {
		t.Fatal(err)
	}
	if calls.Load() != 1 || view.TurnID != original || view.Status != engine.Waiting {
		t.Fatal("restart executed or lost checkpoint", view, calls.Load())
	}
	r := view.Snapshot.State.Requests[0]
	bad := product.WaitResolution{Target: waitTarget(view), RequestID: r.ID, Binding: strings.Repeat("0", 64), Result: json.RawMessage(`{"value":"resolved-external"}`)}
	if err = a.Service.ResolveWait(t.Context(), s.ID, bad); err == nil {
		t.Fatal("wrong binding accepted")
	}
	resolveFirst(t, a, s.ID, view, `{"value":"resolved-external"}`)
	view, err = a.Service.Waits(t.Context(), s.ID)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = a.Service.ResumeWait(t.Context(), s.ID, waitTarget(view)); err == nil {
		t.Fatal("different process recipe continued")
	}
	if calls.Load() != 1 {
		t.Fatal("invalid recipe called model")
	}
	closeApp(t, a)
	a, err = Open(t.Context(), config)
	if err != nil {
		t.Fatal(err)
	}
	view, err = a.Service.Waits(t.Context(), s.ID)
	if err != nil || !view.CanResume {
		t.Fatal(view, err)
	}
	stale := waitTarget(view)
	stale.Revision--
	if _, err = a.Service.ResumeWait(t.Context(), s.ID, stale); !errors.Is(err, durable.ErrConflict) {
		t.Fatal(err)
	}
	resumed, err := a.Service.ResumeWait(t.Context(), s.ID, waitTarget(view))
	if err != nil || resumed != original {
		t.Fatal(resumed, err)
	}
	waitStatus(t, a, s.ID, engine.Succeeded)
	if calls.Load() != 2 {
		t.Fatal("model request replayed", calls.Load())
	}
	history, err := a.Store.History(t.Context(), s.ID, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	inputs := 0
	attempts := map[string]bool{}
	waiting, completed := 0, 0
	for _, e := range history {
		if e.Input != "" {
			inputs++
		}
		if e.Event.Kind == engine.SessionStarted {
			attempts[e.AttemptID] = true
		}
		if e.Event.Kind == engine.Completed {
			if e.Event.Outcome == engine.Waiting {
				waiting++
			}
			if e.Event.Outcome == engine.Succeeded {
				completed++
			}
		}
	}
	turn, err := a.Store.BranchTurn(t.Context(), s.ID, view.BranchID)
	if err != nil || turn.ID != original || turn.Sequence != 1 || inputs != 1 || len(attempts) != 2 || waiting != 1 || completed != 1 {
		t.Fatal(turn, inputs, attempts, waiting, completed, err)
	}
	if _, err = a.Service.ResumeWait(t.Context(), s.ID, waitTarget(view)); err == nil {
		t.Fatal("continued a delivered checkpoint twice")
	}
}
func TestHubWaitApprovalAndQuestion(t *testing.T) {
	for _, kind := range []string{"approval", "question"} {
		t.Run(kind, func(t *testing.T) {
			workspace := t.TempDir()
			var calls atomic.Int32
			tool, args := "write_file", `{"path":"once.txt","content":"approved once"}`
			if kind == "question" {
				tool, args = "ask_human", `{"question":"Choose explicitly","options":[{"label":"First"},{"label":"Second"}]}`
			}
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				if calls.Add(1) <= 2 {
					waitReply(w, tool, args, "")
				} else {
					waitReply(w, "", "", "resolved once")
				}
			}))
			defer server.Close()
			a, err := Open(t.Context(), waitConfig(t, server.URL))
			if err != nil {
				t.Fatal(err)
			}
			defer closeApp(t, a)
			s, err := a.Service.Create(t.Context(), kind, workspace)
			if err != nil {
				t.Fatal(err)
			}
			if kind == "question" {
				if err = a.Store.SetToolEnabled(t.Context(), s.ID, "interaction", true, false); err != nil {
					t.Fatal(err)
				}
			}
			if err = a.Service.EnableDurableWaits(t.Context(), s.ID, true); err != nil {
				t.Fatal(err)
			}
			if _, err = a.Service.Submit(t.Context(), s.ID, "explicit wait"); err != nil {
				t.Fatal(err)
			}
			view := waitStatus(t, a, s.ID, engine.Waiting)
			if len(view.Snapshot.State.Requests) != 1 || view.Snapshot.State.Requests[0].Kind != kind {
				t.Fatal(view)
			}
			if _, err = os.Stat(filepath.Join(workspace, "once.txt")); !os.IsNotExist(err) {
				t.Fatal("side effect before approval", err)
			}
			result := `{"allow":true}`
			if kind == "question" {
				result = `{"selected":[2]}`
			}
			resolveFirst(t, a, s.ID, view, result)
			view, err = a.Service.Waits(t.Context(), s.ID)
			if err != nil {
				t.Fatal(err)
			}
			if _, err = a.Service.ResumeWait(t.Context(), s.ID, waitTarget(view)); err != nil {
				t.Fatal(err)
			}
			waitStatus(t, a, s.ID, engine.Succeeded)
			if calls.Load() != 3 {
				t.Fatal("requests repeated", calls.Load())
			}
			if kind == "approval" {
				data, err := os.ReadFile(filepath.Join(workspace, "once.txt"))
				if err != nil || string(data) != "approved once" {
					t.Fatal(string(data), err)
				}
			}
		})
	}
}
