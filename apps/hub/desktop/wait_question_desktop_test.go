// INPUT: Real Hub bootstrap/stores, local SSE question/write fixtures and MyGo controls.
// OUTPUT: Typed durable answers survive reopening, save without execution and never authorize a write.
// POS: [L7 presentation acceptance] Exercises the existing Controller through UI; no OS-window or paid-model claim.
//go:build desktop

package hubdesktop

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/extension/interaction"
	"github.com/QuincySx/Alva/harness/durable"
	"github.com/QuincySx/Alva/harness/durable/journal"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/llm/provider"
	"github.com/egoist/mygo/ui"
)

func TestNativeDurableQuestionControlsSurviveReopenAndDoNotGrantPermission(t *testing.T) {
	for _, kind := range []string{"single", "multiple", "text", "expired"} {
		t.Run(kind, func(t *testing.T) {
			var calls atomic.Int32
			question := map[string]any{"header": "Saved clarification", "question": "Choose the task input explicitly"}
			if kind != "text" {
				question["options"] = []map[string]string{{"label": "First choice"}, {"label": "Second choice"}, {"label": "Third choice"}}
				question["multi_select"] = kind == "multiple"
			}
			questionJSON, _ := json.Marshal(question)
			upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				n := calls.Add(1)
				tool, args := "ask_human", string(questionJSON)
				if n >= 3 {
					tool, args = "write_file", `{"path":"requires-separate-approval.txt","content":"must not write from an answer"}`
				}
				delta := map[string]any{"tool_calls": []any{map[string]any{"index": 0, "id": fmt.Sprintf("native-question-call-%d", n), "type": "function", "function": map[string]string{"name": tool, "arguments": args}}}}
				data, _ := json.Marshal(map[string]any{"choices": []any{map[string]any{"index": 0, "delta": delta, "finish_reason": "tool_calls"}}})
				w.Header().Set("Content-Type", "text/event-stream")
				fmt.Fprintf(w, "data: %s\n\ndata: [DONE]\n\n", data)
			}))
			defer upstream.Close()
			config := bootstrap.Config{OpenStores: bootstrap.LocalStores, DataDirectory: filepath.Join(t.TempDir(), "private"), ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: upstream.URL, Model: "fixture"}}
			var app *bootstrap.Application
			open := func() {
				t.Helper()
				var err error
				app, err = bootstrap.Open(t.Context(), config)
				if err != nil {
					t.Fatal(err)
				}
			}
			closeApp := func() {
				t.Helper()
				if app != nil {
					ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
					defer cancel()
					if err := app.Close(ctx); err != nil {
						t.Fatal(err)
					}
					app = nil
				}
			}
			defer closeApp()
			open()
			workspace := t.TempDir()
			session, err := app.Service.Create(t.Context(), "typed waiting "+kind, workspace)
			if err != nil {
				t.Fatal(err)
			}
			if err = app.Service.SetToolEnabled(t.Context(), session.ID, "interaction", true, false); err != nil {
				t.Fatal(err)
			}
			if err = app.Service.EnableDurableWaits(t.Context(), session.ID, true); err != nil {
				t.Fatal(err)
			}
			newPage := func() (*NativePage, *ui.Tester) {
				t.Helper()
				page := NewNativePage(app.Service)
				page.ctx, page.selected = t.Context(), session.ID
				page.detail, err = app.Store.Detail(t.Context(), session.ID)
				if err != nil {
					t.Fatal(err)
				}
				page.applyDetail()
				return page, ui.NewTester(page.Build, 1240, 818)
			}
			page, tester := newPage()
			turn, err := app.Service.Submit(t.Context(), session.ID, "Ask a real clarification; any later write needs its own approval")
			if err != nil {
				t.Fatal(err)
			}
			waitNativePage(t, tester, page, func() bool {
				return page.detail.Session.Status == engine.Waiting && !app.Runtime.SessionActive(session.ID)
			})
			closeApp()
			open()
			page, tester = newPage()
			click := func(label string) {
				t.Helper()
				// Operate inside the actual default content height. Clicking an
				// offscreen label is not evidence that a human can reach it.
				if page.pane == "waits" && label != "Continue this waiting Turn" && label != "Refresh waiting tasks" && label != "Waiting tasks" {
					for i := 0; i < 8; i++ {
						box, found := tester.Find(label)
						viewport, ok := tester.Find("Waiting task details")
						if !found || !ok {
							t.Fatal("wait control/scroll viewport missing", label)
						}
						dy := float32(0)
						if box.Y < viewport.Y {
							dy = box.Y - viewport.Y - 8
						} else if box.Y+box.H > viewport.Y+viewport.H {
							dy = box.Y + box.H - viewport.Y - viewport.H + 8
						} else {
							break
						}
						tester.Scroll(viewport.X+viewport.W-12, viewport.Y+20, 0, dy)
					}
				}
				box, found := tester.Find(label)
				if !found || box.Y < 0 || box.Y+box.H > 818 {
					t.Fatal("control remains outside the default viewport", label, box)
				}
				if err := tester.Click(label); err != nil {
					t.Fatal(label, err)
				}
			}
			click("Waiting tasks")
			settlePage(t, tester, page)
			if calls.Load() != 1 || !tester.HasText("Choose the task input explicitly") || tester.HasText("Explicit result JSON (at most 16 KiB)") {
				t.Fatal("cold question needs manual JSON or restarted execution", calls.Load(), page.wait)
			}
			before := page.wait.View
			click("Save clarification without executing") // No default or implicit blank answer.
			var expected interaction.Answer
			switch kind {
			case "single", "expired":
				click("1. First choice")
				click("2. Second choice")
				expected.Selected = []int{2}
			case "multiple":
				click("1. First choice")
				click("3. Third choice")
				click("✓ 1. First choice")
				click("✓ 3. Third choice")
				click("Save clarification without executing") // An empty selection cannot be submitted.
				click("3. Third choice")
				click("1. First choice")
				expected.Selected = []int{1, 3}
			case "text":
				click("Clarification answer · Saved clarification")
				tester.Type(strings.Repeat("<", 3000)) // JSON escaping exceeds 16 KiB even though UTF-8 text does not.
				click("Save clarification without executing")
				if !tester.HasText("Answer exceeds the 16 KiB saved JSON limit.") {
					t.Fatal("encoded answer limit was not explained")
				}
				click("Clarification answer · Saved clarification")
				tester.Key(ui.Cmd, ui.KeyA)
				text := "保留原始答案：\"确认\"、\\路径；这不授予写入权限。"
				tester.Type(text)
				expected.Text = &text
			}
			unchanged, err := app.Service.Waits(t.Context(), session.ID)
			if err != nil || calls.Load() != 1 || unchanged.Snapshot.Revision != before.Snapshot.Revision || len(unchanged.Snapshot.State.Requests[0].Result) != 0 {
				t.Fatal("editing/default/invalid submission changed facts or executed", unchanged, calls.Load(), err)
			}
			if kind == "expired" {
				// Arrange a genuinely expired stored request under the supplied
				// repository's exclusive claim, without waiting for its 24h TTL.
				branch, err := product.ActiveBranch(page.detail)
				if err != nil {
					t.Fatal(err)
				}
				func() {
					_, release, err := app.Repository.BeginRun(t.Context(), session.ID, branch.NativeSessionID)
					if err != nil {
						t.Fatal(err)
					}
					defer release()
					backend, err := journal.Open(t.Context(), app.Repository, session.ID, branch.NativeSessionID)
					if err != nil {
						t.Fatal(err)
					}
					snapshot, err := backend.Load(t.Context())
					if err != nil {
						t.Fatal(err)
					}
					snapshot.State.Requests[0].ExpiresAt = time.Now().Add(-time.Minute)
					if _, err = backend.Commit(t.Context(), snapshot.Revision, snapshot.State, nil); err != nil {
						t.Fatal(err)
					}
				}()
				page.poll()
				waitNativePage(t, tester, page, func() bool { return page.wait.Current && page.wait.View.Snapshot.Revision > before.Snapshot.Revision })
				draft := page.wait.Answers[page.wait.View.Snapshot.State.Requests[0].ID]
				revision := page.wait.View.Snapshot.Revision
				click("1. First choice")
				click("Save clarification without executing")
				click("Continue this waiting Turn")
				if !tester.HasText("Request expired.") || page.paneBusy || page.wait.View.CanResume || page.wait.View.Snapshot.Revision != revision || page.wait.Answers[page.wait.View.Snapshot.State.Requests[0].ID] != draft || calls.Load() != 1 || app.Runtime.SessionActive(session.ID) {
					t.Fatal("expired question admitted an answer or execution", page.wait, calls.Load())
				}
				return
			}
			page.poll()
			waitNativePage(t, tester, page, func() bool { return page.wait.Current })
			click("Save clarification without executing")
			settlePage(t, tester, page)
			var saved interaction.Answer
			view := page.wait.View
			if err = json.Unmarshal(view.Snapshot.State.Requests[0].Result, &saved); err != nil || !reflect.DeepEqual(saved, expected) || !view.CanResume || page.problem != "" || calls.Load() != 1 {
				t.Fatal("typed save changed answer or executed work", saved, expected, view, calls.Load(), err, page.problem)
			}
			closeApp()
			open()
			page, tester = newPage()
			click("Waiting tasks")
			settlePage(t, tester, page)
			if calls.Load() != 1 || !page.wait.View.CanResume || tester.HasText("Save clarification without executing") {
				t.Fatal("saved answer was lost, editable or executed on reopen", page.wait, calls.Load())
			}
			click("Continue this waiting Turn")
			waitNativePage(t, tester, page, func() bool {
				return page.detail.Session.Status == engine.Waiting && !app.Runtime.SessionActive(session.ID) && page.wait.Current && page.wait.View.Snapshot.Revision > view.Snapshot.Revision && page.wait.View.Snapshot.State.Status == "waiting"
			})
			if calls.Load() != 3 || page.wait.View.TurnID != turn || len(page.wait.Answers) != 0 || len(page.wait.View.Snapshot.State.Requests) != 1 || page.wait.View.Snapshot.State.Requests[0].Kind != "approval" {
				t.Fatal("answer was asked again, leaked into the next request or resumed a new Turn", page.wait, calls.Load())
			}
			if _, err = os.Stat(filepath.Join(workspace, "requires-separate-approval.txt")); !os.IsNotExist(err) {
				t.Fatal("a clarification authorized the later write", err)
			}
		})
	}
}

func TestWaitQuestionDraftAndFocusStayWithRequestWhenAnotherDisappears(t *testing.T) {
	service, requests := fixture(t)
	page := NewNativePage(service)
	// Presentation-only snapshots: no save or backend mutation is performed.
	question := func(id string) durable.Request {
		payload, _ := json.Marshal(interaction.Question{ID: "source-" + id, SessionID: "native", RunID: "run", CallID: "call-" + id, Header: id, Prompt: "Explicit text for " + id})
		return durable.Request{ID: id, Kind: "question", RunID: "run", CallID: "call-" + id, Binding: "binding-" + id, Payload: payload, ExpiresAt: time.Now().Add(time.Hour)}
	}
	first, second := question("first"), question("second")
	view := product.WaitView{Status: engine.Waiting, Supported: true, Enabled: true, BranchID: "branch", TurnID: "turn", Snapshot: durable.Snapshot{Revision: 1, State: durable.State{Version: 1, Status: durable.Waiting, Requests: []durable.Request{first, second}}}}
	page.applyWaitView(view)
	tester := ui.NewTester(func(c *ui.Context) { ui.Column(c).Fill().Children(func() { page.buildWaits(c) }) }, 1240, 1600)
	if err := tester.Click("Clarification answer · first"); err != nil {
		t.Fatal(err)
	}
	tester.Type("first draft")
	if err := tester.Click("Clarification answer · second"); err != nil {
		t.Fatal(err)
	}
	tester.Type("second draft")
	view.Snapshot.Revision++
	view.Snapshot.State.Requests = []durable.Request{second}
	page.applyWaitView(view)
	tester.Frame()
	tester.Type(" continued") // Keep typing without refocusing after a sibling is removed.
	var answer interaction.Answer
	if err := json.Unmarshal([]byte(page.wait.Answers[second.ID]), &answer); err != nil || answer.Text == nil || *answer.Text != "second draft continued" || len(page.wait.Answers) != 1 || service.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("answer/focus moved to another request or editing executed work", page.wait.Answers, err)
	}
}
