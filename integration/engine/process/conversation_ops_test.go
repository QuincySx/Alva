// INPUT: Project-owned helper processes speaking Codex app-server JSON-RPC and Pi RPC over stdio
// OUTPUT: Interjection (exact steer shapes, receipts only on delivery evidence, FIFO, no running execution) and Codex fork (exact list/fork shapes, new identity, untouched source) regressions
// POS: [integration / native external-engine adapter] Deterministic protocol tests; no installed CLI, account or model is started
package process

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/platform/storage/bindings"
)

// sameJSON compares a received frame field with the exact expected JSON.
func sameJSON(raw []byte, want string) bool {
	var got, expected any
	return json.Unmarshal(raw, &got) == nil && json.Unmarshal([]byte(want), &expected) == nil && reflect.DeepEqual(got, expected)
}

func TestCodexConversationOpsFixtureProcess(t *testing.T) {
	mode := os.Getenv("ALVA_CODEX_OPS_FIXTURE")
	if mode == "" {
		return
	}
	if !strings.HasSuffix(strings.Join(os.Args, " "), "app-server --listen stdio://") {
		os.Exit(90)
	}
	cwd, _ := os.Getwd()
	thread := "ops-thread"
	write := func(value any) { data, _ := json.Marshal(value); fmt.Println(string(data)) }
	notify := func(method string, params map[string]any) {
		params["threadId"], params["turnId"] = thread, "ops-turn"
		write(map[string]any{"method": method, "params": params})
	}
	item := func(method string, data map[string]any) { notify(method, map[string]any{"item": data}) }
	userMessage := func(id string, clientID any, text string) {
		data := map[string]any{"type": "userMessage", "id": id, "clientId": clientID, "content": []any{map[string]any{"type": "text", "text": text, "text_elements": []any{}}}}
		item("item/started", data)
		item("item/completed", data)
	}
	finish := func() {
		item("item/started", map[string]any{"type": "agentMessage", "id": "answer"})
		item("item/completed", map[string]any{"type": "agentMessage", "id": "answer", "text": "ops answer"})
		notify("turn/completed", map[string]any{"turn": map[string]any{"id": "ops-turn", "status": "completed"}})
	}
	turn := func(id string) map[string]any {
		return map[string]any{"id": id, "rootTurnId": nil, "items": []any{}, "itemsView": "notLoaded", "status": "completed", "error": nil}
	}
	lists, steers := 0, 0
	scan := bufio.NewScanner(os.Stdin)
	scan.Buffer(make([]byte, 4096), 128<<10)
	for scan.Scan() {
		var frame codexFrame
		if json.Unmarshal(scan.Bytes(), &frame) != nil {
			os.Exit(91)
		}
		reply := func(value any) { write(map[string]any{"id": frame.ID, "result": value}) }
		switch frame.Method {
		case "initialize":
			reply(map[string]string{"userAgent": "ops-fixture"})
		case "initialized":
		case "thread/start":
			// A fork never starts an unrelated thread.
			if strings.HasPrefix(mode, "fork") {
				os.Exit(120)
			}
			reply(map[string]any{"thread": map[string]any{"id": thread, "cwd": cwd}})
		case "thread/turns/list":
			// The source is only read: newest first, no item bodies.
			lists++
			base := `"threadId":"source-thread","sortDirection":"desc","itemsView":"notLoaded"`
			switch {
			case mode == "fork-1" && lists == 1 && sameJSON(frame.Params, `{`+base+`,"limit":2}`):
				reply(map[string]any{"data": []any{turn("turn-3"), turn("turn-2")}, "nextCursor": "cursor-older", "backwardsCursor": "cursor-newer"})
			case mode == "fork-all" && lists == 1 && sameJSON(frame.Params, `{`+base+`,"limit":4}`):
				reply(map[string]any{"data": []any{turn("turn-3"), turn("turn-2")}, "nextCursor": "cursor-older", "backwardsCursor": "cursor-newer"})
			case mode == "fork-all" && lists == 2 && sameJSON(frame.Params, `{`+base+`,"limit":2,"cursor":"cursor-older"}`):
				reply(map[string]any{"data": []any{turn("turn-1")}, "nextCursor": nil, "backwardsCursor": "cursor-newer"})
			case mode == "fork-short" && lists == 1 && sameJSON(frame.Params, `{`+base+`,"limit":6}`):
				reply(map[string]any{"data": []any{turn("turn-2"), turn("turn-1")}, "nextCursor": nil, "backwardsCursor": "cursor-newer"})
			default:
				os.Exit(121)
			}
		case "thread/fork":
			want := map[string]string{
				"fork-1":    `{"threadId":"source-thread","lastTurnId":"turn-2","cwd":%q,"sandbox":"read-only","approvalPolicy":"on-request","excludeTurns":true}`,
				"fork-all":  `{"threadId":"source-thread","beforeTurnId":"turn-1","cwd":%q,"sandbox":"read-only","approvalPolicy":"on-request","excludeTurns":true}`,
				"fork-keep": `{"threadId":"source-thread","cwd":%q,"sandbox":"read-only","approvalPolicy":"on-request","excludeTurns":true}`,
			}[mode]
			if want == "" || !sameJSON(frame.Params, fmt.Sprintf(want, cwd)) {
				os.Exit(122)
			}
			// The fork is a new thread; nothing here touches source-thread.
			thread = "forked-thread"
			write(map[string]any{"method": "thread/started", "params": map[string]any{"thread": map[string]any{"id": thread}}})
			reply(map[string]any{"thread": map[string]any{"id": thread, "cwd": cwd}, "model": "fixture", "modelProvider": "fixture", "cwd": cwd})
		case "turn/start":
			var params struct {
				ThreadID string `json:"threadId"`
			}
			if json.Unmarshal(frame.Params, &params) != nil || params.ThreadID != thread || params.ThreadID == "source-thread" {
				os.Exit(123)
			}
			reply(map[string]any{"turn": map[string]string{"id": "ops-turn", "status": "inProgress"}})
			// The prompt's own userMessage has no client id and is no receipt.
			userMessage("prompt-item", nil, "ops prompt")
			if mode == "steer" || mode == "undelivered" {
				item("item/started", map[string]any{"type": "commandExecution", "id": "cmd", "command": "ls", "cwd": cwd, "status": "inProgress"})
				continue
			}
			finish()
		case "turn/steer":
			steers++
			if mode != "steer" && mode != "undelivered" || steers != 1 || !sameJSON(frame.Params, `{"threadId":"ops-thread","expectedTurnId":"ops-turn","clientUserMessageId":"steer-1","input":[{"type":"text","text":"also check README","text_elements":[]}]}`) {
				os.Exit(124)
			}
			reply(map[string]string{"turnId": "ops-turn"})
			// Steering never interrupts the running tool; it ends first.
			item("item/completed", map[string]any{"type": "commandExecution", "id": "cmd", "status": "completed", "exitCode": 0, "aggregatedOutput": "README.md"})
			if mode == "steer" {
				userMessage("foreign-item", "other-client", "not this run's interjection")
				userMessage("steer-item", "steer-1", "also check README")
			}
			finish()
		default:
			os.Exit(125)
		}
	}
	os.Exit(0)
}

func codexOpsFixture(t *testing.T, mode string) (*Adapter, string) {
	t.Helper()
	binary, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	dir := filepath.Join(t.TempDir(), "private")
	store, err := bindings.Open(t.Context(), dir)
	if err != nil {
		t.Fatal(err)
	}
	a, err := New(Config{Bindings: bindingPorts{store}, Vendor: "codex", Transport: "app-server", Command: binary, PrefixArgs: []string{"-test.run=^TestCodexConversationOpsFixtureProcess$", "--"}, StateDirectory: dir, ApprovedHostAccess: true, Timeout: 5 * time.Second, Env: map[string]string{"ALVA_CODEX_OPS_FIXTURE": mode}})
	if err != nil {
		store.Close()
		t.Fatal(err)
	}
	a.nativeEnvironment = func() []string { return []string{"HOME=/fixture/native-home", "PATH=/fixture/native-bin"} }
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		if err := a.Close(ctx); err != nil {
			t.Error(err)
		}
	})
	return a, dir
}

func eventIndex(events []engine.Event, kind engine.Kind, id string) int {
	for i, event := range events {
		if event.Kind == kind && event.ID == id {
			return i
		}
	}
	return -1
}

func TestCodexInterjectionSteersAndReportsOnlyDeliveredMessages(t *testing.T) {
	input := engine.Interjection{ID: "steer-1", Text: "also check README"}
	for _, mode := range []string{"steer", "undelivered"} {
		t.Run(mode, func(t *testing.T) {
			a, _ := codexOpsFixture(t, mode)
			if capabilities := a.Capabilities(); !capabilities.Interjection || !capabilities.Fork {
				t.Fatal("app-server must declare its tested interjection and fork", capabilities)
			}
			if err := a.Interject(t.Context(), "ops-thread", input); !errors.Is(err, engine.ErrSessionNotFound) {
				t.Fatal("interjection without a running execution was accepted", err)
			}
			var events []engine.Event
			var contract engine.StreamContract
			err := a.Execute(t.Context(), engine.Request{OwnerID: "owner", Workspace: t.TempDir(), Prompt: "ops prompt"}, func(event engine.Event) error {
				if err := contract.Accept(event); err != nil {
					return err
				}
				events = append(events, event)
				if event.Kind == engine.ToolStart && event.ID == "cmd" {
					if err := a.Interject(t.Context(), event.SessionID, input); err != nil {
						return fmt.Errorf("steer was not accepted: %w", err)
					}
					if err := a.Interject(t.Context(), event.SessionID, input); err == nil {
						return errors.New("a repeated interjection identity was accepted")
					}
				}
				return nil
			})
			if err != nil || len(events) == 0 || events[len(events)-1].Kind != engine.Completed || events[len(events)-1].Outcome != engine.Succeeded {
				t.Fatal("steered run did not complete", err, events)
			}
			delivered := 0
			for _, event := range events {
				if event.Kind == engine.InputDelivered {
					delivered++
					if event.ID != input.ID || event.Text != input.Text || event.SessionID != "ops-thread" {
						t.Fatal("receipt lost the interjection identity, text or native session", event)
					}
				}
			}
			if mode == "undelivered" {
				if delivered != 0 {
					t.Fatal("a steer that never entered the turn was reported delivered", events)
				}
			} else {
				at := eventIndex(events, engine.InputDelivered, input.ID)
				if delivered != 1 || at < eventIndex(events, engine.ToolEnd, "cmd") || at > eventIndex(events, engine.Message, "answer") {
					t.Fatal("receipt missing, repeated or outside its delivery position", delivered, events)
				}
			}
			if err := a.Interject(t.Context(), "ops-thread", engine.Interjection{ID: "late", Text: "too late"}); !errors.Is(err, engine.ErrSessionNotFound) {
				t.Fatal("interjection after the execution ended was accepted", err)
			}
		})
	}
}

func TestCodexForkCopiesBoundSourceIntoNewThread(t *testing.T) {
	for _, tc := range []struct {
		mode string
		drop int
	}{{"fork-1", 1}, {"fork-all", 3}, {"fork-keep", 0}} {
		t.Run(tc.mode, func(t *testing.T) {
			a, _ := codexOpsFixture(t, tc.mode)
			workspace, err := filepath.EvalSymlinks(t.TempDir())
			if err != nil {
				t.Fatal(err)
			}
			source := storage.SessionBinding{ID: "source-thread", Owner: "owner", Workspace: workspace}
			if err := a.config.Bindings.PutBinding(t.Context(), source); err != nil {
				t.Fatal(err)
			}
			var events []engine.Event
			var contract engine.StreamContract
			err = a.Execute(t.Context(), engine.Request{OwnerID: "owner", Workspace: workspace, Prompt: "ops prompt", Fork: &engine.ForkPoint{Session: "source-thread", BeforeSequence: 41, DropTurns: tc.drop}}, func(event engine.Event) error {
				events = append(events, event)
				return contract.Accept(event)
			})
			if err != nil || len(events) == 0 || events[len(events)-1].Outcome != engine.Succeeded {
				t.Fatal("forked run did not complete", err, events)
			}
			if events[0].Kind != engine.SessionStarted || events[0].SessionID != "forked-thread" {
				t.Fatal("fork did not report the new native identity first", events[0])
			}
			for _, event := range events {
				if event.SessionID != "" && event.SessionID != "forked-thread" {
					t.Fatal("forked run leaked the source identity", event)
				}
			}
			forked, err := a.config.Bindings.ReadBinding(t.Context(), "forked-thread")
			if err != nil || forked.Owner != "owner" || forked.Workspace != workspace {
				t.Fatal("new thread was not bound to the product owner and cwd", forked, err)
			}
			if kept, err := a.config.Bindings.ReadBinding(t.Context(), "source-thread"); err != nil || kept != source {
				t.Fatal("source binding changed", kept, err)
			}
		})
	}
}

func TestCodexForkRefusesWhatItCannotCut(t *testing.T) {
	a, dir := codexOpsFixture(t, "fork-short")
	workspace, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	request := engine.Request{OwnerID: "owner", Workspace: workspace, Prompt: "ops prompt", Fork: &engine.ForkPoint{Session: "source-thread", DropTurns: 5}}
	called := false
	// An unbound source and a fork that also resumes have no side effects.
	if err := a.Execute(t.Context(), request, func(engine.Event) error { called = true; return nil }); !errors.Is(err, engine.ErrSessionNotFound) || called {
		t.Fatal("fork of an unbound source was not refused", err, called)
	}
	both := request
	both.ResumeSession = "source-thread"
	if err := a.Execute(t.Context(), both, func(engine.Event) error { called = true; return nil }); err == nil || called {
		t.Fatal("fork together with resume was not refused", err, called)
	}
	if _, err := os.Stat(filepath.Join(dir, "journals")); !os.IsNotExist(err) {
		t.Fatal("refused fork created execution state", err)
	}
	if err := a.config.Bindings.PutBinding(t.Context(), storage.SessionBinding{ID: "source-thread", Owner: "owner", Workspace: workspace}); err != nil {
		t.Fatal(err)
	}
	// Dropping more turns than the source has fails before any thread/fork.
	var events []engine.Event
	if err := a.Execute(t.Context(), request, func(event engine.Event) error { events = append(events, event); return nil }); err != nil {
		t.Fatal(err)
	}
	if len(events) != 2 || events[0].Kind != engine.Error || events[1].Kind != engine.Completed || events[1].Outcome != engine.Failed {
		t.Fatal("short source did not fail without a new session", events)
	}
}

func TestExecTransportsRefuseInterjectionAndFork(t *testing.T) {
	a, _ := fixtureAdapter(t, "codex", "ok")
	if capabilities := a.Capabilities(); capabilities.Interjection || capabilities.Fork {
		t.Fatal("exec transport declared conversation operations", capabilities)
	}
	if err := a.Interject(t.Context(), "native", engine.Interjection{ID: "steer-1", Text: "hello"}); !errors.Is(err, engine.ErrUnsupported) {
		t.Fatal("exec interjection was not unsupported", err)
	}
	if err := a.Interject(t.Context(), "native", engine.Interjection{ID: "bad id", Text: "hello"}); err == nil || errors.Is(err, engine.ErrUnsupported) {
		t.Fatal("interjection was not validated first", err)
	}
	called := false
	err := a.Execute(t.Context(), engine.Request{OwnerID: "owner", Workspace: t.TempDir(), Prompt: "hello", Fork: &engine.ForkPoint{Session: "source"}}, func(engine.Event) error { called = true; return nil })
	if !errors.Is(err, engine.ErrUnsupported) || called {
		t.Fatal("exec fork was not refused before execution", err, called)
	}
}

func TestPiConversationOpsFixtureProcess(t *testing.T) {
	mode := os.Getenv("ALVA_PI_OPS_FIXTURE")
	if mode == "" {
		return
	}
	arg := func(flag string) string {
		for i, value := range os.Args {
			if value == flag && i+1 < len(os.Args) {
				return os.Args[i+1]
			}
		}
		return ""
	}
	path := arg("--session")
	info, err := os.Stat(path)
	if err != nil {
		os.Exit(83)
	}
	if info.Size() == 0 {
		cwd, _ := os.Getwd()
		header, _ := json.Marshal(map[string]any{"type": "session", "version": 3, "id": "pi-ops-native", "cwd": cwd})
		if err := os.WriteFile(path, append(header, '\n'), 0600); err != nil {
			os.Exit(84)
		}
	}
	output := func(value any) { data, _ := json.Marshal(value); fmt.Println(string(data)) }
	output(map[string]any{"type": "extension_ui_request", "id": "ready", "method": "notify", "message": mustJSON(map[string]any{"alvaBridge": 1, "nonce": os.Getenv("ALVA_PI_BRIDGE_NONCE"), "kind": "ready"})})
	userMessage := func(text string) {
		message := map[string]any{"role": "user", "content": []any{map[string]string{"type": "text", "text": text}}, "timestamp": 1}
		output(map[string]any{"type": "message_start", "message": message})
		output(map[string]any{"type": "message_end", "message": message})
	}
	expected, want := []string{"first interjection", "second interjection"}, 2
	if mode == "pi-undelivered" {
		want = 1
	}
	var queued []string
	scanner := bufio.NewScanner(os.Stdin)
	scanner.Buffer(make([]byte, 4096), 1<<20)
	for scanner.Scan() {
		frame := decode(scanner.Bytes())
		kind := str(frame, "type")
		reply := func(data any) {
			output(map[string]any{"type": "response", "id": str(frame, "id"), "command": kind, "success": true, "data": data})
		}
		switch kind {
		case "get_state":
			reply(map[string]any{"sessionId": "pi-ops-native", "sessionFile": path, "isStreaming": false, "isCompacting": false})
		case "prompt":
			reply(map[string]string{"disposition": "started"})
			output(map[string]string{"type": "agent_start"})
			if mode == "pi-echo" {
				// Pi may answer a steer before it emits the prompt's own message.
				continue
			}
			// The prompt's own user message comes first and is no receipt.
			userMessage(str(frame, "message"))
			output(map[string]any{"type": "tool_execution_start", "toolCallId": "read-call", "toolName": "read", "args": map[string]string{"path": "README.md"}})
		case "steer":
			if mode == "pi-echo" {
				if len(frame) != 3 || !strings.HasPrefix(str(frame, "id"), "alva-") || len(queued) != 0 || str(frame, "message") != "pi ops prompt" {
					os.Exit(132)
				}
				queued = append(queued, str(frame, "message"))
				reply(map[string]string{"disposition": "queued"})
				userMessage("pi ops prompt")
				output(map[string]any{"type": "tool_execution_start", "toolCallId": "read-call", "toolName": "read", "args": map[string]string{"path": "README.md"}})
				output(map[string]any{"type": "tool_execution_end", "toolCallId": "read-call", "toolName": "read", "isError": false, "result": map[string]any{"content": []any{map[string]string{"type": "text", "text": "fixture"}}}})
				userMessage("pi ops prompt")
				output(map[string]any{"type": "message_start", "message": map[string]string{"role": "assistant"}})
				output(map[string]any{"type": "message_end", "message": map[string]any{"role": "assistant", "stopReason": "stop", "content": []any{map[string]string{"type": "text", "text": "pi ops answer"}}}})
				output(map[string]any{"type": "agent_settled", "aborted": false})
				continue
			}
			if len(frame) != 3 || !strings.HasPrefix(str(frame, "id"), "alva-") || len(queued) >= want || str(frame, "message") != expected[len(queued)] {
				os.Exit(130)
			}
			queued = append(queued, str(frame, "message"))
			reply(map[string]string{"disposition": "queued"})
			output(map[string]any{"type": "queue_update", "steering": queued, "followUp": []string{}})
			if len(queued) < want {
				continue
			}
			// Pi delivers steers after the running tool batch, in queue order.
			output(map[string]any{"type": "tool_execution_end", "toolCallId": "read-call", "toolName": "read", "isError": false, "result": map[string]any{"content": []any{map[string]string{"type": "text", "text": "fixture"}}}})
			if mode == "pi-steer" {
				for _, text := range queued {
					userMessage(text)
				}
			}
			output(map[string]any{"type": "message_start", "message": map[string]string{"role": "assistant"}})
			output(map[string]any{"type": "message_end", "message": map[string]any{"role": "assistant", "stopReason": "stop", "content": []any{map[string]string{"type": "text", "text": "pi ops answer"}}}})
			output(map[string]any{"type": "agent_end", "willRetry": false})
			output(map[string]any{"type": "agent_settled", "aborted": false})
		default:
			os.Exit(131)
		}
	}
	os.Exit(0)
}

func piOpsFixture(t *testing.T, mode string) *Adapter {
	t.Helper()
	binary, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	directory := filepath.Join(t.TempDir(), "state")
	store, err := bindings.Open(t.Context(), directory)
	if err != nil {
		t.Fatal(err)
	}
	a, err := New(Config{Bindings: bindingPorts{store}, Vendor: "pi", Command: binary, PrefixArgs: []string{"-test.run=^TestPiConversationOpsFixtureProcess$", "--"}, Transport: "rpc", StateDirectory: directory, ApprovedHostAccess: true, Env: map[string]string{"ALVA_PI_OPS_FIXTURE": mode}})
	if err != nil {
		store.Close()
		t.Fatal(err)
	}
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		_ = a.Close(ctx)
	})
	return a
}

func TestPiInterjectionSteersInOrderAndReportsOnlyDeliveredMessages(t *testing.T) {
	for _, tc := range []struct {
		mode      string
		texts     []string
		delivered bool
	}{{"pi-steer", []string{"first interjection", "second interjection"}, true}, {"pi-undelivered", []string{"first interjection"}, false}} {
		t.Run(tc.mode, func(t *testing.T) {
			a := piOpsFixture(t, tc.mode)
			if capabilities := a.Capabilities(); !capabilities.Interjection || capabilities.Fork {
				t.Fatal("Pi RPC must declare interjection and no unverified fork", capabilities)
			}
			if err := a.Interject(t.Context(), "pi-ops-native", engine.Interjection{ID: "early", Text: "first interjection"}); !errors.Is(err, engine.ErrSessionNotFound) {
				t.Fatal("interjection without a running execution was accepted", err)
			}
			var events []engine.Event
			var contract engine.StreamContract
			err := a.Execute(t.Context(), engine.Request{OwnerID: "owner", Workspace: t.TempDir(), Prompt: "pi ops prompt"}, func(event engine.Event) error {
				if err := contract.Accept(event); err != nil {
					return err
				}
				events = append(events, event)
				if event.Kind == engine.ToolStart && event.ID == "read-call" {
					for i, text := range tc.texts {
						if err := a.Interject(t.Context(), event.SessionID, engine.Interjection{ID: fmt.Sprintf("pi-steer-%d", i+1), Text: text}); err != nil {
							return fmt.Errorf("steer was not queued: %w", err)
						}
					}
				}
				return nil
			})
			if err != nil || len(events) == 0 || events[len(events)-1].Kind != engine.Completed || events[len(events)-1].Outcome != engine.Succeeded {
				t.Fatal("steered Pi run did not complete", err, events)
			}
			var receipts []engine.Event
			for _, event := range events {
				if event.Kind == engine.InputDelivered {
					receipts = append(receipts, event)
				}
			}
			if !tc.delivered {
				if len(receipts) != 0 {
					t.Fatal("a steer still queued at settle was reported delivered", receipts)
				}
			} else {
				if len(receipts) != len(tc.texts) {
					t.Fatal("receipts missing or invented (the prompt echo is no receipt)", receipts)
				}
				for i, receipt := range receipts {
					if receipt.ID != fmt.Sprintf("pi-steer-%d", i+1) || receipt.Text != tc.texts[i] || receipt.SessionID != "pi-ops-native" {
						t.Fatal("receipts lost FIFO identity, text or native session", receipts)
					}
				}
				first, last := eventIndex(events, engine.InputDelivered, "pi-steer-1"), eventIndex(events, engine.InputDelivered, "pi-steer-2")
				if answer := messageIndex(events); first < eventIndex(events, engine.ToolEnd, "read-call") || last < first || answer < last {
					t.Fatal("receipts are outside their delivery position", events)
				}
			}
			if err := a.Interject(t.Context(), "pi-ops-native", engine.Interjection{ID: "late", Text: "too late"}); !errors.Is(err, engine.ErrSessionNotFound) {
				t.Fatal("interjection after the execution ended was accepted", err)
			}
			called := false
			err = a.Execute(t.Context(), engine.Request{OwnerID: "owner", Workspace: t.TempDir(), Prompt: "fork", Fork: &engine.ForkPoint{Session: "pi-ops-native", DropTurns: 1}}, func(engine.Event) error { called = true; return nil })
			if !errors.Is(err, engine.ErrUnsupported) || called {
				t.Fatal("unverified Pi fork was not refused before execution", err, called)
			}
		})
	}
}

func TestPiPromptEchoIsNotAnInterjectionReceipt(t *testing.T) {
	a := piOpsFixture(t, "pi-echo")
	input := engine.Interjection{ID: "same-text", Text: "pi ops prompt"}
	ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
	defer cancel()
	steered := make(chan error, 1)
	go func() {
		// Steer as soon as the prompt is a running native run.
		for {
			err := a.Interject(ctx, "pi-ops-native", input)
			if !errors.Is(err, engine.ErrSessionNotFound) || ctx.Err() != nil {
				steered <- err
				return
			}
			time.Sleep(5 * time.Millisecond)
		}
	}()
	var events []engine.Event
	var contract engine.StreamContract
	err := a.Execute(ctx, engine.Request{OwnerID: "owner", Workspace: t.TempDir(), Prompt: "pi ops prompt"}, func(event engine.Event) error {
		events = append(events, event)
		return contract.Accept(event)
	})
	if steerErr := <-steered; steerErr != nil || err != nil || events[len(events)-1].Outcome != engine.Succeeded {
		t.Fatal("same-text steer run failed", steerErr, err, events)
	}
	at := eventIndex(events, engine.InputDelivered, input.ID)
	if at < eventIndex(events, engine.ToolEnd, "read-call") || eventIndex(events[at+1:], engine.InputDelivered, input.ID) >= 0 {
		t.Fatal("the prompt's own message was taken for the steer receipt", events)
	}
}

func messageIndex(events []engine.Event) int {
	for i, event := range events {
		if event.Kind == engine.Message {
			return i
		}
	}
	return -1
}
