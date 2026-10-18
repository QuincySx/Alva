// INPUT: Real local HTTP model/SDK shell loops, authenticated product routes and application-owned processes
// OUTPUT: Two-turn background survival, same-session read, foreign Stop denial and physical operator shutdown regression
// POS: [L7 Hub product] Acceptance runs only fixture cat; it never discovers a user's shell/session or paid account.
package server

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/go/apps/hub"
	"github.com/QuincySx/Alva/go/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/go/apps/hub/product"
	"github.com/QuincySx/Alva/go/integration/llm/provider"
)

func TestHTTPShellJobSurvivesTwoActualTurnsAndOperatorStopReaps(t *testing.T) {
	var calls atomic.Int32
	var ownedID atomic.Value
	ownedID.Store("")
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		finish := "stop"
		delta := map[string]any{"content": "background remains under explicit application ownership"}
		switch calls.Add(1) {
		case 1:
			finish = "tool_calls"
			delta = map[string]any{"tool_calls": []any{map[string]any{"index": 0, "id": "start-owned-cat", "type": "function", "function": map[string]any{"name": "execute_shell", "arguments": `{"command":"cat","run_in_background":true,"timeout_ms":60000}`}}}}
		case 3:
			args, _ := json.Marshal(map[string]string{"job_id": ownedID.Load().(string), "stdin": "cross-turn approved input\n"})
			finish = "tool_calls"
			delta = map[string]any{"tool_calls": []any{map[string]any{"index": 0, "id": "write-across-turn", "type": "function", "function": map[string]any{"name": "write_shell_stdin", "arguments": string(args)}}}}
		case 4:
			args, _ := json.Marshal(map[string]string{"job_id": ownedID.Load().(string)})
			finish = "tool_calls"
			delta = map[string]any{"tool_calls": []any{map[string]any{"index": 0, "id": "read-across-turn", "type": "function", "function": map[string]any{"name": "read_shell_output", "arguments": string(args)}}}}
		}
		data, _ := json.Marshal(map[string]any{"id": "session-shell-fixture", "choices": []any{map[string]any{"index": 0, "delta": delta, "finish_reason": finish}}})
		w.Header().Set("Content-Type", "text/event-stream")
		fmt.Fprintf(w, "data: %s\n\ndata: [DONE]\n\n", data)
	}))
	defer upstream.Close()
	f := newFixture(t, bootstrap.Config{ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: upstream.URL, Model: "fixture"}})
	session := create(t, f, "cross-turn shell")
	prefix := "/v1/sessions/" + session.ID
	if err := f.app.Store.ReplaceTools(t.Context(), session.ID, hub.ToolSettings{Shell: hub.ShellSettings{Executable: "/bin/sh"}}); err != nil {
		t.Fatal(err)
	}
	body(t, f.request(t, "PUT", prefix+"/settings/tools/shell", []byte(`{"enabled":true,"access_approved":true}`), nil), 200)
	body(t, f.request(t, "POST", prefix+"/turns", []byte(`{"prompt":"start owned background cat"}`), nil), 202)
	approval := httpApproval(t, f, session.ID)
	body(t, f.request(t, "POST", prefix+"/approvals/"+approval.ID+"/decision", []byte(`{"allow":true}`), nil), 200)
	waitIdle(t, f, session.ID)
	var jobs []product.ShellJob
	if json.Unmarshal(body(t, f.request(t, "GET", prefix+"/shell-jobs", nil, nil), 200), &jobs) != nil || len(jobs) != 1 || jobs[0].Job.Status.Reaped {
		t.Fatal("successful Turn dropped live owner", jobs)
	}
	first := jobs[0]
	blocked, cancel := context.WithTimeout(t.Context(), 25*time.Millisecond)
	if err := f.app.Runtime.WithExclusiveWorkspaces(blocked, []string{session.Workspace}, func(context.Context) error {
		t.Error("exclusive mutation entered live background workspace")
		return nil
	}); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal("background dropped product workspace retention", err)
	}
	cancel()
	ownedID.Store(first.Job.Status.Task.ID)
	body(t, f.request(t, "POST", prefix+"/turns", []byte(`{"prompt":"read previous Turn's background job"}`), nil), 202)
	approval = httpApproval(t, f, session.ID)
	if approval.ToolName != "write_shell_stdin" {
		t.Fatal("second attempt did not use fresh call approval", approval)
	}
	body(t, f.request(t, "POST", prefix+"/approvals/"+approval.ID+"/decision", []byte(`{"allow":true}`), nil), 200)
	waitIdle(t, f, session.ID)
	if calls.Load() != 5 {
		t.Fatal("second actual SDK loop failed to return owned output metadata", calls.Load())
	}
	other := create(t, f, "foreign shell owner")
	stop, _ := json.Marshal(map[string]any{"action": "stop", "branch": first.BranchID, "job": first.Job.Status.Task.ID})
	body(t, f.request(t, "POST", "/v1/sessions/"+other.ID+"/shell-jobs", stop, nil), 409)
	body(t, f.request(t, "POST", prefix+"/shell-jobs", stop, nil), 200)
	if json.Unmarshal(body(t, f.request(t, "GET", prefix+"/shell-jobs", nil, nil), 200), &jobs) != nil || len(jobs) != 1 || !jobs[0].Job.Status.Reaped {
		t.Fatal("Stop acknowledged before physical cleanup", jobs)
	}
	if err := f.app.Runtime.WithExclusiveWorkspaces(t.Context(), []string{session.Workspace}, func(context.Context) error { return nil }); err != nil {
		t.Fatal("physical Stop failed to release product workspace", err)
	}
	reset, _ := json.Marshal(map[string]any{"action": "reset", "branch": first.BranchID, "approved": true})
	body(t, f.request(t, "POST", prefix+"/shell-jobs", reset, nil), 200)
	if !f.app.Shells.CleanupComplete() {
		t.Fatal("application still owns unfinished shell cleanup")
	}
	detail, err := f.app.Store.Detail(context.Background(), session.ID)
	if err != nil {
		t.Fatal(err)
	}
	branch, _ := productBranch(detail)
	log, err := f.app.Repository.OpenSession(t.Context(), session.ID, branch.NativeSessionID)
	if err != nil {
		t.Fatal(err)
	}
	records, err := log.Records(t.Context(), 0)
	if err != nil {
		t.Fatal(err)
	}
	starts, outcomes := 0, 0
	for _, r := range records {
		if r.Namespace == "alva.shell" && r.Type == "job_intent" {
			starts++
		}
		if r.Namespace == "alva.shell" && r.Type == "job_outcome" {
			outcomes++
		}
	}
	if starts != 1 || outcomes != 1 {
		t.Fatal("job replayed or outcome missing", starts, outcomes)
	}
}
