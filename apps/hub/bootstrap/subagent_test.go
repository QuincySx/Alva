// INPUT: Shared native recipe, local streaming HTTP fixture and explicit real HITL decisions
// OUTPUT: Root/child approvals, actual file effect and cold typed Inspector isolation regression
// POS: [L7 Hub product] Full SDK-backed SubAgent product slice without paid models, external CLI or user credentials
package bootstrap

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/llm/provider"
	"github.com/QuincySx/Alva/sdk/agent/security"
)

func nextApproval(t *testing.T, a *Application, id string) security.ApprovalRequest {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		pending, err := a.Service.PendingApprovals(context.Background(), id)
		if err != nil {
			t.Fatal(err)
		}
		if len(pending) > 0 {
			return pending[0]
		}
		time.Sleep(time.Millisecond)
	}
	history, _ := a.Store.History(context.Background(), id, 0, 32)
	t.Fatalf("no actual pending approval: %+v", history)
	return security.ApprovalRequest{}
}
func TestOptInNativeChildHITLAndTypedColdInspector(t *testing.T) {
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
			http.Error(w, "bad fixture request", 400)
			return
		}
		last := input.Messages[len(input.Messages)-1]
		var delta map[string]any
		finish := "stop"
		if last.Role == "tool" {
			text := "ROOT-DONE"
			if last.ToolCallID == "child-write" {
				text = "CHILD-DONE"
			}
			delta = map[string]any{"content": text}
		} else {
			name, callID, args := "agent", "root-spawn", `{"task":"CHILD-WRITE","tools":["write_file"],"timeout_ms":30000,"max_iterations":4}`
			if strings.Contains(string(last.Content), "CHILD-WRITE") {
				name, callID, args = "write_file", "child-write", `{"path":"child-approved.txt","content":"real child effect"}`
			}
			delta = map[string]any{"tool_calls": []any{map[string]any{"index": 0, "id": callID, "type": "function", "function": map[string]any{"name": name, "arguments": args}}}}
			finish = "tool_calls"
		}
		data, _ := json.Marshal(map[string]any{"id": fmt.Sprintf("native-subagent-fixture-%d", calls.Add(1)), "choices": []any{map[string]any{"index": 0, "delta": delta, "finish_reason": finish}}})
		w.Header().Set("Content-Type", "text/event-stream")
		fmt.Fprintf(w, "data: %s\n\ndata: [DONE]\n\n", data)
	}))
	defer upstream.Close()
	ctx := context.Background()
	a, err := Open(ctx, Config{OpenStores: testStores, OpenMemory: LocalMemory, OpenBindings: LocalBindings, DataDirectory: filepath.Join(t.TempDir(), "private"), ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: upstream.URL, Model: "fixture"}})
	if err != nil {
		t.Fatal(err)
	}
	defer closeApp(t, a)
	p, err := a.Service.Create(ctx, "actual native child", "")
	if err != nil {
		t.Fatal(err)
	}
	settings, err := a.Store.Settings(ctx, p.ID)
	if err != nil || settings.SubagentsEnabled {
		t.Fatal("SubAgent was not opt-in", err)
	}
	if err = a.Store.SetSubagentsEnabled(ctx, p.ID, true); err != nil {
		t.Fatal(err)
	}
	if calls.Load() != 0 {
		t.Fatal("enabling capability contacted provider")
	}
	foreign, err := a.Service.Create(ctx, "foreign", "")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = a.Service.Submit(ctx, p.ID, "ROOT-DELEGATE"); err != nil {
		t.Fatal(err)
	}
	root := nextApproval(t, a, p.ID)
	if err = a.Store.SetSubagentsEnabled(ctx, p.ID, false); err == nil {
		t.Fatal("capability settings mutated active recipe")
	}
	if root.ToolName != "agent" {
		t.Fatal("missing actual spawn consent", root.ToolName)
	}
	if err = a.Service.RespondApproval(ctx, foreign.ID, root.ID, true); err == nil {
		t.Fatal("foreign product approved root spawn")
	}
	if err = a.Service.RespondApproval(ctx, p.ID, root.ID, true); err != nil {
		t.Fatal(err)
	}
	child := nextApproval(t, a, p.ID)
	if child.ToolName != "write_file" || child.SessionID == root.SessionID {
		t.Fatal("child approval hidden or identity rewritten", child.ToolName, child.SessionID)
	}
	if _, err = os.Stat(filepath.Join(p.Workspace, "child-approved.txt")); !os.IsNotExist(err) {
		t.Fatal("child write happened before child consent")
	}
	if err = a.Service.RespondApproval(ctx, p.ID, child.ID, true); err != nil {
		t.Fatal("root dispatch failed actual child response", err)
	}
	detail := awaitTerminal(t, a, p.ID)
	branch, err := product.ActiveBranch(detail)
	if err != nil {
		t.Fatal(err)
	}
	if data, err := os.ReadFile(filepath.Join(p.Workspace, "child-approved.txt")); err != nil || string(data) != "real child effect" {
		t.Fatal("approved actual child effect missing", err)
	}
	if calls.Load() != 4 {
		t.Fatalf("unexpected replay/model calls: %d", calls.Load())
	}
	link, err := a.Repository.ParentLink(ctx, p.ID, child.SessionID)
	if err != nil || link.ParentID != branch.NativeSessionID {
		t.Fatal("actual typed ancestry missing", err)
	}
	path := []product.ChildHop{{LinkSequence: link.Link.Sequence}}
	page, err := a.Service.InspectorAt(ctx, p.ID, branch.ID, path, "", 0, 0)
	if err != nil || page.NativeID != child.SessionID || len(page.Entries) == 0 {
		t.Fatal("typed child metadata unavailable", err)
	}
	uri := page.Entries[0].Value.URI
	text, err := a.Service.InspectorReadAt(ctx, p.ID, branch.ID, path, uri, 0)
	if err != nil || len(text.Text) > 2048 {
		t.Fatal("bounded child text failed", err)
	}
	parentRefs, err := a.Repository.BoundReferences(ctx, p.ID, branch.NativeSessionID)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = parentRefs.ReadText(ctx, uri, 0, 2048); err == nil {
		t.Fatal("Inspector granted child logs to parent model")
	}
	if _, err = a.Service.InspectorRead(ctx, p.ID, branch.ID, uri, 0); err == nil {
		t.Fatal("root Inspector read child body without typed path")
	}
	if _, err = a.Service.InspectorAt(ctx, foreign.ID, branch.ID, path, "", 0, 0); err == nil {
		t.Fatal("foreign product followed child link")
	}
	history, err := a.Store.History(ctx, p.ID, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	for _, entry := range history {
		if entry.Event.Kind == engine.ToolStart && entry.Event.Name == "write_file" {
			t.Fatal("child execution falsely flattened into root stream")
		}
	}
	if a.Runtime.HasActive() {
		t.Fatal("cold Inspector restored runtime")
	}
}
