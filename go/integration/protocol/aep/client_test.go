// INPUT: same-test-binary owned AEP helper and explicit capability/observer fixtures
// OUTPUT: Bidirectional callback, identity denial, full journal, cancellation and observer regressions
// POS: [integration / external protocol] Actual stdio Plugin protocol acceptance without external runtimes or user credentials
package aep

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/QuincySx/Alva/go/platform/system/process"
)

func TestAEPHelper(t *testing.T) {
	mode := ""
	for _, arg := range os.Args {
		if strings.HasPrefix(arg, "--aep-helper=") {
			mode = strings.TrimPrefix(arg, "--aep-helper=")
		}
	}
	if mode == "" {
		return
	}
	reader := bufio.NewScanner(os.Stdin)
	reader.Buffer(make([]byte, 4096), FrameLimit+1)
	write := func(value any) { data, _ := json.Marshal(value); fmt.Println(string(data)) }
	for reader.Scan() {
		var request struct {
			ID, Method string
			Params     json.RawMessage
		}
		if json.Unmarshal(reader.Bytes(), &request) != nil {
			os.Exit(2)
		}
		switch request.Method {
		case "shutdown":
			if mode == "shutdown-hang" {
				continue
			}
			write(map[string]any{"jsonrpc": "2.0", "id": request.ID, "result": map[string]any{}})
			if mode == "shutdown-linger" {
				for {
					time.Sleep(time.Second)
				}
			}
			fmt.Fprintln(os.Stderr, "graceful shutdown final diagnostic")
			os.Exit(0)
		case "initialize":
			name := "fixture"
			capabilities := []string{"host:log"}
			if mode == "identity" {
				name = "foreign"
			}
			if mode == "expanded" {
				capabilities = append(capabilities, "host:memory.write")
			}
			if mode == "reduced" {
				capabilities = []string{}
			}
			write(map[string]any{"jsonrpc": "2.0", "id": request.ID, "result": Handshake{ProtocolVersion: Version, Plugin: PluginInfo{Name: name, Version: "1"}, Tools: []Tool{{Name: "fixture_tool", Description: "untrusted", InputSchema: json.RawMessage(`{"type":"object"}`)}}, Capabilities: capabilities}})
		case "initialized":
			if mode == "partial-stderr" {
				fmt.Fprint(os.Stderr, strings.Repeat("partial stderr ", 10000))
				write(map[string]any{"jsonrpc": "2.0", "method": "host/log", "params": map[string]string{"message": "stderr pending ready"}})
			} else {
				fmt.Fprint(os.Stderr, "idle stderr evidence\n")
			}
		case "tools/call":
			if mode == "hang" {
				continue
			}
			callbackMethod := "host/log"
			if mode == "denied" {
				callbackMethod = "host/memory.write"
			}
			write(map[string]any{"jsonrpc": "2.0", "id": "p-1", "method": callbackMethod, "params": map[string]string{"message": "callback"}})
			if !reader.Scan() {
				os.Exit(3)
			}
			var result map[string]json.RawMessage
			_ = json.Unmarshal(reader.Bytes(), &result)
			if mode == "denied" {
				if len(result["error"]) == 0 {
					os.Exit(4)
				}
			} else if string(result["result"]) != `{"logged":true}` {
				os.Exit(5)
			}
			write(map[string]any{"jsonrpc": "2.0", "id": request.ID, "result": map[string]any{"text": strings.Repeat("日志", 10000) + "完整尾部", "callback_denied": mode == "denied"}})
		case "extension/before_tool_call":
			write(map[string]any{"jsonrpc": "2.0", "id": request.ID, "result": Action{Action: "block", Reason: "fixture veto"}})
		default:
			if request.ID != "" {
				write(map[string]any{"jsonrpc": "2.0", "id": request.ID, "result": map[string]bool{"ok": true}})
			}
		}
	}
	os.Exit(0)
}
func fixtureConfig(t *testing.T, mode string) Config {
	return Config{Name: "fixture", PluginVersion: "1", Process: process.Config{Command: os.Args[0], Args: []string{"-test.run=^TestAEPHelper$", "--", "--aep-helper=" + mode}, Cwd: t.TempDir()}, RequestedCapabilities: []string{"host:log"}, HostCapabilities: HostCapabilities{HostAPI: []string{"log"}, Events: []string{"extension/before_tool_call"}}, RequestTimeout: time.Second}
}

func TestGracefulShutdownDrainsFinalOutputAndReallyReaps(t *testing.T) {
	for _, mode := range []string{"normal", "shutdown-hang", "shutdown-linger", "shutdown-observer"} {
		t.Run(mode, func(t *testing.T) {
			var mu sync.Mutex
			shutdowns, final := 0, false
			observation := errors.New("fixture final observer failure")
			observer := ObserverFunc(func(_ context.Context, event IOEvent) error {
				mu.Lock()
				defer mu.Unlock()
				if event.Direction == "outgoing_attempt" && strings.Contains(string(event.Frame), `"method":"shutdown"`) {
					shutdowns++
				}
				if event.Direction == "stderr" && strings.Contains(string(event.Frame), "graceful shutdown final diagnostic") {
					final = true
					if mode == "shutdown-observer" {
						return observation
					}
				}
				return nil
			})
			client, err := Open(context.Background(), fixtureConfig(t, mode), HandlerFunc(func(context.Context, string, json.RawMessage) (json.RawMessage, error) {
				return json.RawMessage(`null`), nil
			}), observer)
			if err != nil {
				t.Fatal(err)
			}
			ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
			defer cancel()
			err = client.Shutdown(ctx)
			if mode == "normal" && err != nil {
				t.Fatal("healthy peer failed graceful cleanup", err)
			}
			if (mode == "shutdown-hang" || mode == "shutdown-linger") && !errors.Is(err, context.DeadlineExceeded) {
				t.Fatal("hung peer claimed graceful success", err)
			}
			if mode == "shutdown-observer" && !errors.Is(err, observation) {
				t.Fatal("final observer failure lost", err)
			}
			if !client.CleanupComplete() {
				t.Fatal("shutdown returned before actual cleanup")
			}
			if _, err = client.Call(ctx, "tools/list", nil); err == nil {
				t.Fatal("shutdown reopened request admission")
			}
			mu.Lock()
			defer mu.Unlock()
			if shutdowns != 1 || (mode == "normal" || mode == "shutdown-observer") && !final {
				t.Fatal("shutdown exchange/final stderr missing", shutdowns, final)
			}
		})
	}
}
func TestBidirectionalPluginCallAndCompleteJournal(t *testing.T) {
	var mu sync.Mutex
	var frames []IOEvent
	callbacks := 0
	observer := ObserverFunc(func(_ context.Context, event IOEvent) error {
		mu.Lock()
		defer mu.Unlock()
		frames = append(frames, event)
		return nil
	})
	client, err := Open(context.Background(), fixtureConfig(t, "normal"), HandlerFunc(func(_ context.Context, method string, params json.RawMessage) (json.RawMessage, error) {
		callbacks++
		if method != "host/log" || !strings.Contains(string(params), "callback") {
			t.Error("callback routing changed")
		}
		return json.RawMessage(`{"logged":true}`), nil
	}), observer)
	if err != nil {
		t.Fatal(err)
	}
	data, err := client.Call(context.Background(), "tools/call", map[string]any{"name": "fixture_tool", "arguments": map[string]any{}})
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(data), "完整尾部") || callbacks != 1 {
		t.Fatal("callback or complete UTF-8 result missing")
	}
	handshake := client.Handshake()
	handshake.Tools[0].InputSchema[0] = 'x'
	if !json.Valid(client.Handshake().Tools[0].InputSchema) {
		t.Fatal("handshake snapshot shares bytes")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err = client.Close(ctx); err != nil {
		t.Fatal(err)
	}
	mu.Lock()
	defer mu.Unlock()
	var incoming, outgoing, stderr bool
	for _, frame := range frames {
		incoming = incoming || frame.Direction == "incoming" && strings.Contains(string(frame.Frame), "完整尾部")
		outgoing = outgoing || frame.Direction == "outgoing_attempt" && strings.Contains(string(frame.Frame), "logged")
		stderr = stderr || frame.Direction == "stderr"
	}
	if !incoming || !outgoing || !stderr {
		t.Fatalf("journal omitted traffic: in=%v out=%v stderr=%v", incoming, outgoing, stderr)
	}
}
func TestUndeclaredCallbackNeverInvokesHost(t *testing.T) {
	client, err := Open(context.Background(), fixtureConfig(t, "denied"), HandlerFunc(func(context.Context, string, json.RawMessage) (json.RawMessage, error) {
		t.Error("undeclared callback invoked host")
		return nil, nil
	}), nil)
	if err != nil {
		t.Fatal(err)
	}
	data, err := client.Call(context.Background(), "tools/call", map[string]any{})
	if err != nil || !strings.Contains(string(data), `"callback_denied":true`) {
		t.Fatalf("denied callback not answered: %s %v", data, err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err = client.Close(ctx); err != nil {
		t.Fatal(err)
	}
}
func TestHandshakeCannotExpandIdentityOrCapabilities(t *testing.T) {
	for _, mode := range []string{"identity", "expanded", "reduced"} {
		t.Run(mode, func(t *testing.T) {
			client, err := Open(context.Background(), fixtureConfig(t, mode), HandlerFunc(func(context.Context, string, json.RawMessage) (json.RawMessage, error) {
				return json.RawMessage(`null`), nil
			}), nil)
			if err == nil {
				client.Close(context.Background())
				t.Fatal("untrusted handshake expanded authority")
			}
		})
	}
}
func TestCancellationStopsConnectionWithoutReplay(t *testing.T) {
	client, err := Open(context.Background(), fixtureConfig(t, "hang"), HandlerFunc(func(context.Context, string, json.RawMessage) (json.RawMessage, error) {
		return json.RawMessage(`null`), nil
	}), nil)
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Millisecond)
	defer cancel()
	if _, err = client.Call(ctx, "tools/call", nil); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("wrong cancel: %v", err)
	}
	if _, err = client.Call(context.Background(), "tools/call", nil); err == nil {
		t.Fatal("cancelled connection replayed")
	}
	cleanup, stop := context.WithTimeout(context.Background(), 3*time.Second)
	defer stop()
	if err = client.Close(cleanup); err != nil {
		t.Fatal(err)
	}
}
func TestObserverFailureStopsHandshake(t *testing.T) {
	client, err := Open(context.Background(), fixtureConfig(t, "normal"), HandlerFunc(func(context.Context, string, json.RawMessage) (json.RawMessage, error) {
		return json.RawMessage(`null`), nil
	}), ObserverFunc(func(context.Context, IOEvent) error { return errors.New("private store failure") }))
	if err == nil {
		client.Close(context.Background())
		t.Fatal("observer failure ignored")
	}
	if strings.Contains(err.Error(), "private store failure") {
		t.Fatal("observer internals escaped")
	}
}

func TestClosePreservesLateStderrObservationFailure(t *testing.T) {
	want := errors.New("private storage path /never/expose")
	ready := make(chan struct{})
	var once sync.Once
	client, err := Open(context.Background(), fixtureConfig(t, "partial-stderr"), HandlerFunc(func(context.Context, string, json.RawMessage) (json.RawMessage, error) {
		return json.RawMessage(`{}`), nil
	}), ObserverFunc(func(_ context.Context, event IOEvent) error {
		if event.Direction == "incoming" && strings.Contains(string(event.Frame), "stderr pending ready") {
			once.Do(func() { close(ready) })
		}
		if event.Direction == "stderr" {
			return want
		}
		return nil
	}))
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
	err = client.Close(ctx)
	if !errors.Is(err, want) || !client.CleanupComplete() || strings.Contains(err.Error(), "/never/expose") {
		t.Fatal("late observer cause hidden/leaked or child unreaped", err)
	}
}
