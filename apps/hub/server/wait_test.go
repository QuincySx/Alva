// INPUT: Authenticated loopback HTTP, actual persistent Hub bootstrap and local SSE waits.
// OUTPUT: Cold reads, strict ownership/revision, result-only writes and explicit same-Turn continuation acceptance.
// POS: [L7 transport acceptance] Exercises the real Agent/controller; HTTP never owns or automatically resumes execution.
package server

import (
	"bytes"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/harness/durable"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/llm/provider"
)

func httpWaitView(t *testing.T, f *fixture, id string, status engine.Outcome) product.WaitView {
	t.Helper()
	deadline := time.Now().Add(8 * time.Second)
	for time.Now().Before(deadline) {
		var v product.WaitView
		if err := json.Unmarshal(body(t, f.request(t, "GET", "/v1/sessions/"+id+"/waits", nil, nil), 200), &v); err != nil {
			t.Fatal(err)
		}
		if v.Status == status && !f.app.Runtime.SessionActive(id) {
			return v
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("HTTP owner did not settle to", status)
	return product.WaitView{}
}
func TestHTTPDurableWaitResolutionRequiresExplicitSameTurnResume(t *testing.T) {
	var calls atomic.Int32
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		n := calls.Add(1)
		if n == 2 {
			var raw json.RawMessage
			if err := json.NewDecoder(r.Body).Decode(&raw); err != nil || !bytes.Contains(raw, []byte("actual-http-result")) {
				t.Error("stored result was not delivered", string(raw), err)
			}
		}
		delta := map[string]any{"content": "actual HTTP continuation"}
		finish := "stop"
		if n%2 == 1 {
			delta = map[string]any{"tool_calls": []any{map[string]any{"index": 0, "id": "http-wait", "type": "function", "function": map[string]string{"name": "await_external", "arguments": `{"request":{"job":"http-example"}}`}}}}
			finish = "tool_calls"
		}
		data, _ := json.Marshal(map[string]any{"choices": []any{map[string]any{"index": 0, "delta": delta, "finish_reason": finish}}})
		w.Header().Set("Content-Type", "text/event-stream")
		fmt.Fprintf(w, "data: %s\n\ndata: [DONE]\n\n", data)
	}))
	defer upstream.Close()
	f := newFixture(t, bootstrap.Config{OpenStores: bootstrap.LocalStores, ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: upstream.URL, Model: "fixture"}})
	s := create(t, f, "HTTP waiting")
	prefix := "/v1/sessions/" + s.ID
	for _, tc := range []struct {
		method, path string
		data         []byte
		headers      map[string]string
		status       int
	}{
		{"GET", prefix + "/waits", nil, map[string]string{"Authorization": ""}, 401},
		{"GET", prefix + "/waits", nil, map[string]string{"Origin": "https://foreign.example"}, 403},
		{"GET", prefix + "/waits?unknown=1", nil, nil, 400},
		{"POST", prefix + "/waits", []byte(`{}`), nil, 405},
		{"PUT", prefix + "/waits-enabled", []byte(`{"Enabled":true}`), nil, 400},
		{"PUT", prefix + "/waits-enabled", []byte(`{"enabled":true,"enabled":false}`), nil, 400},
		{"PUT", prefix + "/waits-enabled", []byte(`{"enabled":null}`), nil, 400},
	} {
		body(t, f.request(t, tc.method, tc.path, tc.data, tc.headers), tc.status)
	}
	body(t, f.request(t, "PUT", prefix+"/waits-enabled", []byte(`{"enabled":true}`), nil), 200)
	var cold product.WaitView
	if err := json.Unmarshal(body(t, f.request(t, "GET", prefix+"/waits", nil, nil), 200), &cold); err != nil || !cold.Enabled || cold.Snapshot.State.Status != durable.Idle || calls.Load() != 0 {
		t.Fatal(cold, calls.Load(), err)
	}
	var admitted map[string]string
	if err := json.Unmarshal(body(t, f.request(t, "POST", prefix+"/turns", []byte(`{"prompt":"one HTTP input"}`), nil), 202), &admitted); err != nil {
		t.Fatal(err)
	}
	v := httpWaitView(t, f, s.ID, engine.Waiting)
	if len(v.Snapshot.State.Requests) != 1 || v.CanResume || calls.Load() != 1 || v.TurnID != admitted["turn_id"] {
		t.Fatal(v, admitted, calls.Load())
	}
	target := product.WaitTarget{BranchID: v.BranchID, TurnID: v.TurnID, Revision: v.Snapshot.Revision}
	targetData, _ := json.Marshal(target)
	body(t, f.request(t, "POST", prefix+"/wait-resume", targetData, nil), 409)
	body(t, f.request(t, "POST", prefix+"/turns", []byte(`{"prompt":"cannot bypass"}`), nil), 409)
	body(t, f.request(t, "PUT", prefix+"/waits-enabled", []byte(`{"enabled":false}`), nil), 409)
	body(t, f.request(t, "POST", prefix+"/wait-resume", []byte(`{"branch_id":"x","turn_id":"x","revision":1,"prompt":"replay"}`), nil), 400)
	body(t, f.request(t, "POST", prefix+"/wait-resolve", []byte(`{"target":{"branch_id":"x","branch_id":"y","turn_id":"x","revision":1},"request_id":"x","binding":"x","result":{}}`), nil), 400)
	r := v.Snapshot.State.Requests[0]
	answer := product.WaitResolution{Target: target, RequestID: r.ID, Binding: r.Binding, Result: json.RawMessage(`{"value":"actual-http-result"}`)}
	answerData, _ := json.Marshal(answer)
	foreign := create(t, f, "foreign wait target")
	body(t, f.request(t, "POST", "/v1/sessions/"+foreign.ID+"/wait-resolve", answerData, nil), 409)
	body(t, f.request(t, "POST", prefix+"/wait-resolve", answerData, nil), 200)
	if calls.Load() != 1 || f.app.Runtime.SessionActive(s.ID) {
		t.Fatal("supplying a result executed work")
	}
	body(t, f.request(t, "POST", prefix+"/wait-resolve", answerData, nil), 409)
	body(t, f.request(t, "POST", prefix+"/wait-resume", targetData, nil), 409)
	v = httpWaitView(t, f, s.ID, engine.Waiting)
	if !v.CanResume {
		t.Fatal(v)
	}
	target.Revision = v.Snapshot.Revision
	targetData, _ = json.Marshal(target)
	var resumed map[string]string
	if err := json.Unmarshal(body(t, f.request(t, "POST", prefix+"/wait-resume", targetData, nil), 202), &resumed); err != nil || resumed["turn_id"] != admitted["turn_id"] {
		t.Fatal(resumed, err)
	}
	httpWaitView(t, f, s.ID, engine.Succeeded)
	if calls.Load() != 2 {
		t.Fatal("continuation replayed model requests", calls.Load())
	}
	history, err := f.app.Store.History(t.Context(), s.ID, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	inputs, waits, success := 0, 0, 0
	for _, e := range history {
		if e.Input != "" {
			inputs++
		}
		if e.Event.Kind == engine.Completed {
			if e.Event.Outcome == engine.Waiting {
				waits++
			}
			if e.Event.Outcome == engine.Succeeded {
				success++
			}
		}
	}
	if inputs != 1 || waits != 1 || success != 1 {
		t.Fatal(inputs, waits, success)
	}
	body(t, f.request(t, "POST", prefix+"/turns", []byte(`{"prompt":"new explicit input"}`), nil), 202)
	v = httpWaitView(t, f, s.ID, engine.Waiting)
	target = product.WaitTarget{BranchID: v.BranchID, TurnID: v.TurnID, Revision: v.Snapshot.Revision}
	targetData, _ = json.Marshal(target)
	body(t, f.request(t, "POST", prefix+"/wait-abandon", targetData, nil), 200)
	v = httpWaitView(t, f, s.ID, engine.Interrupted)
	if v.Snapshot.State.Status != durable.Idle || calls.Load() != 3 {
		t.Fatal("abandonment executed/replayed work", v, calls.Load())
	}
}
