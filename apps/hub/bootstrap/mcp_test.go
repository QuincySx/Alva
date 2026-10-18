// INPUT: Explicit same-binary MCP child, local model HTTP, Hub recipe and native SQLite authority.
// OUTPUT: Lazy MCP registration in the actual attempt resource owner, exact invocation and complete physical cleanup.
// POS: [L7 Hub product] Shared MCP assembly remains owned through SDK execution and application shutdown.
package bootstrap

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"sync/atomic"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	managedmcp "github.com/QuincySx/Alva/extension/mcp"
	"github.com/QuincySx/Alva/integration/llm/provider"
)

func TestNativeMCPResourceHelper(t *testing.T) {
	marker := os.Getenv("ALVA_MCP_RESOURCE_MARKER")
	if marker == "" {
		return
	}
	if err := os.WriteFile(marker, []byte("actual owned child started"), 0600); err != nil {
		os.Exit(2)
	}
	scanner := bufio.NewScanner(os.Stdin)
	for scanner.Scan() {
		var request struct {
			ID     json.RawMessage `json:"id"`
			Method string          `json:"method"`
		}
		if json.Unmarshal(scanner.Bytes(), &request) != nil {
			os.Exit(3)
		}
		var result any
		switch request.Method {
		case "initialize":
			result = map[string]any{"protocolVersion": "2025-11-25", "capabilities": map[string]any{"tools": map[string]any{}}, "serverInfo": map[string]string{"name": "owned-fixture", "version": "1"}}
		case "notifications/initialized":
			continue
		case "tools/list":
			result = map[string]any{"tools": []any{map[string]any{"name": "echo", "inputSchema": map[string]any{"type": "object"}}}}
		case "tools/call":
			result = map[string]any{"content": []any{map[string]string{"type": "text", "text": "actual MCP source tail"}}}
		default:
			os.Exit(4)
		}
		data, _ := json.Marshal(map[string]any{"jsonrpc": "2.0", "id": request.ID, "result": result})
		fmt.Println(string(data))
	}
	os.Exit(0)
}

func TestMCPActualAssemblyOwnsLazyWorkerUntilPhysicalCleanup(t *testing.T) {
	ctx := context.Background()
	var app *Application
	var captured atomic.Pointer[managedmcp.Plugin]
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var delta map[string]any
		finish := "stop"
		if calls.Add(1) == 1 {
			app.pendingMu.Lock()
			for runner := range app.pending {
				for _, resource := range runner.resources {
					if p, ok := resource.(*managedmcp.Plugin); ok {
						captured.Store(p)
					}
				}
			}
			app.pendingMu.Unlock()
			delta = map[string]any{"tool_calls": []any{map[string]any{"index": 0, "id": "mcp-call", "type": "function", "function": map[string]string{"name": "mcp_runtime", "arguments": `{"action":"call_tool","server_id":"fixture","tool_name":"echo","arguments":{}}`}}}}
			finish = "tool_calls"
		} else {
			delta = map[string]any{"content": "owned MCP fixture completed"}
		}
		data, _ := json.Marshal(map[string]any{"id": "fixture", "choices": []any{map[string]any{"index": 0, "delta": delta, "finish_reason": finish}}})
		w.Header().Set("Content-Type", "text/event-stream")
		fmt.Fprintf(w, "data: %s\n\ndata: [DONE]\n\n", data)
	}))
	defer server.Close()
	var err error
	app, err = Open(ctx, Config{DataDirectory: filepath.Join(t.TempDir(), "private"), ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: server.URL, Model: "fixture"}})
	if err != nil {
		t.Fatal(err)
	}
	defer closeApp(t, app)
	product, err := app.Service.Create(ctx, "owned MCP", "")
	if err != nil {
		t.Fatal(err)
	}
	exe, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	marker := filepath.Join(t.TempDir(), "started")
	if err = app.Store.ReplaceMCP(ctx, product.ID, []hub.MCPServer{{ID: "fixture", Command: exe, Args: []string{"-test.run=^TestNativeMCPResourceHelper$", "--"}, Env: map[string]string{"ALVA_MCP_RESOURCE_MARKER": marker}, Enabled: true, HostAccessApproved: true}}); err != nil {
		t.Fatal(err)
	}
	// Import deliberately strips grants; enable only this harmless explicit test
	// child through the same independent host-approval API used by the product.
	if err = app.Store.SetMCPEnabled(ctx, product.ID, "fixture", true, true); err != nil {
		t.Fatal(err)
	}
	if _, err = os.Stat(marker); !os.IsNotExist(err) {
		t.Fatal("saving approved recipe eagerly started MCP", err)
	}
	if _, err = app.Service.Submit(ctx, product.ID, "invoke the explicit local MCP fixture"); err != nil {
		t.Fatal(err)
	}
	awaitTerminal(t, app, product.ID)
	p := captured.Load()
	if p == nil {
		t.Fatal("actual recipe omitted MCP from its attempt resource registry")
	}
	if !p.CleanupComplete() {
		t.Fatal("SDK/attempt did not finish MCP physical cleanup")
	}
	if _, err = os.Stat(marker); err != nil {
		t.Fatal("MCP fixture never actually ran", err)
	}
	if calls.Load() != 2 || p.Stats().Records < 8 {
		t.Fatal("missing actual gateway/protocol facts", calls.Load(), p.Stats())
	}
	app.pendingMu.Lock()
	pending := len(app.pending)
	app.pendingMu.Unlock()
	if pending != 0 {
		t.Fatal("completed MCP attempt retained unfinished resource", pending)
	}
}
