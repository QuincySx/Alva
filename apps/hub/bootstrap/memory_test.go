// INPUT: Real local model HTTP, explicit memory opt-in, actual HITL and private native/product/memory stores
// OUTPUT: Cross-restart memory continuity, reference-only delivery, no scan and workspace isolation regressions
// POS: [L7 Hub product] End-to-end optional capability test without embeddings services or user-owned project data
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

	"github.com/QuincySx/Alva/integration/llm/provider"
)

func TestExplicitPersistentMemorySurvivesRestartWithoutScanningOrCrossWorkspaceRead(t *testing.T) {
	const body = "uniqueneedle actual stored memory body"
	var calls atomic.Int32
	results := make(chan string, 4)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var input struct {
			Messages []struct {
				Role    string
				Content json.RawMessage
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
			results <- string(last.Content)
			delta = map[string]any{"content": "explicit memory operation completed"}
		} else {
			name, args := "memory_search", `{"query":"uniqueneedle","limit":5}`
			if strings.Contains(string(last.Content), "STORE-MEMORY") {
				name = "memory_store"
				encoded, _ := json.Marshal(map[string]string{"key": "explicit-key", "content": body, "category": "project"})
				args = string(encoded)
			}
			delta = map[string]any{"tool_calls": []any{map[string]any{"index": 0, "id": "memory-call", "type": "function", "function": map[string]any{"name": name, "arguments": args}}}}
			finish = "tool_calls"
		}
		data, _ := json.Marshal(map[string]any{"id": fmt.Sprintf("memory-fixture-%d", calls.Add(1)), "choices": []any{map[string]any{"index": 0, "delta": delta, "finish_reason": finish}}})
		w.Header().Set("Content-Type", "text/event-stream")
		fmt.Fprintf(w, "data: %s\n\ndata: [DONE]\n\n", data)
	}))
	defer server.Close()
	ctx := context.Background()
	config := Config{OpenStores: testStores, OpenMemory: LocalMemory, OpenBindings: LocalBindings, DataDirectory: filepath.Join(t.TempDir(), "private"), ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: server.URL, Model: "fixture"}}
	a, err := Open(ctx, config)
	if err != nil {
		t.Fatal(err)
	}
	defer closeApp(t, a)
	session, err := a.Service.Create(ctx, "persistent memory", "")
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(session.Workspace, "MEMORY.md"), []byte("neverautoscan secret file body"), 0600); err != nil {
		t.Fatal(err)
	}
	settings, err := a.Store.Settings(ctx, session.ID)
	if err != nil || settings.PersistentMemory {
		t.Fatal("persistent memory enabled by default", err)
	}
	if _, err = os.Stat(filepath.Join(config.DataDirectory, "memory")); !os.IsNotExist(err) {
		t.Fatal("startup created optional memory authority", err)
	}
	if err = a.Store.SetPersistentMemory(ctx, session.ID, true); err != nil {
		t.Fatal(err)
	}
	if calls.Load() != 0 {
		t.Fatal("preference executed a model")
	}
	if _, err = a.Service.Submit(ctx, session.ID, "STORE-MEMORY"); err != nil {
		t.Fatal(err)
	}
	request := nextApproval(t, a, session.ID)
	if request.ToolName != "memory_store" {
		t.Fatal("unexpected actual memory permission", request.ToolName)
	}
	if err = a.Store.SetPersistentMemory(ctx, session.ID, false); err == nil {
		t.Fatal("active memory recipe changed")
	}
	if err = a.Service.RespondApproval(ctx, session.ID, request.ID, true); err != nil {
		t.Fatal(err)
	}
	awaitTerminal(t, a, session.ID)
	backend, err := a.workspaceMemory(ctx, session.Workspace)
	if err != nil {
		t.Fatal(err)
	}
	entries, err := backend.FTSSearch(ctx, "uniqueneedle", 5)
	if err != nil || len(entries) != 1 {
		t.Fatal("actual approved memory was not persisted", err, len(entries))
	}
	entries, err = backend.FTSSearch(ctx, "neverautoscan", 5)
	if err != nil || len(entries) != 0 {
		t.Fatal("constructor or turn automatically scanned files", err)
	}
	closeApp(t, a)
	b, err := Open(ctx, config)
	if err != nil {
		t.Fatal(err)
	}
	defer closeApp(t, b)
	if _, err = b.Service.Submit(ctx, session.ID, "SEARCH-MEMORY"); err != nil {
		t.Fatal(err)
	}
	awaitTerminal(t, b, session.ID)
	backend, err = b.workspaceMemory(ctx, session.Workspace)
	if err != nil {
		t.Fatal(err)
	}
	entries, err = backend.FTSSearch(ctx, "uniqueneedle", 5)
	if err != nil || len(entries) != 1 {
		t.Fatal("workspace memory lost across app restart", err)
	}
	other, err := b.workspaceMemory(ctx, t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	entries, err = other.FTSSearch(ctx, "uniqueneedle", 5)
	if err != nil || len(entries) != 0 {
		t.Fatal("memory namespace crossed workspace boundary", err)
	}
	if calls.Load() != 4 || len(results) != 2 {
		t.Fatal("unexpected model replay", calls.Load(), len(results))
	}
	for range 2 {
		if strings.Contains(<-results, body) {
			t.Fatal("raw memory body automatically injected instead of reference")
		}
	}
}
