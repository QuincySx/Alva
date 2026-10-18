// INPUT: Real demo Hub HTTP/native facts and official MCP clients over actual newline stdio connections
// OUTPUT: Read-only discovery/index/range authorization, no Agent replay and cancellation regressions
// POS: [L7 Hub product] Diagnostic acceptance uses the same real product authority as native/HTTP applications.
package devtools

import (
	"context"
	"encoding/json"
	"net"
	"net/http/httptest"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/apps/hub/server"
	"github.com/QuincySx/Alva/platform/system/loopback"
	"github.com/modelcontextprotocol/go-sdk/mcp"
)

func TestActualDevtoolsMCPReadsOwnedIndexesAndSelectedRecordsWithoutReplay(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	app, err := bootstrap.Open(ctx, bootstrap.Config{DataDirectory: filepath.Join(t.TempDir(), "private"), Demo: true})
	if err != nil {
		t.Fatal(err)
	}
	defer app.Close(context.Background())
	s, err := app.Service.Create(ctx, "inspect me", "")
	if err != nil {
		t.Fatal(err)
	}
	if err = app.Runtime.Run(ctx, s.ID, "actual demo facts"); err != nil {
		t.Fatal(err)
	}
	other, err := app.Service.Create(ctx, "foreign owner", "")
	if err != nil {
		t.Fatal(err)
	}
	detail, err := app.Store.Detail(ctx, s.ID)
	if err != nil {
		t.Fatal(err)
	}
	branch, err := product.ActiveBranch(detail)
	if err != nil {
		t.Fatal(err)
	}
	log, err := app.Repository.OpenSession(ctx, s.ID, branch.NativeSessionID)
	if err != nil {
		t.Fatal(err)
	}
	before, err := log.Records(ctx, 0)
	if err != nil {
		t.Fatal(err)
	}
	token, err := loopback.NewToken()
	if err != nil {
		t.Fatal(err)
	}
	httpServer := httptest.NewUnstartedServer(nil)
	handler, err := server.New(app.Service, server.Config{Authority: httpServer.Listener.Addr().String(), Token: token})
	if err != nil {
		t.Fatal(err)
	}
	httpServer.Config.Handler = handler
	httpServer.Start()
	defer httpServer.Close()
	defer handler.Close()
	diagnostic, err := New(Config{Address: httpServer.URL, Token: token})
	if err != nil {
		t.Fatal(err)
	}
	a, b := net.Pipe()
	defer a.Close()
	defer b.Close()
	done := make(chan error, 1)
	go func() { done <- diagnostic.Serve(ctx, a, a) }()
	client := mcp.NewClient(&mcp.Implementation{Name: "fixture", Version: "1"}, nil)
	session, err := client.Connect(ctx, &mcp.IOTransport{Reader: b, Writer: b, MaxLineLength: 1 << 20}, nil)
	if err != nil {
		t.Fatal(err)
	}
	tools, err := session.ListTools(ctx, nil)
	if err != nil || len(tools.Tools) != 1 || tools.Tools[0].Name != "hub_inspect" || !tools.Tools[0].Annotations.ReadOnlyHint {
		t.Fatal("read-only MCP catalog invalid", err)
	}
	call := func(args map[string]any, wantError bool) string {
		t.Helper()
		result, err := session.CallTool(ctx, &mcp.CallToolParams{Name: "hub_inspect", Arguments: args})
		if err != nil {
			t.Fatal(err)
		}
		if result.IsError != wantError || len(result.Content) != 1 {
			t.Fatal("unexpected tool result", result.IsError, wantError)
		}
		text, ok := result.Content[0].(*mcp.TextContent)
		if !ok {
			t.Fatal("unexpected nontext result")
		}
		if strings.Contains(text.Text, token) {
			t.Fatal("diagnostic result leaked bearer")
		}
		return text.Text
	}
	if result := call(map[string]any{"action": "sessions"}, false); !strings.Contains(result, "inspect me") {
		t.Fatal("actual sessions absent")
	}
	call(map[string]any{"action": "history", "session_id": s.ID}, false)
	index := call(map[string]any{"action": "inspector", "session_id": s.ID, "branch_id": branch.ID}, false)
	var page product.InspectorPage
	if json.Unmarshal([]byte(index), &page) != nil || len(page.Entries) == 0 {
		t.Fatal("actual typed index absent")
	}
	uri := page.Entries[0].Value.URI
	call(map[string]any{"action": "record", "session_id": s.ID, "branch_id": branch.ID, "uri": uri, "offset": 0}, false)
	call(map[string]any{"action": "record", "session_id": other.ID, "branch_id": branch.ID, "uri": uri}, true)
	call(map[string]any{"action": "stop", "session_id": s.ID}, true)
	call(map[string]any{"action": "sessions", "native_id": branch.NativeSessionID}, true)
	after, err := log.Records(ctx, 0)
	if err != nil || len(after) != len(before) || app.Runtime.HasActive() {
		t.Fatal("diagnostic observation replayed or changed facts", err)
	}
	if err = session.Close(); err != nil {
		t.Fatal(err)
	}
	select {
	case err = <-done:
		if err != nil {
			t.Fatal(err)
		}
	case <-ctx.Done():
		t.Fatal("MCP server did not join stdio/handlers")
	}
}
func TestDevtoolsRejectsNoncanonicalOrCredentialBearingAddresses(t *testing.T) {
	token, _ := loopback.NewToken()
	for _, address := range []string{"http://localhost:1234", "http://0.0.0.0:1234", "https://127.0.0.1:1234", "http://127.0.0.1:1234/", "http://127.0.0.1:1234?token=x", "http://user@127.0.0.1:1234"} {
		if _, err := New(Config{Address: address, Token: token}); err == nil {
			t.Fatal("unsafe diagnostic address accepted")
		}
	}
}
