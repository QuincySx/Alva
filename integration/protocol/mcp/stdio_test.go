// INPUT: testing, bounded local test-helper subprocess, MCP client and full-frame observer
// OUTPUT: Handshake, pagination, continuous logs, cancellation, observer failure and frame regressions
// POS: [integration / external protocol] Native MCP acceptance using only owned fixtures without external servers or credentials
package mcp

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/QuincySx/Alva/platform/system/process"
)

func TestMCPChild(t *testing.T) {
	mode := ""
	for _, arg := range os.Args {
		if strings.HasPrefix(arg, "--alva-mcp-child=") {
			mode = strings.TrimPrefix(arg, "--alva-mcp-child=")
		}
	}
	if mode == "" {
		return
	}
	scanner := bufio.NewScanner(os.Stdin)
	scanner.Buffer(make([]byte, 4096), FrameLimit+1)
	for scanner.Scan() {
		var request struct {
			ID     json.RawMessage            `json:"id"`
			Method string                     `json:"method"`
			Params map[string]json.RawMessage `json:"params"`
		}
		if json.Unmarshal(scanner.Bytes(), &request) != nil {
			os.Exit(2)
		}
		if request.Method == "notifications/initialized" {
			if mode == "partial-stderr" {
				fmt.Fprint(os.Stderr, strings.Repeat("partial stderr ", 10000))
			} else {
				fmt.Fprintln(os.Stderr, "idle stderr marker")
			}
			fmt.Fprintln(os.Stdout, `{"jsonrpc":"2.0","method":"notifications/message","params":{"text":"idle notice"}}`)
			continue
		}
		var result any
		switch request.Method {
		case "initialize":
			version := versions[0]
			if mode == "badversion" {
				version = "1900-01-01"
			}
			result = map[string]any{"protocolVersion": version, "capabilities": map[string]any{"tools": map[string]any{}}, "serverInfo": map[string]string{"name": "local-fixture", "version": "1"}}
		case "tools/list":
			if _, ok := request.Params["cursor"]; !ok {
				result = map[string]any{"tools": []any{map[string]any{"name": "echo", "description": "first tool", "inputSchema": map[string]string{"type": "object"}}}, "nextCursor": "page-2"}
			} else {
				result = map[string]any{"tools": []any{map[string]any{"name": "wait", "inputSchema": map[string]string{"type": "object"}}}}
			}
		case "tools/call":
			var name string
			json.Unmarshal(request.Params["name"], &name)
			switch name {
			case "wait":
				for {
					time.Sleep(time.Hour)
				}
			case "partial":
				fmt.Fprint(os.Stdout, `{"jsonrpc":"2.0","result":`)
				os.Exit(0)
			case "flood":
				for i := 0; i < 140; i++ {
					fmt.Fprintln(os.Stdout, `{"jsonrpc":"2.0","method":"notifications/progress"}`)
				}
				continue
			case "error":
				fmt.Fprintf(os.Stdout, "{\"jsonrpc\":\"2.0\",\"id\":%s,\"error\":{\"code\":-1,\"message\":\"remote private body\"}}\n", request.ID)
				continue
			default:
				result = map[string]any{"content": []map[string]string{{"type": "text", "text": "complete echoed result"}}, "isError": false}
			}
		default:
			result = map[string]any{}
		}
		encoded, _ := json.Marshal(map[string]any{"jsonrpc": "2.0", "id": request.ID, "result": result})
		fmt.Fprintln(os.Stdout, string(encoded))
	}
	os.Exit(0)
}

type journal struct {
	mu     sync.Mutex
	events []IOEvent
	fail   bool
}

func (j *journal) Record(ctx context.Context, event IOEvent) (string, error) {
	if err := ctx.Err(); err != nil {
		return "", err
	}
	j.mu.Lock()
	defer j.mu.Unlock()
	if j.fail {
		return "", errors.New("journal unavailable")
	}
	j.events = append(j.events, event)
	return fmt.Sprint(len(j.events)), nil
}
func (j *journal) contains(direction, text string) bool {
	j.mu.Lock()
	defer j.mu.Unlock()
	for _, event := range j.events {
		if event.Direction == direction && strings.Contains(string(event.Frame), text) {
			return true
		}
	}
	return false
}
func config(t *testing.T, mode string) Config {
	t.Helper()
	return Config{ID: "test-server", Process: process.Config{Command: os.Args[0], Args: []string{"-test.run=^TestMCPChild$", "--", "--alva-mcp-child=" + mode}, Cwd: t.TempDir()}, ConnectTimeout: 3 * time.Second, RequestTimeout: 3 * time.Second}
}
func closeClient(t *testing.T, c *Client) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err := c.Close(ctx); err != nil {
		t.Error(err)
	}
}
func TestHandshakePagedToolsAndContinuousJournal(t *testing.T) {
	j := &journal{}
	c, err := Open(context.Background(), config(t, "normal"), j)
	if err != nil {
		t.Fatal(err)
	}
	defer closeClient(t, c)
	tools, err := c.ListTools(context.Background())
	if err != nil || len(tools) != 2 {
		t.Fatalf("tools: %v %v", tools, err)
	}
	data, err := c.CallTool(context.Background(), "echo", json.RawMessage(`{"input":"selected"}`))
	if err != nil || !strings.Contains(string(data), "complete echoed result") {
		t.Fatalf("call: %s %v", data, err)
	}
	deadline := time.Now().Add(time.Second)
	for !j.contains("stderr", "idle stderr marker") && time.Now().Before(deadline) {
		time.Sleep(time.Millisecond)
	}
	if !j.contains("incoming", "idle notice") || !j.contains("stderr", "idle stderr marker") || !j.contains("outgoing_attempt", "tools/call") {
		t.Fatal("continuous idle I/O or full outgoing attempt missing")
	}
	if len(ToolAlias("server", "tool")) != 64 || ToolAlias("a:b", "c") == ToolAlias("a", "b:c") {
		t.Fatal("invalid MCP alias")
	}
}
func TestCancellationStopsOwnedConnectionWithoutReplay(t *testing.T) {
	c, err := Open(context.Background(), config(t, "normal"), nil)
	if err != nil {
		t.Fatal(err)
	}
	defer closeClient(t, c)
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Millisecond)
	defer cancel()
	_, err = c.CallTool(ctx, "wait", json.RawMessage(`{}`))
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("cancel: %v", err)
	}
	if c.Connected() {
		t.Fatal("cancelled transport still connected")
	}
	if _, err = c.CallTool(context.Background(), "echo", json.RawMessage(`{}`)); err == nil {
		t.Fatal("cancelled RPC replayed")
	}
}
func TestIncompletePrefixAndObserverFailureAreExplicit(t *testing.T) {
	j := &journal{}
	c, err := Open(context.Background(), config(t, "normal"), j)
	if err != nil {
		t.Fatal(err)
	}
	_, err = c.CallTool(context.Background(), "partial", json.RawMessage(`{}`))
	if err == nil {
		t.Fatal("partial response accepted")
	}
	closeClient(t, c)
	if !j.contains("incomplete_incoming", "result") {
		t.Fatal("incomplete prefix omitted from journal")
	}
	j = &journal{}
	c, err = Open(context.Background(), config(t, "normal"), j)
	if err != nil {
		t.Fatal(err)
	}
	j.mu.Lock()
	j.fail = true
	j.mu.Unlock()
	_, err = c.CallTool(context.Background(), "echo", json.RawMessage(`{}`))
	if err == nil || c.Connected() {
		t.Fatal("observer failure did not stop interaction")
	}
	cleanup, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	var observation *observerError
	if err = c.Close(cleanup); !errors.As(err, &observation) || !c.CleanupComplete() {
		t.Fatal("observer failure lost during actual cleanup", err)
	}
}

func TestClosePreservesLateStderrObservationFailureWithoutPrivateText(t *testing.T) {
	want := errors.New("private storage path /never/expose")
	ready := make(chan struct{})
	var once sync.Once
	observer := ObserverFunc(func(_ context.Context, event IOEvent) (string, error) {
		if event.Direction == "incoming" && strings.Contains(string(event.Frame), "idle notice") {
			once.Do(func() { close(ready) })
		}
		if event.Direction == "stderr" {
			return "", want
		}
		return "ref", nil
	})
	c, err := Open(context.Background(), config(t, "partial-stderr"), observer)
	if err != nil {
		t.Fatal(err)
	}
	select {
	case <-ready:
	case <-time.After(3 * time.Second):
		t.Fatal("fixture did not write pending stderr")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	err = c.Close(ctx)
	if !errors.Is(err, want) || !c.CleanupComplete() || strings.Contains(err.Error(), "/never/expose") {
		t.Fatal("late journal failure hidden, leaked or child unreaped", err)
	}
}
func TestProtocolFailuresDoNotExposeRemoteBody(t *testing.T) {
	if c, err := Open(context.Background(), config(t, "badversion"), nil); err == nil {
		closeClient(t, c)
		t.Fatal("bad negotiation accepted")
	}
	for _, name := range []string{"flood", "error"} {
		t.Run(name, func(t *testing.T) {
			c, err := Open(context.Background(), config(t, "normal"), nil)
			if err != nil {
				t.Fatal(err)
			}
			defer closeClient(t, c)
			_, err = c.CallTool(context.Background(), name, json.RawMessage(`{}`))
			if err == nil || strings.Contains(err.Error(), "remote private body") {
				t.Fatalf("protocol failure: %v", err)
			}
		})
	}
}
func TestFrameBudgetsAndPartialBytePreservation(t *testing.T) {
	for _, test := range []struct {
		data string
		ok   bool
	}{{"{}\n", true}, {"{}", false}, {strings.Repeat("x", FrameLimit+3) + "\n", false}} {
		data, err := readFrame(bufio.NewReader(bytes.NewBufferString(test.data)))
		if (err == nil) != test.ok {
			t.Fatalf("unexpected frame: %d %v", len(data), err)
		}
		if len(data) > FrameLimit {
			t.Fatal("unbounded frame")
		}
	}
}
