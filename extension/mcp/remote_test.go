// INPUT: A loopback Streamable HTTP fixture, real session journal and approving guard
// OUTPUT: Managed MCP regressions for remote servers: lazy start, redacted headers and no process requirement
// POS: [L4 extension] Remote MCP runs through the same approved gateway and journal as local processes
package managedmcp

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"sync/atomic"
	"testing"
	"time"

	protocol "github.com/QuincySx/Alva/integration/protocol/mcp"
	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

func TestRemoteMCPRunsLazilyThroughTheApprovedGateway(t *testing.T) {
	const secret = "remote-header-credential-value"
	var requests atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		requests.Add(1)
		if r.Header.Get("X-Api-Key") != secret {
			http.Error(w, "missing header", http.StatusUnauthorized)
			return
		}
		var message struct {
			ID     json.RawMessage `json:"id"`
			Method string          `json:"method"`
		}
		if json.NewDecoder(r.Body).Decode(&message) != nil {
			http.Error(w, "bad", http.StatusBadRequest)
			return
		}
		if message.Method == "notifications/initialized" || r.Method == http.MethodDelete {
			w.WriteHeader(http.StatusAccepted)
			return
		}
		result := map[string]any{"tools": []any{map[string]any{"name": "remote_echo", "description": "echo " + secret, "inputSchema": map[string]string{"type": "object"}}}}
		if message.Method == "initialize" {
			result = map[string]any{"protocolVersion": "2025-06-18", "capabilities": map[string]any{"tools": map[string]any{}}, "serverInfo": map[string]string{"name": "remote", "version": "1"}}
		}
		w.Header().Set("Content-Type", "application/json")
		json.NewEncoder(w).Encode(map[string]any{"jsonrpc": "2.0", "id": message.ID, "result": result})
	}))
	defer server.Close()
	ctx := context.Background()
	repo, err := session.Open(ctx, filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	log, err := repo.Create(ctx, "product", "native", "")
	if err != nil {
		t.Fatal(err)
	}
	guard, err := security.NewGuard(t.TempDir(), nil, nil, security.NewPermissionManager(security.Ask, security.ApprovalFunc(allow)))
	if err != nil {
		t.Fatal(err)
	}
	p, err := New(ctx, Config{Repository: repo, Owner: "product", NativeID: "native", Guard: guard, Servers: []protocol.Config{{ID: "remote", HTTP: &protocol.HTTPConfig{URL: server.URL + "/mcp", Headers: map[string]string{"X-Api-Key": secret}}, ConnectTimeout: 3 * time.Second, RequestTimeout: 3 * time.Second}}})
	if err != nil {
		t.Fatal(err)
	}
	defer p.Close(ctx)
	state, err := abi.NewAgentState(fixtureModel{}, log, nil)
	if err != nil {
		t.Fatal(err)
	}
	runtime := abi.RuntimeContext{RunID: "run", State: state}
	if err = p.OnRunStart(ctx, runtime); err != nil {
		t.Fatal(err)
	}
	if requests.Load() != 0 {
		t.Fatal("run start contacted the remote server")
	}
	if _, err = invoke(ctx, p, `{"action":"search_tools","server_id":"remote"}`); err != nil {
		t.Fatal(err)
	}
	if err = p.OnRunEnd(ctx, runtime, nil); err != nil {
		t.Fatal(err)
	}
	data, _ := json.Marshal(p.journal.records)
	if requests.Load() == 0 || bytes.Contains(data, []byte(secret)) || !bytes.Contains(data, []byte("remote_echo")) {
		t.Fatal("remote discovery missing or header secret journaled", string(data))
	}
}

func TestRemoteMCPRejectsAProcessOnTheSameEntry(t *testing.T) {
	ctx := context.Background()
	repo, err := session.Open(ctx, filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	if _, err = repo.Create(ctx, "product", "native", ""); err != nil {
		t.Fatal(err)
	}
	guard, _ := security.NewGuard(t.TempDir(), nil, nil, security.NewPermissionManager(security.Ask, security.ApprovalFunc(allow)))
	config := protocol.Config{ID: "mixed", HTTP: &protocol.HTTPConfig{URL: "https://mcp.example.test/mcp"}}
	config.Process.Command = "/bin/echo"
	if _, err = New(ctx, Config{Repository: repo, Owner: "product", NativeID: "native", Guard: guard, Servers: []protocol.Config{config}}); err == nil {
		t.Fatal("entry with both a process and an endpoint was accepted")
	}
}
