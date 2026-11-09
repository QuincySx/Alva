// INPUT: Authenticated HTTP requests against a real product service with the demo Alva engine
// OUTPUT: Queue, interject, edit, regenerate, version and fork route regressions including refusals and bad input
// POS: [L7 Hub product] The HTTP entry reaches the running-time and version operations only through the narrow Turns/Conversations services
package server

import (
	"encoding/json"
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	bootstrap "github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/integration/engine"
)

func demoFixture(t *testing.T) *fixture {
	t.Helper()
	return newFixture(t, bootstrap.Config{OpenStores: bootstrap.LocalStores, OpenMemory: bootstrap.LocalMemory, OpenBindings: bootstrap.LocalBindings, Demo: true})
}

// pausedSession is a conversation whose last Turn was stopped: its queue
// pauses, so queued messages wait for an explicit send.
func pausedSession(t *testing.T, f *fixture, title string) hub.Session {
	t.Helper()
	s := create(t, f, title)
	claim, err := f.app.Store.BeginTurn(t.Context(), s.ID, "stopped turn")
	if err != nil {
		t.Fatal(err)
	}
	if err = f.app.Store.FinishTurn(t.Context(), claim.Turn.ID, engine.Cancelled); err != nil {
		t.Fatal(err)
	}
	return s
}

func TestQueueOnAnIdleConversationStartsAtOnceAndAPausedOneKeepsWaiting(t *testing.T) {
	f := demoFixture(t)
	idle := create(t, f, "idle")
	body(t, f.request(t, "POST", "/v1/sessions/"+idle.ID+"/queue", []byte(`{"text":"send me now"}`), nil), 201)
	approveAndSettle(t, f, idle.ID)
	var queued struct{ Items []hub.PendingInput }
	if json.Unmarshal(body(t, f.request(t, "GET", "/v1/sessions/"+idle.ID+"/queue", nil, nil), 200), &queued) != nil || len(queued.Items) != 0 {
		t.Fatal("an idle conversation must start the message at once", queued)
	}
	if turn, err := f.app.Store.LatestTurn(t.Context(), idle.ID); err != nil || turn.Prompt != "send me now" {
		t.Fatal("the queued message became a Turn", turn.Prompt, err)
	}
	paused := pausedSession(t, f, "paused")
	body(t, f.request(t, "POST", "/v1/sessions/"+paused.ID+"/queue", []byte(`{"text":"wait for me"}`), nil), 201)
	if json.Unmarshal(body(t, f.request(t, "GET", "/v1/sessions/"+paused.ID+"/queue", nil, nil), 200), &queued) != nil || len(queued.Items) != 1 {
		t.Fatal("a paused conversation keeps the message queued", queued)
	}
	if f.app.Runtime.SessionActive(paused.ID) {
		t.Fatal("a paused queue must not start anything by itself")
	}
}

func TestQueueRoutes(t *testing.T) {
	f := demoFixture(t)
	s := pausedSession(t, f, "queue")
	path := "/v1/sessions/" + s.ID
	var queued struct{ Items []hub.PendingInput }
	if json.Unmarshal(body(t, f.request(t, "GET", path+"/queue", nil, nil), 200), &queued) != nil || len(queued.Items) != 0 {
		t.Fatal("a fresh queue is empty", queued)
	}
	var item hub.PendingInput
	if json.Unmarshal(body(t, f.request(t, "POST", path+"/queue", []byte(`{"text":"later"}`), nil), 201), &item) != nil || item.ID == "" || item.State != hub.PendingQueued {
		t.Fatal("queued input", item)
	}
	edited := body(t, f.request(t, "POST", path+"/queue-edit", []byte(fmt.Sprintf(`{"input_id":%q,"text":"later, edited"}`, item.ID)), nil), 200)
	if !strings.Contains(string(edited), "later, edited") {
		t.Fatal(string(edited))
	}
	for _, bad := range []struct{ route, payload string }{
		{"queue", `{"text":""}`}, {"queue", `{"text":"x","extra":1}`}, {"queue", `{"text":"a\u0000b"}`},
		{"queue-edit", `{"text":"x"}`}, {"queue-edit", `{"input_id":"bad id","text":"x"}`}, {"queue-cancel", `{}`},
		{"interject", `{"text":"x"}`}, {"edit", `{"prompt":"x"}`}, {"regenerate", `{}`}, {"version-select", `{}`}, {"fork", `{}`},
	} {
		if response := f.request(t, "POST", path+"/"+bad.route, []byte(bad.payload), nil); response.StatusCode != 400 {
			t.Fatalf("%s %s answered %d, want 400", bad.route, bad.payload, response.StatusCode)
		}
	}
	for _, c := range []struct{ method, route string }{{"GET", "interject"}, {"GET", "edit"}, {"DELETE", "queue"}, {"POST", "versions"}, {"GET", "fork"}} {
		if response := f.request(t, c.method, path+"/"+c.route, nil, nil); response.StatusCode != 405 {
			t.Fatalf("%s %s answered %d, want 405", c.method, c.route, response.StatusCode)
		}
	}
	// A queue full at eight refuses the ninth with a reason.
	for i := 1; i < hub.MaxPendingInputs; i++ {
		body(t, f.request(t, "POST", path+"/queue", []byte(`{"text":"more"}`), nil), 201)
	}
	body(t, f.request(t, "POST", path+"/queue", []byte(`{"text":"ninth"}`), nil), 409)
	body(t, f.request(t, "POST", path+"/queue-cancel", []byte(fmt.Sprintf(`{"input_id":%q}`, item.ID)), nil), 200)
	body(t, f.request(t, "POST", path+"/queue-cancel", []byte(fmt.Sprintf(`{"input_id":%q}`, item.ID)), nil), 409) // already gone
	body(t, f.request(t, "GET", "/v1/sessions/hub-missing/queue", nil, nil), 404)
	// Interjecting needs a running execution the caller observed.
	body(t, f.request(t, "POST", path+"/interject", []byte(`{"execution_id":"attempt-none","text":"now"}`), nil), 409)
}

func TestQueueSendRouteStartsTheHeadOfAPausedQueue(t *testing.T) {
	f := demoFixture(t)
	s := create(t, f, "paused")
	path := "/v1/sessions/" + s.ID
	claim, err := f.app.Store.BeginTurn(t.Context(), s.ID, "running turn")
	if err != nil {
		t.Fatal(err)
	}
	body(t, f.request(t, "POST", path+"/queue", []byte(`{"text":"resume with this"}`), nil), 201)
	body(t, f.request(t, "POST", path+"/queue-send", []byte(`{}`), nil), 409) // the Turn still runs
	if err = f.app.Store.FinishTurn(t.Context(), claim.Turn.ID, engine.Cancelled); err != nil {
		t.Fatal(err)
	}
	var started map[string]string
	if json.Unmarshal(body(t, f.request(t, "POST", path+"/queue-send", []byte(`{}`), nil), 202), &started) != nil || started["turn_id"] == "" || started["execution_id"] == "" {
		t.Fatal("queue-send must return the Turn and execution identity", started)
	}
	waitIdle(t, f, s.ID)
	var queued struct{ Items []hub.PendingInput }
	if json.Unmarshal(body(t, f.request(t, "GET", path+"/queue", nil, nil), 200), &queued) != nil || len(queued.Items) != 0 {
		t.Fatal("a sent input leaves the queue", queued)
	}
}

func TestVersionAndForkRoutes(t *testing.T) {
	f := demoFixture(t)
	s := create(t, f, "versions")
	path := "/v1/sessions/" + s.ID
	var first, second map[string]string
	for _, step := range []struct {
		prompt string
		into   *map[string]string
	}{{"alpha", &first}, {"bravo", &second}} {
		data, _ := json.Marshal(map[string]string{"prompt": step.prompt})
		if json.Unmarshal(body(t, f.request(t, "POST", path+"/turns", data, nil), 202), step.into) != nil {
			t.Fatal("turn")
		}
		approveAndSettle(t, f, s.ID)
	}
	var edited map[string]string
	data, _ := json.Marshal(map[string]string{"turn_id": second["turn_id"], "prompt": "bravo edited"})
	if json.Unmarshal(body(t, f.request(t, "POST", path+"/edit", data, nil), 202), &edited) != nil || edited["turn_id"] == "" {
		t.Fatal("edit must start a Turn", edited)
	}
	approveAndSettle(t, f, s.ID)
	var versions struct {
		Versions map[string]struct {
			Index, Count int
			Choices      []struct {
				LineID string `json:"LineID"`
				Active bool   `json:"Active"`
			}
		}
	}
	if json.Unmarshal(body(t, f.request(t, "GET", path+"/versions", nil, nil), 200), &versions) != nil {
		t.Fatal("versions")
	}
	switcher, ok := versions.Versions[edited["turn_id"]]
	if !ok || switcher.Count != 2 || switcher.Index != 2 || switcher.Choices[0].Active {
		t.Fatal("the edited message is version 2 of 2", versions)
	}
	data, _ = json.Marshal(map[string]string{"line_id": switcher.Choices[0].LineID})
	body(t, f.request(t, "POST", path+"/version-select", data, nil), 200)
	data, _ = json.Marshal(map[string]string{"line_id": "line-missing"})
	body(t, f.request(t, "POST", path+"/version-select", data, nil), 404)
	// The original version's last reply can be regenerated; a middle Turn cannot.
	data, _ = json.Marshal(map[string]string{"turn_id": first["turn_id"]})
	body(t, f.request(t, "POST", path+"/regenerate", data, nil), 409)
	data, _ = json.Marshal(map[string]string{"turn_id": second["turn_id"]})
	body(t, f.request(t, "POST", path+"/regenerate", data, nil), 202)
	approveAndSettle(t, f, s.ID)
	// Fork from the first Turn into a new conversation. That Turn is inherited by
	// the regenerated version's branch, so no reliable native anchor exists and
	// the fork is reported as degraded (the model sees a summary).
	data, _ = json.Marshal(map[string]string{"turn_id": first["turn_id"]})
	var forked struct {
		Session  hub.Session `json:"session"`
		Degraded bool        `json:"degraded"`
	}
	if json.Unmarshal(body(t, f.request(t, "POST", path+"/fork", data, nil), 201), &forked) != nil || forked.Session.ID == s.ID || forked.Session.ForkedFrom == nil || forked.Session.ForkedFrom.TurnID != first["turn_id"] || !forked.Degraded {
		t.Fatal("fork", forked)
	}
	data, _ = json.Marshal(map[string]string{"turn_id": "turn-missing"})
	body(t, f.request(t, "POST", path+"/fork", data, nil), 404)
	body(t, f.request(t, "POST", path+"/edit", mustJSON(map[string]string{"turn_id": "turn-missing", "prompt": "x"}), nil), 404)
}

func mustJSON(v any) []byte {
	data, _ := json.Marshal(v)
	return data
}

// approveAndSettle answers the demo echo tool's approval until the session ends.
func approveAndSettle(t *testing.T, f *fixture, id string) {
	t.Helper()
	for i := 0; i < 5000; i++ {
		requests, _ := f.app.Service.PendingApprovals(t.Context(), id)
		for _, request := range requests {
			_ = f.app.Service.RespondApproval(t.Context(), id, request.ID, true)
		}
		detail, err := f.app.Store.Detail(t.Context(), id)
		if err != nil {
			t.Fatal(err)
		}
		if detail.Session.Status != hub.Running && !f.app.Runtime.SessionActive(id) {
			if detail.Session.Status != engine.Succeeded {
				t.Fatalf("turn ended %s", detail.Session.Status)
			}
			return
		}
		sleepBriefly()
	}
	t.Fatal("turn never settled")
}

func sleepBriefly() { time.Sleep(2 * time.Millisecond) }
