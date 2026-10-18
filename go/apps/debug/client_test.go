// INPUT: Actual diagnostic loopback service and official MCP client/server over owned newline stdio streams
// OUTPUT: Cold tool catalog, actual state/action/capture/control, read-only authority and cancellation acceptance
// POS: [L6 diagnostics] Real transport fixtures use only synthetic UI state and no accounts or global configuration.
package debug

import (
	"bytes"
	"context"
	"encoding/json"
	"image"
	"image/png"
	"net"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/modelcontextprotocol/go-sdk/mcp"
)

func TestActualUIDiagnosticMCPStateActionImageAndControl(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
	defer cancel()
	s := fixtureServer(t, true, false)
	var effects atomic.Int32
	var imageData bytes.Buffer
	if err := png.Encode(&imageData, image.NewRGBA(image.Rect(0, 0, 24, 12))); err != nil {
		t.Fatal(err)
	}
	v, _ := NewView(ctx, func(fn func()) { fn() }, func() Snapshot { return fixtureSnapshot(int(effects.Load())) }, func(string, json.RawMessage) error { effects.Add(1); return nil }, func() ([]byte, error) { return imageData.Bytes(), nil })
	unregister, err := s.Register("window", "Fixture", []Method{{Name: "increment", Effect: "ui"}}, v)
	if err != nil {
		t.Fatal(err)
	}
	defer unregister()
	address, err := s.Start("127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	client, err := NewClient(ClientConfig{Address: address, Token: s.config.Token, AllowControls: true})
	if err != nil {
		t.Fatal(err)
	}
	a, b := net.Pipe()
	defer a.Close()
	defer b.Close()
	done := make(chan error, 1)
	go func() { done <- client.Serve(ctx, a, a) }()
	connection, err := mcp.NewClient(&mcp.Implementation{Name: "fixture", Version: "1"}, nil).Connect(ctx, &mcp.IOTransport{Reader: b, Writer: b, MaxLineLength: 8 << 20}, nil)
	if err != nil {
		t.Fatal(err)
	}
	tools, err := connection.ListTools(ctx, nil)
	if err != nil || len(tools.Tools) != 4 || effects.Load() != 0 {
		t.Fatal("catalog executed UI work or is incomplete", err)
	}
	call := func(name string, args any) *mcp.CallToolResult {
		t.Helper()
		result, err := connection.CallTool(ctx, &mcp.CallToolParams{Name: name, Arguments: args})
		if err != nil || result.IsError {
			t.Fatal("actual diagnostic MCP failed", name, err, result)
		}
		return result
	}
	result := call("ui_inspect", map[string]any{"action": "state", "view_id": "window"})
	text, ok := result.Content[0].(*mcp.TextContent)
	if !ok || strings.Contains(text.Text, s.config.Token) {
		t.Fatal("invalid or private state result")
	}
	var snapshot Snapshot
	if err := json.Unmarshal([]byte(text.Text), &snapshot); err != nil {
		t.Fatal(err)
	}
	args := map[string]any{"operation_id": "mcp-once", "view_id": "window", "revision": snapshot.Revision, "method": "increment", "args": map[string]any{}}
	call("ui_action", args)
	call("ui_action", args)
	if effects.Load() != 1 {
		t.Fatal("official MCP retry replayed a UI effect")
	}
	result = call("ui_capture", map[string]any{"view_id": "window"})
	imageContent, ok := result.Content[0].(*mcp.ImageContent)
	if !ok || imageContent.MIMEType != "image/png" || !bytes.Equal(imageContent.Data, imageData.Bytes()) {
		t.Fatal("actual PNG did not survive official MCP image transport")
	}
	call("ui_diagnostic_control", map[string]any{"action": "level", "level": "debug"})
	call("ui_inspect", map[string]any{"action": "logs"})
	call("ui_diagnostic_control", map[string]any{"action": "shutdown"})
	if effects.Load() != 1 {
		t.Fatal("diagnostic shutdown executed another UI effect")
	}
	if err := connection.Close(); err != nil {
		t.Fatal(err)
	}
	select {
	case err := <-done:
		if err != nil {
			t.Fatal(err)
		}
	case <-ctx.Done():
		t.Fatal("diagnostic MCP did not join stdio")
	}
}

func TestDefaultMCPUIClientAdvertisesOnlyReads(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	s := fixtureServer(t, true, true)
	address, err := s.Start("127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	client, err := NewClient(ClientConfig{Address: address, Token: s.config.Token})
	if err != nil {
		t.Fatal(err)
	}
	a, b := net.Pipe()
	defer a.Close()
	defer b.Close()
	done := make(chan error, 1)
	go func() { done <- client.Serve(ctx, a, a) }()
	connection, err := mcp.NewClient(&mcp.Implementation{Name: "fixture", Version: "1"}, nil).Connect(ctx, &mcp.IOTransport{Reader: b, Writer: b}, nil)
	if err != nil {
		t.Fatal(err)
	}
	tools, err := connection.ListTools(ctx, nil)
	if err != nil || len(tools.Tools) != 2 {
		t.Fatal(tools, err)
	}
	for _, tool := range tools.Tools {
		if !tool.Annotations.ReadOnlyHint {
			t.Fatal("default MCP client exposes authority")
		}
	}
	if _, err := client.Action(ctx, []byte(`{}`)); err == nil {
		t.Fatal("server opt-in granted client controls implicitly")
	}
	connection.Close()
	select {
	case err := <-done:
		if err != nil {
			t.Fatal(err)
		}
	case <-ctx.Done():
		t.Fatal("MCP did not join")
	}
}
