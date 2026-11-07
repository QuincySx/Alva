// INPUT: Deterministic owned helper process speaking Codex JSON-RPC over stdio
// OUTPUT: Handshake, callback, native identity, policy, framing and physical cancellation regressions
// POS: [integration / native external-engine adapter] No installed CLI, account, credential or external model is executed
package process

import (
	"bufio"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage/bindings"
)

func TestCodexAppServerFixtureProcess(t *testing.T) {
	mode := os.Getenv("ALVA_CODEX_RPC_FIXTURE")
	if mode == "" {
		return
	}
	if !strings.HasSuffix(strings.Join(os.Args, " "), "app-server --listen stdio://") {
		os.Exit(90)
	}
	if mode == "environment" {
		for key, value := range map[string]string{"HOME": "/fixture/native-home", "USERPROFILE": "/fixture/userprofile", "XDG_CONFIG_HOME": "/fixture/xdg", "MISE_DATA_DIR": "/fixture/mise", "PATH": "/fixture/override-bin", "PYTHONUTF8": "1", "PYTHONIOENCODING": "utf-8", "NO_COLOR": "1", "CLICOLOR": "0", "FORCE_COLOR": "0", "TERM": "dumb", "PSStyle__OutputRendering": "PlainText", "NO_PROXY": "corp.internal,localhost,lower.internal,127.0.0.1,::1"} {
			if os.Getenv(key) != value {
				os.Exit(112)
			}
		}
		if os.Getenv("CODEX_HOME") != os.Getenv("ALVA_EXPECTED_CODEX_HOME") {
			os.Exit(113)
		}
	}
	write := func(value any) { data, _ := json.Marshal(value); fmt.Println(string(data)) }
	notify := func(method string, params map[string]any) {
		params["threadId"] = "rpc-native"
		if method != "thread/tokenUsage/updated" && method != "thread/started" {
			params["turnId"] = "turn-native"
		}
		write(map[string]any{"method": method, "params": params})
	}
	item := func(method, kind, id string, extra map[string]any) {
		data := map[string]any{"type": kind, "id": id}
		for k, v := range extra {
			data[k] = v
		}
		notify(method, map[string]any{"item": data})
	}
	finish := func(status string) {
		item("item/started", "agentMessage", "answer", nil)
		text := "hello app-server"
		if mode == "ask" || mode == "sequential-secret" {
			notify("item/agentMessage/delta", map[string]any{"itemId": "answer", "delta": "human-"})
			notify("item/agentMessage/delta", map[string]any{"itemId": "answer", "delta": "secret-answer"})
			text += "; private answer human-secret-answer; benign tail retained"
			if mode == "sequential-secret" {
				text += "; second answer second-private-answer"
			}
		} else if mode == "environment" {
			notify("item/agentMessage/delta", map[string]any{"itemId": "answer", "delta": strings.Repeat("benign streaming prefix ", 20) + "native-env-"})
			notify("item/agentMessage/delta", map[string]any{"itemId": "answer", "delta": "secret-ABCD; benign streaming suffix"})
			text += "; native-env-secret-ABCD; benign tail retained"
		} else {
			notify("item/agentMessage/delta", map[string]any{"itemId": "answer", "delta": "hello "})
		}
		item("item/completed", "agentMessage", "answer", map[string]any{"text": text})
		notify("thread/tokenUsage/updated", map[string]any{"tokenUsage": map[string]any{"last": map[string]int{"inputTokens": 8, "outputTokens": 5}}})
		if mode == "resume-usage" {
			write(map[string]any{"method": "thread/tokenUsage/updated", "params": map[string]any{"threadId": "rpc-native", "turnId": "previous-turn", "tokenUsage": map[string]any{"last": map[string]int{"inputTokens": 999, "outputTokens": 888}}}})
		}
		notify("turn/completed", map[string]any{"turn": map[string]any{"id": "turn-native", "status": status}})
		if mode == "late" {
			notify("item/agentMessage/delta", map[string]any{"itemId": "answer", "delta": "illegal late output"})
		}
	}
	started, initialized, permission, answer, secondAnswer := false, false, false, false, false
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
			if started {
				os.Exit(92)
			}
			var params struct {
				Capabilities struct {
					Experimental bool `json:"experimentalApi"`
				} `json:"capabilities"`
			}
			if json.Unmarshal(frame.Params, &params) != nil || !params.Capabilities.Experimental {
				os.Exit(104)
			}
			started = true
			if mode == "pending-eof" {
				os.Exit(0)
			}
			reply(map[string]string{"userAgent": "fixture"})
		case "initialized":
			initialized = true
		case "thread/start", "thread/resume":
			if !initialized {
				os.Exit(93)
			}
			var params map[string]any
			json.Unmarshal(frame.Params, &params)
			cwd, _ := os.Getwd()
			if params["cwd"] != cwd || params["sandbox"] != "workspace-write" || params["approvalPolicy"] != "on-request" {
				os.Exit(94)
			}
			if frame.Method == "thread/resume" && params["threadId"] != "rpc-native" {
				os.Exit(95)
			}
			id := "rpc-native"
			if mode == "foreign-thread" {
				id = "foreign-native"
			}
			write(map[string]any{"method": "thread/started", "params": map[string]any{"thread": map[string]any{"id": id}}})
			reply(map[string]any{"thread": map[string]any{"id": id, "cwd": cwd}})
			if mode == "resume-usage" {
				if frame.Method != "thread/resume" {
					os.Exit(110)
				}
				write(map[string]any{"method": "thread/tokenUsage/updated", "params": map[string]any{"threadId": id, "turnId": "previous-turn", "tokenUsage": map[string]any{"last": map[string]int{"inputTokens": 999, "outputTokens": 888}}}})
			}
		case "turn/start":
			var params struct {
				Sandbox struct {
					Type       string   `json:"type"`
					Roots      []string `json:"writableRoots"`
					Network    bool     `json:"networkAccess"`
					ExcludeEnv bool     `json:"excludeTmpdirEnvVar"`
					ExcludeTmp bool     `json:"excludeSlashTmp"`
				} `json:"sandboxPolicy"`
				Approval string                       `json:"approvalPolicy"`
				Input    []map[string]json.RawMessage `json:"input"`
			}
			if json.Unmarshal(frame.Params, &params) != nil || params.Sandbox.Type != "workspaceWrite" || len(params.Sandbox.Roots) != 1 || params.Sandbox.Network || !params.Sandbox.ExcludeEnv || !params.Sandbox.ExcludeTmp || params.Approval != "on-request" || len(params.Input) == 0 || string(params.Input[0]["text_elements"]) != "[]" {
				os.Exit(96)
			}
			if mode == "files" {
				if len(params.Input) != 3 || string(params.Input[2]["type"]) != `"localImage"` {
					os.Exit(105)
				}
				var path string
				json.Unmarshal(params.Input[2]["path"], &path)
				if path != os.Getenv("ALVA_CODEX_INPUT_FILE") || !strings.Contains(string(params.Input[1]["text"]), "application/pdf") || !strings.Contains(string(params.Input[1]["text"]), "captured-text.txt") {
					os.Exit(106)
				}
			}
			if mode == "skills" {
				if len(params.Input) != 2 || string(params.Input[1]["type"]) != `"skill"` || string(params.Input[1]["name"]) != `"selected-skill"` {
					os.Exit(108)
				}
				var path string
				json.Unmarshal(params.Input[1]["path"], &path)
				if path != os.Getenv("ALVA_CODEX_SKILL_FILE") {
					os.Exit(109)
				}
			}
			reply(map[string]any{"turn": map[string]string{"id": "turn-native", "status": "inProgress"}})
			if mode == "malformed" {
				fmt.Println("not JSON")
				continue
			}
			if mode == "partial" {
				fmt.Print(`{"method":"notice"}`)
				os.Exit(0)
			}
			if mode == "oversize" {
				fmt.Println(strings.Repeat("x", (1<<20)+1))
				os.Exit(0)
			}
			if mode == "ask" || mode == "sequential-secret" || mode == "secret-identity" || mode == "cancel" || mode == "resolved" {
				item("item/started", "commandExecution", "cmd", map[string]any{"command": "echo fixture", "cwd": "/fixture", "status": "inProgress"})
				write(map[string]any{"id": 7, "method": "item/commandExecution/requestApproval", "params": map[string]any{"threadId": "rpc-native", "turnId": "turn-native", "itemId": "cmd", "command": "echo fixture", "availableDecisions": []string{"accept", "decline"}}})
				if mode == "ask" || mode == "sequential-secret" || mode == "secret-identity" || mode == "resolved" {
					write(map[string]any{"id": "7", "method": "item/tool/requestUserInput", "params": map[string]any{"threadId": "rpc-native", "turnId": "turn-native", "itemId": "question", "questions": []any{map[string]any{"id": "color", "header": "Color", "question": "Pick a color", "isOther": true, "isSecret": true, "options": []any{map[string]string{"label": "Blue", "description": "A blue color"}}}}}})
				}
				if mode == "resolved" {
					notify("serverRequest/resolved", map[string]any{"requestId": "7"})
					notify("serverRequest/resolved", map[string]any{"requestId": 7})
					item("item/completed", "commandExecution", "cmd", map[string]any{"status": "declined"})
					finish("completed")
				}
				continue
			}
			if mode == "unknown" {
				write(map[string]any{"id": 12, "method": "unknown/execute", "params": map[string]string{"threadId": "rpc-native", "turnId": "turn-native"}})
				continue
			}
			if mode == "child" {
				item("item/started", "commandExecution", "cmd", map[string]any{"status": "inProgress"})
				write(map[string]any{"method": "thread/started", "params": map[string]any{"thread": map[string]string{"id": "child-thread"}}})
				write(map[string]any{"method": "item/completed", "params": map[string]any{"threadId": "child-thread", "turnId": "child-turn", "item": map[string]string{"id": "cmd", "type": "commandExecution", "status": "completed"}}})
				write(map[string]any{"method": "turn/completed", "params": map[string]any{"threadId": "child-thread", "turn": map[string]string{"id": "child-turn", "status": "completed"}}})
				write(map[string]any{"id": 99, "method": "item/commandExecution/requestApproval", "params": map[string]string{"threadId": "child-thread", "turnId": "child-turn", "itemId": "cmd"}})
				continue
			}
			if mode == "resume-usage" {
				data, err := os.ReadFile("native-fixture.txt")
				if err != nil {
					os.Exit(111)
				}
				item("item/started", "commandExecution", "read-fixture", map[string]any{"command": "read native-fixture.txt", "status": "inProgress"})
				item("item/completed", "commandExecution", "read-fixture", map[string]any{"status": "completed", "aggregatedOutput": string(data), "exitCode": 0})
			}
			if mode == "environment" {
				item("item/started", "reasoning", "reason", nil)
				notify("item/reasoning/textDelta", map[string]any{"itemId": "reason", "delta": strings.Repeat("benign reasoning prefix ", 20) + "native-env-"})
				notify("item/reasoning/textDelta", map[string]any{"itemId": "reason", "delta": "secret-ABCD; benign reasoning suffix"})
				item("item/completed", "reasoning", "reason", nil)
				item("item/started", "commandExecution", "large-tool", map[string]any{"command": "owned fixture output", "status": "inProgress"})
				item("item/completed", "commandExecution", "large-tool", map[string]any{"status": "completed", "aggregatedOutput": "large-before " + strings.Repeat("x", 70<<10) + " native-env-secret-ABCD large-after", "exitCode": 0})
				for _, echo := range []string{"native-env-secret-ABCD", "native-env-", "secret-ABCD"} {
					notify("fixture/notice", map[string]any{"opaqueEcho": echo})
					fmt.Fprintln(os.Stderr, echo)
				}
			}
			finish("completed")
		case "turn/interrupt":
			if mode != "cancel" {
				os.Exit(97)
			}
			if path := os.Getenv("ALVA_CODEX_INTERRUPT_FILE"); path != "" {
				os.WriteFile(path, []byte("interrupt"), 0600)
			}
			reply(map[string]any{})
			item("item/completed", "commandExecution", "cmd", map[string]any{"status": "declined"})
			notify("turn/completed", map[string]any{"turn": map[string]string{"id": "turn-native", "status": "interrupted"}})
		case "":
			id, _ := codexRequestID(frame.ID)
			if mode == "child" {
				if id != "rpc-n:99" || len(frame.Error) == 0 {
					os.Exit(107)
				}
				item("item/completed", "commandExecution", "cmd", map[string]any{"status": "completed"})
				finish("completed")
				continue
			}
			if mode == "unknown" {
				if len(frame.Error) == 0 {
					os.Exit(98)
				}
				finish("completed")
				continue
			}
			if mode != "ask" && mode != "sequential-secret" && mode != "secret-identity" {
				os.Exit(99)
			}
			if id == "rpc-n:7" {
				if string(frame.Result) != `{"decision":"accept"}` {
					os.Exit(100)
				}
				permission = true
			} else if id == "rpc-s:7" || mode == "sequential-secret" && id == "rpc-s:followup" {
				var result struct {
					Answers map[string]struct {
						Answers []string `json:"answers"`
					} `json:"answers"`
				}
				json.Unmarshal(frame.Result, &result)
				questionID, expected := "color", "human-secret-answer"
				if id == "rpc-s:followup" {
					questionID, expected = "credential-2", "second-private-answer"
				}
				if len(result.Answers[questionID].Answers) != 1 || result.Answers[questionID].Answers[0] != expected {
					os.Exit(101)
				}
				if id == "rpc-s:followup" {
					secondAnswer = true
				} else {
					answer = true
				}
				notify("fixture/notice", map[string]any{"opaqueEcho": "human-secret-answer"})
				notify("fixture/notice", map[string]any{"opaqueEcho": "human-"})
				notify("fixture/notice", map[string]any{"opaqueEcho": "secret-answer"})
				fmt.Fprintln(os.Stderr, "private answer echo human-secret-answer")
				fmt.Fprintln(os.Stderr, "human-")
				fmt.Fprintln(os.Stderr, "secret-answer")
			} else {
				os.Exit(102)
			}
			notify("serverRequest/resolved", map[string]any{"requestId": frame.ID})
			if permission && answer {
				if mode == "sequential-secret" && !secondAnswer {
					write(map[string]any{"id": "followup", "method": "item/tool/requestUserInput", "params": map[string]any{"threadId": "rpc-native", "turnId": "turn-native", "itemId": "followup", "questions": []any{map[string]any{"id": "credential-2", "header": "Second credential", "question": "Replace human-secret-answer with a second credential", "isOther": true, "isSecret": true, "options": []any{map[string]string{"label": "Continue", "description": "Replace human-secret-answer"}}}}}})
					continue
				}
				if mode == "secret-identity" {
					write(map[string]any{"id": "collision", "method": "item/tool/requestUserInput", "params": map[string]any{"threadId": "rpc-native", "turnId": "turn-native", "itemId": "followup", "questions": []any{map[string]any{"id": "human-secret-answer", "header": "Followup", "question": "Choose", "isOther": false, "isSecret": false, "options": []any{map[string]string{"label": "Continue"}}}}}})
					continue
				}
				item("item/completed", "commandExecution", "cmd", map[string]any{"status": "completed", "aggregatedOutput": "tool result human-secret-answer", "exitCode": 0})
				finish("completed")
			}
		default:
			os.Exit(103)
		}
	}
	if path := os.Getenv("ALVA_CODEX_EXIT_FILE"); path != "" {
		os.WriteFile(path, []byte("joined"), 0600)
	}
	if mode == "nonzero" {
		os.Exit(9)
	}
	if mode == "hang" {
		for {
			time.Sleep(time.Second)
		}
	}
	os.Exit(0)
}

func codexAppServerFixture(t *testing.T, mode string) (*Adapter, string) {
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
	a, err := New(Config{Bindings: bindingPorts{store}, Vendor: "codex", Transport: "app-server", CodexSandbox: "workspaceWrite", Command: binary, PrefixArgs: []string{"-test.run=^TestCodexAppServerFixtureProcess$", "--"}, StateDirectory: dir, ApprovedHostAccess: true, Timeout: 5 * time.Second, Env: map[string]string{"ALVA_CODEX_RPC_FIXTURE": mode, "FIXTURE_TOKEN": "marker-secret-12345"}})
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

func TestCodexAppServerBidirectionalCallbacksAndJournal(t *testing.T) {
	a, dir := codexAppServerFixture(t, "ask")
	if !a.Capabilities().PermissionCallback || !a.Capabilities().UserInputCallback || !a.Capabilities().FileInputs {
		t.Fatal("callback capability missing")
	}
	var observed []engine.Event
	err := a.Execute(t.Context(), engine.Request{OwnerID: "owner", Workspace: t.TempDir(), Prompt: "fixture prompt"}, func(event engine.Event) error {
		observed = append(observed, event)
		switch event.Kind {
		case engine.PermissionRequest:
			if event.ID != "rpc-n:7" {
				t.Fatal(event.ID)
			}
			if err := a.RespondUserInput(t.Context(), event.SessionID, event.ID, engine.UserInputResponse{}); !errors.Is(err, engine.ErrSessionNotFound) {
				t.Fatal("question answered permission", err)
			}
			return a.RespondPermission(t.Context(), event.SessionID, event.ID, engine.PermissionDecision{Allow: true})
		case engine.UserInputRequested:
			if event.ID != "rpc-s:7" || !event.Question.Questions[0].IsSecret {
				t.Fatal("question metadata missing", event)
			}
			if err := a.RespondPermission(t.Context(), event.SessionID, event.ID, engine.PermissionDecision{Allow: true}); !errors.Is(err, engine.ErrSessionNotFound) {
				t.Fatal("permission answered question", err)
			}
			return a.RespondUserInput(t.Context(), event.SessionID, event.ID, engine.UserInputResponse{Answers: map[string][]string{"color": {"human-secret-answer"}}})
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	last := observed[len(observed)-1]
	if last.Outcome != engine.Succeeded || last.Usage.InputTokens != 8 || last.Usage.OutputTokens != 5 {
		t.Fatal("native success/usage missing", observed)
	}
	resolved := 0
	var reconstructed strings.Builder
	for _, event := range observed {
		if event.Kind == engine.MessageDelta || event.Kind == engine.ReasoningDelta {
			t.Fatal("post-secret streaming fragments reached product events")
		}
		if event.Kind == engine.Message {
			reconstructed.Reset()
			reconstructed.WriteString(event.Text)
		}
		if event.Kind == engine.InteractionResolved {
			resolved++
		}
	}
	serialized, _ := json.Marshal(observed)
	for _, forbidden := range []string{"human-secret-answer", "human-", "secret-answer"} {
		if strings.Contains(string(serialized), forbidden) || strings.Contains(reconstructed.String(), forbidden) {
			t.Fatal("secret or split fragment leaked through product event", forbidden)
		}
	}
	if !strings.Contains(reconstructed.String(), "hello app-server") || !strings.Contains(reconstructed.String(), "[REDACTED]") || !strings.Contains(reconstructed.String(), "benign tail retained") {
		t.Fatal("completed answer was not usefully redacted", reconstructed.String())
	}
	if resolved != 2 {
		t.Fatal("callbacks were not resolved", resolved)
	}
	files, _ := os.ReadDir(filepath.Join(dir, "journals"))
	data, _ := os.ReadFile(filepath.Join(dir, "journals", files[0].Name()))
	for _, forbidden := range []string{"human-secret-answer", "human-", "secret-answer"} {
		if strings.Contains(string(data), forbidden) {
			t.Fatal("human answer or native split echo leaked to journal", forbidden)
		}
	}
	if !strings.Contains(string(data), "human input response body omitted") {
		t.Fatal("response omission not journaled")
	}
	if err := a.RespondPermission(t.Context(), "rpc-native", "rpc-n:7", engine.PermissionDecision{Allow: true}); !errors.Is(err, engine.ErrSessionNotFound) {
		t.Fatal("late callback accepted", err)
	}
}

func TestCodexAppServerSequentialSecretQuestions(t *testing.T) {
	a, dir := codexAppServerFixture(t, "sequential-secret")
	var observed []engine.Event
	var owner *codexRun
	questions := 0
	err := a.Execute(t.Context(), engine.Request{OwnerID: "owner", Workspace: t.TempDir(), Prompt: "fixture"}, func(event engine.Event) error {
		observed = append(observed, event)
		switch event.Kind {
		case engine.PermissionRequest:
			return a.RespondPermission(t.Context(), event.SessionID, event.ID, engine.PermissionDecision{Allow: true})
		case engine.UserInputRequested:
			questions++
			if event.Question == nil || len(event.Question.Questions) != 1 {
				return errors.New("secret question payload missing")
			}
			question := event.Question.Questions[0]
			if !question.IsSecret || !question.IsOther {
				return errors.New("secret question control flags changed")
			}
			owner = a.codexRun(event.SessionID)
			if questions == 1 {
				if event.ID != "rpc-s:7" || question.ID != "color" {
					return errors.New("first secret question routing changed")
				}
				return a.RespondUserInput(t.Context(), event.SessionID, event.ID, engine.UserInputResponse{Answers: map[string][]string{"color": {"human-secret-answer"}}})
			}
			if questions != 2 || event.ID != "rpc-s:followup" || question.ID != "credential-2" || question.Options[0].Label != "Continue" {
				return errors.New("second secret question routing changed")
			}
			if question.Question != "Replace [REDACTED] with a second credential" || question.Options[0].Description != "Replace [REDACTED]" {
				return errors.New("second secret question text was not redacted")
			}
			owner.mu.Lock()
			pending := owner.pending[event.ID]
			owner.mu.Unlock()
			if pending.question.Questions[0].Question != "Replace human-secret-answer with a second credential" || !pending.question.Questions[0].IsSecret {
				return errors.New("question projection changed the native pending request")
			}
			return a.RespondUserInput(t.Context(), event.SessionID, event.ID, engine.UserInputResponse{Answers: map[string][]string{"credential-2": {"second-private-answer"}}})
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	if questions != 2 || len(observed) == 0 || observed[len(observed)-1].Outcome != engine.Succeeded {
		t.Fatal("sequential secret questions did not complete", questions, observed)
	}
	if owner == nil || !owner.rpc.proc.CleanupComplete() {
		t.Fatal("sequential secret questions did not join the native process")
	}
	data, err := json.Marshal(observed)
	if err != nil {
		t.Fatal(err)
	}
	files, err := os.ReadDir(filepath.Join(dir, "journals"))
	if err != nil || len(files) != 1 {
		t.Fatal("native journal missing", err)
	}
	journalData, err := os.ReadFile(filepath.Join(dir, "journals", files[0].Name()))
	if err != nil {
		t.Fatal(err)
	}
	for _, secret := range []string{"human-secret-answer", "human-", "secret-answer", "second-private-answer"} {
		if strings.Contains(string(data), secret) || strings.Contains(string(journalData), secret) {
			t.Fatal("sequential secret answer leaked", secret)
		}
	}
	if !strings.Contains(string(data), "hello app-server") || !strings.Contains(string(data), "benign tail retained") {
		t.Fatal("safe completed aggregate was lost")
	}
}

func TestCodexAppServerNativeResultAndPhysicalExit(t *testing.T) {
	for _, mode := range []string{"ok", "unknown", "child", "hang", "late", "malformed", "partial", "oversize", "nonzero", "pending-eof"} {
		t.Run(mode, func(t *testing.T) {
			a, _ := codexAppServerFixture(t, mode)
			var terminal engine.Event
			if err := a.Execute(t.Context(), engine.Request{OwnerID: "owner", Workspace: t.TempDir(), Prompt: "prompt"}, func(event engine.Event) error {
				if event.Kind == engine.Completed {
					terminal = event
				}
				return nil
			}); err != nil {
				t.Fatal(err)
			}
			want := engine.Failed
			if mode == "ok" || mode == "unknown" || mode == "child" || mode == "hang" {
				want = engine.Succeeded
			}
			if terminal.Outcome != want {
				t.Fatalf("%s: want %s got %+v", mode, want, terminal)
			}
		})
	}
}

func TestCodexAppServerCancelInterruptsAndJoinsPendingNativeRequest(t *testing.T) {
	a, _ := codexAppServerFixture(t, "cancel")
	dir := t.TempDir()
	interrupt := filepath.Join(dir, "interrupt")
	exited := filepath.Join(dir, "exit")
	a.config.Env["ALVA_CODEX_INTERRUPT_FILE"] = interrupt
	a.config.Env["ALVA_CODEX_EXIT_FILE"] = exited
	ready := make(chan struct{})
	finished := make(chan error, 1)
	var terminal engine.Outcome
	go func() {
		finished <- a.Execute(t.Context(), engine.Request{OwnerID: "owner", Workspace: dir, Prompt: "prompt"}, func(event engine.Event) error {
			if event.Kind == engine.PermissionRequest {
				close(ready)
			}
			if event.Kind == engine.Completed {
				terminal = event.Outcome
			}
			return nil
		})
	}()
	select {
	case <-ready:
	case <-time.After(3 * time.Second):
		t.Fatal("no native callback")
	}
	ctx, cancel := context.WithTimeout(t.Context(), 3*time.Second)
	defer cancel()
	if err := a.Cancel(ctx, "rpc-native"); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(interrupt); err != nil {
		t.Fatal("native interrupt was skipped", err)
	}
	if _, err := os.Stat(exited); err != nil {
		t.Fatal("cancel returned before physical child exit", err)
	}
	if err := <-finished; err != nil || terminal != engine.Cancelled {
		t.Fatal("cancel terminal", terminal, err)
	}
	if err := a.RespondPermission(ctx, "rpc-native", "rpc-n:7", engine.PermissionDecision{Allow: true}); !errors.Is(err, engine.ErrSessionNotFound) {
		t.Fatal("cancel retained pending approval", err)
	}
}

func TestCodexAppServerResumeOwnerCwdAndNativeIdentity(t *testing.T) {
	a, dir := codexAppServerFixture(t, "ok")
	workspace := t.TempDir()
	request := engine.Request{OwnerID: "owner", Workspace: workspace, Prompt: "first"}
	if err := a.Execute(t.Context(), request, func(engine.Event) error { return nil }); err != nil {
		t.Fatal(err)
	}
	request.ResumeSession = "rpc-native"
	request.OwnerID = "foreign"
	if err := a.Execute(t.Context(), request, func(engine.Event) error { t.Fatal("foreign owner emitted"); return nil }); !errors.Is(err, engine.ErrSessionNotFound) {
		t.Fatal(err)
	}
	request.OwnerID = "owner"
	request.Workspace = t.TempDir()
	if err := a.Execute(t.Context(), request, func(engine.Event) error { t.Fatal("foreign cwd emitted"); return nil }); !errors.Is(err, engine.ErrSessionNotFound) {
		t.Fatal(err)
	}
	request.Workspace = workspace
	if err := a.Execute(t.Context(), request, func(engine.Event) error { return nil }); err != nil {
		t.Fatal(err)
	}
	files, _ := os.ReadDir(filepath.Join(dir, "journals"))
	if len(files) != 2 {
		t.Fatal("foreign request started IO", len(files))
	}
	a.config.Env["ALVA_CODEX_RPC_FIXTURE"] = "foreign-thread"
	var outcome engine.Outcome
	if err := a.Execute(t.Context(), request, func(event engine.Event) error {
		if event.Kind == engine.Completed {
			outcome = event.Outcome
		}
		return nil
	}); err != nil || outcome != engine.Failed {
		t.Fatal("changed native identity resumed", outcome, err)
	}
}

func TestCodexAppServerResumeHistoricalUsageKeepsCurrentToolResults(t *testing.T) {
	a, dir := codexAppServerFixture(t, "ok")
	workspace := t.TempDir()
	const contents = "READ_ONLY_FIXTURE_RESULT_20261026\n"
	if err := os.WriteFile(filepath.Join(workspace, "native-fixture.txt"), []byte(contents), 0600); err != nil {
		t.Fatal(err)
	}
	request := engine.Request{OwnerID: "owner", Workspace: workspace, Prompt: "create fixture session"}
	if err := a.Execute(t.Context(), request, func(engine.Event) error { return nil }); err != nil {
		t.Fatal(err)
	}
	a.config.Env["ALVA_CODEX_RPC_FIXTURE"] = "resume-usage"
	request.ResumeSession, request.Prompt = "rpc-native", "read owned fixture"
	var events []engine.Event
	var owner *codexRun
	if err := a.Execute(t.Context(), request, func(event engine.Event) error {
		events = append(events, event)
		if event.Kind == engine.SessionStarted {
			owner = a.codexRun(event.SessionID)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	last := events[len(events)-1]
	if last.Outcome != engine.Succeeded || last.Usage == nil || last.Usage.InputTokens != 8 || last.Usage.OutputTokens != 5 {
		t.Fatal("historical resume usage failed or replaced current turn measurements", last)
	}
	starts, ends := 0, 0
	for _, event := range events {
		if event.Kind == engine.ToolStart && event.ID == "read-fixture" && event.Name == "commandExecution" {
			starts++
		}
		if event.Kind == engine.ToolEnd && event.ID == "read-fixture" && event.Name == "commandExecution" && !event.IsError && event.Text == contents {
			ends++
		}
	}
	if starts != 1 || ends != 1 || owner == nil || !owner.rpc.proc.CleanupComplete() {
		t.Fatal("resume did not retain exact tool pairing and physical cleanup", starts, ends)
	}
	journalPath := filepath.Join(dir, "journals", strings.TrimPrefix(owner.journalURI, "engine-journal://")+".jsonl")
	data, err := os.ReadFile(journalPath)
	if err != nil {
		t.Fatal(err)
	}
	if snapshots := strings.Count(string(data), "previous-turn"); snapshots != 2 {
		t.Fatal("historical native usage snapshots were not retained in the private journal", snapshots)
	}
}

func TestCodexHistoricalUsageCannotRelaxTurnAuthority(t *testing.T) {
	for _, request := range []bool{false, true} {
		t.Run(fmt.Sprint(request), func(t *testing.T) {
			run := codexRun{session: "bound-thread", turnID: "current-turn", usage: engine.Usage{InputTokens: 8, OutputTokens: 5}}
			frame := codexFrame{Method: "thread/tokenUsage/updated", Params: json.RawMessage(`{"threadId":"bound-thread","turnId":"previous-turn","tokenUsage":{"last":{"inputTokens":999,"outputTokens":888}}}`)}
			if request {
				frame.ID = json.RawMessage(`7`)
			}
			if err := run.handle(t.Context(), frame); (err != nil) != request {
				t.Fatal("historical usage request gained notification authority", err)
			}
			if run.usage.InputTokens != 8 || run.usage.OutputTokens != 5 || run.finished {
				t.Fatal("historical usage changed active turn facts")
			}
			frame.Method, frame.ID = "item/started", nil
			if err := run.handle(t.Context(), frame); err == nil {
				t.Fatal("historical usage exception admitted a foreign-turn tool")
			}
		})
	}
}

func TestCodexAppServerNativeResolvesUnansweredPrompts(t *testing.T) {
	a, _ := codexAppServerFixture(t, "resolved")
	resolved := 0
	err := a.Execute(t.Context(), engine.Request{OwnerID: "owner", Workspace: t.TempDir(), Prompt: "prompt"}, func(event engine.Event) error {
		if event.Kind == engine.InteractionResolved {
			resolved++
			if err := a.RespondPermission(t.Context(), event.SessionID, event.ID, engine.PermissionDecision{}); !errors.Is(err, engine.ErrSessionNotFound) {
				t.Fatal("resolved callback still pending", err)
			}
		}
		return nil
	})
	if err != nil || resolved != 2 {
		t.Fatal("native pending cleanup missing", resolved, err)
	}
}

func TestCodexAppServerPoliciesAreExplicitAndBounded(t *testing.T) {
	for _, config := range []Config{
		{Vendor: "claude", Transport: "app-server"}, {Vendor: "codex", Transport: "unknown"}, {Vendor: "codex", CodexSandbox: "workspaceWrite"}, {Vendor: "codex", Transport: "app-server", CodexSandbox: "dangerFullAccess"}, {Vendor: "codex", Transport: "app-server", CodexApprovalPolicy: "auto"},
	} {
		if err := validateCodexTransport(&config); err == nil {
			t.Fatal("unsupported policy accepted", config)
		}
	}
	config := Config{Vendor: "codex", Transport: "app-server"}
	if err := validateCodexTransport(&config); err != nil || config.CodexSandbox != "readOnly" || config.CodexApprovalPolicy != "onRequest" {
		t.Fatal("conservative native defaults missing", config, err)
	}
}

func TestCodexAppServerTypedFilesUseNativeImageAndSeparateOriginalTextPaths(t *testing.T) {
	a, _ := codexAppServerFixture(t, "files")
	dir := t.TempDir()
	imagePath := filepath.Join(dir, "captured-image.png")
	documentPath := filepath.Join(dir, "captured-document.pdf")
	textPath := filepath.Join(dir, "captured-text.txt")
	for _, path := range []string{imagePath, documentPath, textPath} {
		if err := os.WriteFile(path, []byte("fixture"), 0600); err != nil {
			t.Fatal(err)
		}
	}
	a.config.Env["ALVA_CODEX_INPUT_FILE"] = imagePath
	digest := sha256.Sum256([]byte("fixture"))
	hash := hex.EncodeToString(digest[:])
	files := []engine.InputFile{
		{ID: "image", Name: "image.png", URI: "artifact://image", Path: imagePath, MediaType: "image/png", SHA256: hash, Bytes: 7},
		{ID: "document", Name: "document.pdf", URI: "artifact://pdf", Path: documentPath, MediaType: "application/pdf", SHA256: hash, Bytes: 7, TextURI: "artifact://text", TextPath: textPath, TextSHA256: hash, TextBytes: 7},
	}
	var outcome engine.Outcome
	if err := a.Execute(t.Context(), engine.Request{OwnerID: "owner", Workspace: dir, Prompt: "read selected originals", InputFiles: files}, func(event engine.Event) error {
		if event.Kind == engine.Completed {
			outcome = event.Outcome
		}
		return nil
	}); err != nil || outcome != engine.Succeeded {
		t.Fatal("typed files did not reach native transport", outcome, err)
	}
	execAdapter, _ := fixtureAdapter(t, "codex", "ok")
	if execAdapter.Capabilities().FileInputs {
		t.Fatal("exec advertised a missing file input channel")
	}
	if err := execAdapter.Execute(t.Context(), engine.Request{InputFiles: files}, func(engine.Event) error { t.Fatal("unsupported files started execution"); return nil }); !errors.Is(err, engine.ErrUnsupported) {
		t.Fatal("exec silently ignored file inputs", err)
	}
}

func TestCodexAppServerRejectsChangedCaptureBeforeNativeExecution(t *testing.T) {
	for _, kind := range []string{"original-content", "original-size", "derivative-content", "missing", "symlink"} {
		t.Run(kind, func(t *testing.T) {
			a, _ := codexAppServerFixture(t, "files")
			dir := t.TempDir()
			original, derivative := filepath.Join(dir, "original"), filepath.Join(dir, "text")
			data := []byte("fixture")
			for _, path := range []string{original, derivative} {
				if err := os.WriteFile(path, data, 0600); err != nil {
					t.Fatal(err)
				}
			}
			digest := sha256.Sum256(data)
			hash := hex.EncodeToString(digest[:])
			files := []engine.InputFile{
				{ID: "image", Name: "image.png", URI: "artifact://image", Path: original, MediaType: "image/png", SHA256: hash, Bytes: 7},
				{ID: "document", Name: "document.pdf", URI: "artifact://pdf", Path: original, MediaType: "application/pdf", SHA256: hash, Bytes: 7, TextURI: "artifact://text", TextPath: derivative, TextSHA256: hash, TextBytes: 7},
			}
			a.config.Env["ALVA_CODEX_INPUT_FILE"] = original
			switch kind {
			case "original-content":
				if err := os.WriteFile(original, []byte("changed"), 0600); err != nil {
					t.Fatal(err)
				}
			case "original-size":
				if err := os.WriteFile(original, []byte("longer original"), 0600); err != nil {
					t.Fatal(err)
				}
			case "derivative-content":
				if err := os.WriteFile(derivative, []byte("changed"), 0600); err != nil {
					t.Fatal(err)
				}
			case "missing":
				if err := os.Remove(original); err != nil {
					t.Fatal(err)
				}
			case "symlink":
				link := filepath.Join(dir, "alias")
				if err := os.Symlink(original, link); err != nil {
					t.Fatal(err)
				}
				files[0].Path = link
			}
			called := false
			err := a.Execute(t.Context(), engine.Request{OwnerID: "owner", Workspace: dir, Prompt: "read the frozen capture", InputFiles: files}, func(engine.Event) error {
				called = true
				return nil
			})
			if err == nil || called || !strings.Contains(err.Error(), "selected file capture") {
				t.Fatal("changed capture started native execution or was accepted", kind, called, err)
			}
		})
	}
}

func TestCodexAppServerSelectedSkillUsesNativeInputAndRejectsChangedSource(t *testing.T) {
	a, _ := codexAppServerFixture(t, "skills")
	dir, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, "SKILL.md")
	data := []byte("---\nname: selected-skill\ndescription: Explicit fixture\n---\nUse selected knowledge.\n")
	if err := os.WriteFile(path, data, 0600); err != nil {
		t.Fatal(err)
	}
	sum := sha256.Sum256(data)
	selected := []engine.InputSkill{{Name: "selected-skill", Path: path, SHA256: hex.EncodeToString(sum[:])}}
	a.config.Env["ALVA_CODEX_SKILL_FILE"] = path
	var outcome engine.Outcome
	request := engine.Request{OwnerID: "owner", Workspace: dir, Prompt: "use selected Skill", InputSkills: selected}
	if err := a.Execute(t.Context(), request, func(event engine.Event) error {
		if event.Kind == engine.Completed {
			outcome = event.Outcome
		}
		return nil
	}); err != nil || outcome != engine.Succeeded || !a.Capabilities().SkillInputs {
		t.Fatal("selected Skill did not reach native transport", outcome, err)
	}
	if err := os.WriteFile(path, []byte("changed after selection"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := a.Execute(t.Context(), request, func(engine.Event) error { t.Fatal("changed Skill started native execution"); return nil }); err == nil || !strings.Contains(err.Error(), "changed since selection") {
		t.Fatal("changed Skill was accepted", err)
	}
	if err := os.Truncate(path, 1<<20+1); err != nil {
		t.Fatal(err)
	}
	if err := verifyCodexInputSkills(t.Context(), selected); err == nil {
		t.Fatal("unbounded Skill source accepted")
	}
	execAdapter, _ := fixtureAdapter(t, "codex", "ok")
	if execAdapter.Capabilities().SkillInputs {
		t.Fatal("exec advertised a missing Skill channel")
	}
	if err := execAdapter.Execute(t.Context(), engine.Request{InputSkills: selected}, func(engine.Event) error { t.Fatal("unsupported Skill started execution"); return nil }); !errors.Is(err, engine.ErrUnsupported) {
		t.Fatal("exec silently ignored Skill selection", err)
	}
}

func TestCodexAppServerCancelledConsumedResponsesCloseAndReapOriginalProcess(t *testing.T) {
	for _, kind := range []engine.Kind{engine.PermissionRequest, engine.UserInputRequested} {
		t.Run(string(kind), func(t *testing.T) {
			a, _ := codexAppServerFixture(t, "ask")
			ready := make(chan engine.Event, 1)
			finished := make(chan error, 1)
			go func() {
				finished <- a.Execute(t.Context(), engine.Request{OwnerID: "owner", Workspace: t.TempDir(), Prompt: "fixture"}, func(event engine.Event) error {
					if event.Kind == kind {
						ready <- event
					}
					return nil
				})
			}()
			var event engine.Event
			select {
			case event = <-ready:
			case <-time.After(2 * time.Second):
				t.Fatal("native callback did not arrive")
			}
			run := a.codexRun(event.SessionID)
			if run == nil {
				t.Fatal("missing actual native owner")
			}
			run.rpc.journal.mu.Lock()
			responseCtx, cancel := context.WithCancel(t.Context())
			responseDone := make(chan error, 1)
			go func() {
				if kind == engine.PermissionRequest {
					responseDone <- a.RespondPermission(responseCtx, event.SessionID, event.ID, engine.PermissionDecision{Allow: true})
				} else {
					responseDone <- a.RespondUserInput(responseCtx, event.SessionID, event.ID, engine.UserInputResponse{Answers: map[string][]string{"color": {"human-secret-answer"}}})
				}
			}()
			removed := false
			deadline := time.Now().Add(time.Second)
			for time.Now().Before(deadline) {
				run.mu.Lock()
				_, pending := run.pending[event.ID]
				run.mu.Unlock()
				if !pending {
					removed = true
					break
				}
				time.Sleep(time.Millisecond)
			}
			cancel()
			run.rpc.journal.mu.Unlock()
			if !removed {
				t.Fatal("fixture did not reach consumed response before dispatch")
			}
			select {
			case err := <-responseDone:
				if !errors.Is(err, context.Canceled) {
					t.Fatal("response lost cancellation cause", err)
				}
			case <-time.After(2 * time.Second):
				t.Fatal("response did not settle")
			}
			select {
			case <-run.rpc.done:
			case <-time.After(2 * time.Second):
				t.Fatal("consumed response failure left native RPC open")
			}
			select {
			case <-run.rpc.proc.Done():
			case <-time.After(2 * time.Second):
				t.Fatal("consumed response failure did not physically join")
			}
			if !run.rpc.proc.CleanupComplete() {
				t.Fatal("response failure reported cleanup without physical reap")
			}
			select {
			case err := <-finished:
				if err != nil {
					t.Fatal(err)
				}
			case <-time.After(2 * time.Second):
				t.Fatal("original execution remained blocked on an unanswerable request")
			}
		})
	}
}

func TestCodexPrivateCompletedPayloadsRedactQuestionsAndPermissions(t *testing.T) {
	var observed []engine.Event
	run := codexRun{session: "native", rpc: &codexRPC{journal: &journal{privateOutput: true, secrets: []string{"private-value"}}}, emit: func(event engine.Event) error { observed = append(observed, event); return nil }}
	question := &engine.UserInputRequest{Questions: []engine.UserInputQuestion{{ID: "followup", Header: "private-value", Question: "About private-value", IsSecret: true, IsOther: true, Options: []engine.UserInputOption{{Label: "Continue", Description: "private-value"}}}}}
	for _, event := range []engine.Event{
		{Kind: engine.Message, ID: "message", Text: "Completed private-value safely"},
		{Kind: engine.ToolEnd, ID: "tool", Input: json.RawMessage(`{"result":"private-value"}`)},
		{Kind: engine.PermissionRequest, ID: "permission", Name: "commandExecution", Text: "private-value", Input: json.RawMessage(`{"command":"echo private-value"}`)},
		{Kind: engine.UserInputRequested, ID: "question", Name: "requestUserInput", Question: question},
	} {
		if err := run.event(event); err != nil {
			t.Fatal(err)
		}
	}
	data, _ := json.Marshal(observed)
	if strings.Contains(string(data), "private-value") || !strings.Contains(string(data), "[REDACTED]") {
		t.Fatal("private native payload leaked or lost useful structure")
	}
	if question.Questions[0].Question != "About private-value" {
		t.Fatal("redaction mutated the native pending question")
	}
	projected := observed[len(observed)-1].Question.Questions[0]
	if !projected.IsSecret || !projected.IsOther || projected.Header != "[REDACTED]" || projected.Question != "About [REDACTED]" || projected.Options[0].Description != "[REDACTED]" {
		t.Fatal("private question lost its typed controls or safe text projection")
	}
	question.Questions[0].Options[0].Label = "private-value"
	if err := run.event(engine.Event{Kind: engine.UserInputRequested, ID: "unsafe-choice", Name: "requestUserInput", Question: question}); err == nil {
		t.Fatal("private choice label was silently rewritten into a different native answer")
	}
}

func TestCodexSecretQuestionIdentityFailurePhysicallyClosesNativeRequest(t *testing.T) {
	a, _ := codexAppServerFixture(t, "secret-identity")
	var owner *codexRun
	var events []engine.Event
	questions := 0
	err := a.Execute(t.Context(), engine.Request{OwnerID: "owner", Workspace: t.TempDir(), Prompt: "fixture"}, func(event engine.Event) error {
		events = append(events, event)
		switch event.Kind {
		case engine.PermissionRequest:
			return a.RespondPermission(t.Context(), event.SessionID, event.ID, engine.PermissionDecision{Allow: true})
		case engine.UserInputRequested:
			questions++
			owner = a.codexRun(event.SessionID)
			return a.RespondUserInput(t.Context(), event.SessionID, event.ID, engine.UserInputResponse{Answers: map[string][]string{"color": {"human-secret-answer"}}})
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	if questions != 1 || len(events) == 0 || events[len(events)-1].Outcome != engine.Failed {
		t.Fatal("unsafe question identity was exposed or left execution blocked", questions)
	}
	if owner == nil || !owner.rpc.proc.CleanupComplete() {
		t.Fatal("private question failure did not join original native process")
	}
	data, _ := json.Marshal(events)
	if strings.Contains(string(data), "human-secret-answer") {
		t.Fatal("unsafe private question identity leaked to product sink")
	}
}

func TestCodexOversizedFinalMessageKeepsMarkedPrefixAndJournalReference(t *testing.T) {
	for _, size := range []int{(60 << 10) + 512, 70 << 10, 200 << 10} {
		// A multi-byte rune straddles the 60 KiB cut.
		text := strings.Repeat("a", (60<<10)-1) + "答" + strings.Repeat("b", size-(60<<10)-2)
		var events []engine.Event
		run := codexRun{session: "native", journalURI: "journal://codex/native", rpc: &codexRPC{journal: &journal{}}, emit: func(event engine.Event) error { events = append(events, event); return nil }}
		if err := run.event(engine.Event{Kind: engine.Message, ID: "answer", Text: text}); err != nil {
			t.Fatal(err)
		}
		if len(events) != 1 {
			t.Fatal("final message was not published once", len(events))
		}
		message := events[0]
		if !message.Truncated || message.Text == "" || len(message.Text) > 60<<10 || !utf8.ValidString(message.Text) || !strings.HasPrefix(text, message.Text) {
			t.Fatal("oversized Codex answer lost its bounded readable prefix", size, message.Truncated, len(message.Text))
		}
		if message.Reference == nil || message.Reference.SessionID != "native" || message.Reference.RecordID != "answer" || message.Reference.URI != "journal://codex/native" {
			t.Fatal("truncated Codex answer lost its complete journal reference", message.Reference)
		}
		contract := engine.StreamContract{}
		if err := contract.Accept(engine.Event{Kind: engine.SessionStarted, SessionID: "native"}); err != nil {
			t.Fatal(err)
		}
		if err := contract.Accept(message); err != nil {
			t.Fatal("bounded Codex answer still fails the stream contract", err)
		}
	}
	var events []engine.Event
	run := codexRun{session: "native", rpc: &codexRPC{journal: &journal{}}, emit: func(event engine.Event) error { events = append(events, event); return nil }}
	if err := run.event(engine.Event{Kind: engine.Message, ID: "answer", Text: "short answer"}); err != nil || len(events) != 1 || events[0].Truncated || events[0].Reference != nil || events[0].Text != "short answer" {
		t.Fatal("bounded Codex answer was rewritten", err, events)
	}
}
