// INPUT: Real HTTP provider/tool loop and authenticated child approvals/typed Inspector routes
// OUTPUT: Native child HITL, owner isolation and explicit bounded ancestry-only body access regressions
// POS: [L7 Hub product] Recursive transport acceptance cannot promote child logs or permissions into the parent model
package server

import (
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/go/apps/hub"
	bootstrap "github.com/QuincySx/Alva/go/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/go/apps/hub/product"
	"github.com/QuincySx/Alva/go/integration/llm/provider"
	"github.com/QuincySx/Alva/go/sdk/agent/security"
)

func httpApproval(t *testing.T, f *fixture, id string) security.ApprovalRequest {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		var pending []security.ApprovalRequest
		if json.Unmarshal(body(t, f.request(t, "GET", "/v1/sessions/"+id+"/approvals", nil, nil), 200), &pending) != nil {
			t.Fatal("invalid approval index")
		}
		if len(pending) > 0 {
			return pending[0]
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("missing actual approval")
	return security.ApprovalRequest{}
}

func TestHTTPNativeChildApprovalAndColdTypedInspector(t *testing.T) {
	var calls atomic.Int32
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var input struct {
			Messages []struct {
				Role       string          `json:"role"`
				Content    json.RawMessage `json:"content"`
				ToolCallID string          `json:"tool_call_id"`
			} `json:"messages"`
		}
		if json.NewDecoder(r.Body).Decode(&input) != nil || len(input.Messages) == 0 {
			http.Error(w, "fixture request invalid", 400)
			return
		}
		last := input.Messages[len(input.Messages)-1]
		delta := map[string]any{"content": "actual final response"}
		finish := "stop"
		if last.Role != "tool" {
			name, callID, args := "agent", "root-spawn", `{"task":"HTTP-CHILD-WRITE","tools":["write_file"],"timeout_ms":30000,"max_iterations":4}`
			if strings.Contains(string(last.Content), "HTTP-CHILD-WRITE") {
				name, callID, args = "write_file", "child-write", `{"path":"http-child.txt","content":"actual child file"}`
			}
			delta = map[string]any{"tool_calls": []any{map[string]any{"index": 0, "id": callID, "type": "function", "function": map[string]any{"name": name, "arguments": args}}}}
			finish = "tool_calls"
		}
		data, _ := json.Marshal(map[string]any{"id": fmt.Sprint(calls.Add(1)), "choices": []any{map[string]any{"index": 0, "delta": delta, "finish_reason": finish}}})
		w.Header().Set("Content-Type", "text/event-stream")
		_, _ = fmt.Fprintf(w, "data: %s\n\ndata: [DONE]\n\n", data)
	}))
	defer upstream.Close()
	f := newFixture(t, bootstrap.Config{ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: upstream.URL, Model: "fixture"}})
	root, foreign := create(t, f, "child test"), create(t, f, "other")
	prefix := "/v1/sessions/" + root.ID
	body(t, f.request(t, "PUT", prefix+"/settings/subagents", []byte(`{"enabled":true}`), nil), 200)
	if calls.Load() != 0 {
		t.Fatal("settings started model")
	}
	body(t, f.request(t, "POST", prefix+"/turns", []byte(`{"prompt":"delegate a file write"}`), nil), 202)
	spawn := httpApproval(t, f, root.ID)
	if spawn.ToolName != "agent" {
		t.Fatal("missing actual root spawn consent")
	}
	body(t, f.request(t, "POST", prefix+"/approvals/"+spawn.ID+"/decision", []byte(`{"allow":true}`), nil), 200)
	child := httpApproval(t, f, root.ID)
	if child.SessionID == spawn.SessionID || child.ToolName != "write_file" {
		t.Fatal("child identity or approval lost")
	}
	body(t, f.request(t, "POST", "/v1/sessions/"+foreign.ID+"/approvals/"+child.ID+"/decision", []byte(`{"allow":true}`), nil), 409)
	if _, err := os.Stat(filepath.Join(root.Workspace, "http-child.txt")); !os.IsNotExist(err) {
		t.Fatal("file existed before child approval")
	}
	body(t, f.request(t, "POST", prefix+"/approvals/"+child.ID+"/decision", []byte(`{"allow":true}`), nil), 200)
	waitIdle(t, f, root.ID)
	var detail hub.Detail
	if json.Unmarshal(body(t, f.request(t, "GET", prefix, nil, nil), 200), &detail) != nil {
		t.Fatal("invalid committed branch")
	}
	branch, err := product.ActiveBranch(detail)
	if err != nil {
		t.Fatal(err)
	}
	link, err := f.app.Repository.ParentLink(t.Context(), root.ID, child.SessionID)
	if err != nil {
		t.Fatal(err)
	}
	path := "&child_path=" + fmt.Sprint(link.Link.Sequence)
	query := "?branch=" + branch.ID
	var page product.InspectorPage
	if json.Unmarshal(body(t, f.request(t, "GET", prefix+"/inspector"+query+path, nil, nil), 200), &page) != nil || page.NativeID != child.SessionID || len(page.Entries) == 0 {
		t.Fatal("typed Inspector did not resolve actual child")
	}
	uri := page.Entries[0].Value.URI
	read := prefix + "/artifact" + query + "&uri=" + url.QueryEscape(uri)
	data := body(t, f.request(t, "GET", read+path, nil, nil), 200)
	if len(data) > 16<<10 {
		t.Fatal("unbounded child body")
	}
	body(t, f.request(t, "GET", read, nil, nil), 404)
	body(t, f.request(t, "GET", strings.Replace(read+path, root.ID, foreign.ID, 1), nil, nil), 409)
	for _, invalid := range []string{"0", "01", "-1", "1,", "1,1,1,1,1,1,1,1,1"} {
		body(t, f.request(t, "GET", prefix+"/inspector"+query+"&child_path="+url.QueryEscape(invalid), nil, nil), 400)
	}
	if calls.Load() != 4 || f.app.Runtime.HasActive() {
		t.Fatal("cold Inspector caused replay", calls.Load())
	}
	childBundle := body(t, f.request(t, "GET", prefix+"/inspector-bundle"+query+path, nil, nil), 200)
	var manifest struct {
		RootID string `json:"root_id"`
	}
	first := strings.SplitN(string(childBundle), "\n", 2)[0]
	if json.Unmarshal([]byte(first), &manifest) != nil || manifest.RootID != child.SessionID {
		t.Fatal("export did not preserve actual typed child target")
	}
	rootBundle := body(t, f.request(t, "GET", prefix+"/inspector-bundle"+query+"&include_children=true", nil, nil), 200)
	var completion struct {
		Sessions int `json:"sessions"`
	}
	lines := strings.Split(strings.TrimSpace(string(rootBundle)), "\n")
	if json.Unmarshal([]byte(lines[len(lines)-1]), &completion) != nil || completion.Sessions != 2 || calls.Load() != 4 {
		t.Fatal("recursive export scope or no-replay guarantee lost")
	}
}
