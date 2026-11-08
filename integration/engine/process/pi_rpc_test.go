// INPUT: Project-owned Pi RPC subprocess, fixed bridge source and private temporary native files
// OUTPUT: Pre-execution allow/deny, free-text question, exact resume and cancellation regressions
// POS: [integration / native external-engine adapter] Deterministic protocol tests never start user CLIs or account-backed models
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

	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage/bindings"
)

func TestPiRPCFixtureProcess(t *testing.T) {
	mode := os.Getenv("ALVA_PI_RPC_FIXTURE")
	if mode == "" {
		return
	}
	recordFixtureArgv()
	arg := func(flag string) string {
		for i, value := range os.Args {
			if value == flag && i+1 < len(os.Args) {
				return os.Args[i+1]
			}
		}
		return ""
	}
	if arg("--mode") != "rpc" || arg("--tools") != "read,bash,edit,write,grep,find,ls,ask_user" {
		os.Exit(81)
	}
	source, err := os.ReadFile(arg("--extension"))
	if err != nil || string(source) != piBridgeSource {
		os.Exit(82)
	}
	path := arg("--session")
	info, err := os.Stat(path)
	if err != nil {
		os.Exit(83)
	}
	if info.Size() == 0 {
		cwd, _ := os.Getwd()
		header, _ := json.Marshal(map[string]any{"type": "session", "version": 3, "id": "pi-rpc-native", "cwd": cwd})
		if err := os.WriteFile(path, append(header, '\n'), 0600); err != nil {
			os.Exit(84)
		}
	}
	output := func(value any) { data, _ := json.Marshal(value); fmt.Println(string(data)) }
	nonce := os.Getenv("ALVA_PI_BRIDGE_NONCE")
	if mode != "no-ready" {
		output(map[string]any{"type": "extension_ui_request", "id": "ready", "method": "notify", "message": mustJSON(map[string]any{"alvaBridge": 1, "nonce": nonce, "kind": "ready"})})
	}
	toolEnd := func(id, name string, isError bool) {
		output(map[string]any{"type": "tool_execution_end", "toolCallId": id, "toolName": name, "isError": isError, "result": map[string]any{"content": []any{map[string]string{"type": "text", "text": "fixture result"}}}})
	}
	finish := func(text string) {
		output(map[string]any{"type": "message_start", "message": map[string]string{"role": "assistant"}})
		output(map[string]any{"type": "message_end", "message": map[string]any{"role": "assistant", "stopReason": "stop", "content": []any{map[string]string{"type": "text", "text": text}}}})
		output(map[string]any{"type": "agent_end", "willRetry": false})
		output(map[string]any{"type": "agent_settled", "aborted": false})
		if mode == "after-settled" {
			output(map[string]any{"type": "tool_execution_start", "toolCallId": "too-late", "toolName": "write", "args": map[string]string{"path": "late"}})
		}
	}
	finishStreamed := func(text string) {
		output(map[string]any{"type": "message_start", "message": map[string]string{"role": "assistant"}})
		for offset := 0; offset < len(text); offset += 8 << 10 {
			end := min(offset+(8<<10), len(text))
			output(map[string]any{"type": "message_update", "assistantMessageEvent": map[string]string{"type": "text_delta", "delta": text[offset:end]}})
		}
		output(map[string]any{"type": "message_end", "message": map[string]any{"role": "assistant", "stopReason": "stop", "content": []any{map[string]string{"type": "text", "text": text}}}})
		output(map[string]any{"type": "agent_end", "willRetry": false})
		output(map[string]any{"type": "agent_settled", "aborted": false})
	}
	scanner := bufio.NewScanner(os.Stdin)
	scanner.Buffer(make([]byte, 4096), 64<<20)
	for scanner.Scan() {
		frame := decode(scanner.Bytes())
		kind := str(frame, "type")
		reply := func(data any) {
			output(map[string]any{"type": "response", "id": str(frame, "id"), "command": kind, "success": true, "data": data})
		}
		switch kind {
		case "get_state":
			native, file := "pi-rpc-native", path
			if mode == "foreign-id" {
				native = "foreign-native"
			}
			if mode == "foreign-file" {
				file = filepath.Join(filepath.Dir(path), "foreign.jsonl")
			}
			reply(map[string]any{"sessionId": native, "sessionFile": file, "isStreaming": false, "isCompacting": false})
			if mode == "no-ready" {
				output(map[string]string{"type": "extension_error", "error": "owned extension not loaded"})
			}
		case "prompt":
			_ = os.WriteFile(os.Getenv("ALVA_PI_PROMPT_MARKER"), []byte("prompt accepted"), 0600)
			reply(map[string]string{"disposition": "started"})
			output(map[string]string{"type": "agent_start"})
			if mode == "long-public" {
				finishStreamed(piLongReply("", 80<<10))
				continue
			}
			if mode == "images" {
				var images []map[string]any
				if json.Unmarshal(frame["images"], &images) != nil || len(images) != 1 || len(images[0]["data"].(string)) <= 16<<20 {
					os.Exit(89)
				}
				output(map[string]any{"type": "message_start", "message": map[string]any{"role": "user", "content": images}})
				output(map[string]any{"type": "message_end", "message": map[string]any{"role": "user", "content": images}})
				finish("image native record retained")
				continue
			}
			if mode == "partial-large" {
				fmt.Print(`{"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"` + strings.Repeat("x", 2<<20))
				_ = os.WriteFile(os.Getenv("ALVA_PI_PROMPT_MARKER")+".partial", nil, 0600)
				continue
			}
			if mode == "native-error" {
				output(map[string]any{"type": "message_end", "message": map[string]string{"role": "assistant", "stopReason": "error", "errorMessage": "fixture native failure"}})
				output(map[string]any{"type": "agent_settled", "aborted": false})
				continue
			}
			output(map[string]any{"type": "tool_execution_start", "toolCallId": "write-call", "toolName": "write", "args": map[string]string{"path": os.Getenv("ALVA_PI_WRITE_TARGET"), "content": "approved fixture edit"}})
			permissionNonce := nonce
			if mode == "bad-nonce" {
				permissionNonce = "foreign-nonce"
			}
			output(map[string]any{"type": "extension_ui_request", "id": "permission-1", "method": "confirm", "title": "Alva native tool approval", "message": mustJSON(map[string]any{"alvaBridge": 1, "nonce": permissionNonce, "kind": "permission", "toolCallId": "write-call", "toolName": "write", "input": map[string]string{"path": os.Getenv("ALVA_PI_WRITE_TARGET"), "content": "approved fixture edit"}})})
		case "extension_ui_response":
			if str(frame, "id") == "permission-1" {
				allowed := boolean(frame, "confirmed")
				if allowed {
					if err := os.WriteFile(os.Getenv("ALVA_PI_WRITE_TARGET"), []byte("approved fixture edit"), 0600); err != nil {
						os.Exit(85)
					}
				}
				toolEnd("write-call", "write", !allowed)
				if strings.HasPrefix(mode, "question") && allowed {
					output(map[string]any{"type": "tool_execution_start", "toolCallId": "question-call", "toolName": "ask_user", "args": map[string]string{"question": "Which fixture value?"}})
					output(map[string]any{"type": "extension_ui_request", "id": "question-1", "method": "input", "title": mustJSON(map[string]any{"alvaBridge": 1, "nonce": nonce, "kind": "question", "toolCallId": "question-call", "question": "Which fixture value?"})})
				} else {
					finish("fixture complete")
				}
			} else if str(frame, "id") == "question-1" {
				answer := str(frame, "value")
				toolEnd("question-call", "ask_user", false)
				if mode == "question-20k" || mode == "question-80k" {
					size := 20 << 10
					if mode == "question-80k" {
						size = 80 << 10
					}
					finishStreamed(piLongReply(answer, size))
					continue
				}
				if answer != "" {
					output(map[string]any{"type": "message_start", "message": map[string]string{"role": "assistant"}})
					for _, text := range []string{"fixture private-", "answer-value end"} {
						output(map[string]any{"type": "message_update", "assistantMessageEvent": map[string]string{"type": "text_delta", "delta": text}})
					}
				}
				finish("fixture " + answer + " end")
			} else {
				os.Exit(86)
			}
		case "abort":
			_ = os.WriteFile(os.Getenv("ALVA_PI_ABORT_MARKER"), []byte("abort accepted"), 0600)
			reply(nil)
		default:
			os.Exit(87)
		}
	}
	if scanner.Err() != nil {
		os.Exit(88)
	}
	os.Exit(0)
}

func piLongReply(secret string, size int) string {
	data := []byte(strings.Repeat("x", size))
	if secret != "" {
		for _, boundary := range []int{16 << 10, 64 << 10} {
			if boundary+len(secret) < len(data) {
				copy(data[boundary-6:], secret)
			}
		}
	}
	copy(data[len(data)-len("FINAL-TAIL"):], "FINAL-TAIL")
	return string(data)
}

func piRPCFixture(t *testing.T, mode string) (*Adapter, string, map[string]string) {
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
	env := map[string]string{"ALVA_PI_RPC_FIXTURE": mode, "ALVA_PI_PROMPT_MARKER": filepath.Join(t.TempDir(), "prompt"), "ALVA_PI_ABORT_MARKER": filepath.Join(t.TempDir(), "abort"), "ALVA_PI_WRITE_TARGET": filepath.Join(t.TempDir(), "selected.txt")}
	a, err := New(Config{Bindings: bindingPorts{store}, Vendor: "pi", Command: binary, PrefixArgs: []string{"-test.run=^TestPiRPCFixtureProcess$", "--"}, Transport: "rpc", StateDirectory: directory, ApprovedHostAccess: true, Env: env})
	if err != nil {
		store.Close()
		t.Fatal(err)
	}
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		_ = a.Close(ctx)
	})
	return a, directory, env
}

func TestPiRPCApprovalPrecedesActualWriteAndDenialHasNoEffect(t *testing.T) {
	for _, allow := range []bool{false, true} {
		t.Run(fmt.Sprint(allow), func(t *testing.T) {
			a, _, env := piRPCFixture(t, "approval")
			var terminal engine.Outcome
			var permission, resolved, starts, ends int
			err := a.Execute(t.Context(), engine.Request{OwnerID: "owner", Workspace: t.TempDir(), Prompt: "edit the selected fixture"}, func(event engine.Event) error {
				switch event.Kind {
				case engine.PermissionRequest:
					permission++
					if _, err := os.Stat(env["ALVA_PI_WRITE_TARGET"]); !os.IsNotExist(err) {
						t.Fatal("Pi wrote before actual approval", err)
					}
					if event.Name != "write" || event.Effect != engine.EffectWrite || event.Reference == nil || !strings.HasPrefix(event.ID, "pi:engine-") {
						t.Fatal("approval lost native identity or tool details", event)
					}
					if event.Display == nil || event.Display.Kind != engine.ToolEdit || event.Display.Path == "" {
						t.Fatal("approval lost the display of the call it asks to run", event.Display)
					}
					return a.RespondPermission(t.Context(), event.SessionID, event.ID, engine.PermissionDecision{Allow: allow})
				case engine.InteractionResolved:
					resolved++
					if event.Resolution == nil || event.Resolution.Allowed == nil || *event.Resolution.Allowed != allow {
						t.Fatal("resolved approval did not report the relayed decision", event.Resolution)
					}
				case engine.ToolStart:
					starts++
				case engine.ToolEnd:
					ends++
					if event.IsError == allow {
						t.Fatal("denial did not become the exact failed native tool", event)
					}
				case engine.Completed:
					terminal = event.Outcome
				}
				return nil
			})
			if err != nil || terminal != engine.Succeeded || permission != 1 || resolved != 1 || starts != 1 || ends != 1 {
				t.Fatal("native approval/write lifecycle failed", terminal, permission, resolved, starts, ends, err)
			}
			data, err := os.ReadFile(env["ALVA_PI_WRITE_TARGET"])
			if allow {
				if err != nil || string(data) != "approved fixture edit" {
					t.Fatal("approved write never executed", err)
				}
			} else if !os.IsNotExist(err) {
				t.Fatal("declined native write had a side effect", err)
			}
			if !a.Capabilities().PermissionCallback || !a.Capabilities().UserInputCallback {
				t.Fatal("RPC interaction capability missing")
			}
		})
	}
}
func TestPiRPCQuestionIsSeparateAndAnswerEchoesAreRedacted(t *testing.T) {
	a, directory, _ := piRPCFixture(t, "question")
	var questions int
	var events []engine.Event
	err := a.Execute(t.Context(), engine.Request{OwnerID: "owner", Workspace: t.TempDir(), Prompt: "edit and ask the fixture"}, func(event engine.Event) error {
		events = append(events, event)
		switch event.Kind {
		case engine.PermissionRequest:
			return a.RespondPermission(t.Context(), event.SessionID, event.ID, engine.PermissionDecision{Allow: true})
		case engine.UserInputRequested:
			questions++
			if len(event.Question.Questions) != 1 || event.Question.Questions[0].ID != "answer" || len(event.Question.Questions[0].Options) != 0 {
				t.Fatal("Pi invented unsupported question choices", event)
			}
			return a.RespondUserInput(t.Context(), event.SessionID, event.ID, engine.UserInputResponse{Answers: map[string][]string{"answer": {"private-answer-value"}}})
		}
		return nil
	})
	if err != nil || questions != 1 || events[len(events)-1].Outcome != engine.Succeeded {
		t.Fatal("Pi question lifecycle failed", questions, events, err)
	}
	resolved := 0
	for _, event := range events {
		if strings.Contains(event.Text, "private-") || strings.Contains(event.Text, "answer-value") {
			t.Fatal("Pi answer echo leaked into product facts", event)
		}
		if event.Kind == engine.InteractionResolved && event.Name == "user_input" {
			// A free-text answer is recorded as answered, never with its body.
			resolved++
			if event.Resolution == nil || event.Resolution.Answers["answer"] == nil || len(event.Resolution.Answers["answer"]) != 0 {
				t.Fatal("relayed Pi answer kept a body", event.Resolution)
			}
		}
	}
	if serialized, _ := json.Marshal(events); resolved != 1 || strings.Contains(string(serialized), "private-answer-value") {
		t.Fatal("Pi answer leaked into a resolved fact", resolved)
	}
	entries, err := os.ReadDir(filepath.Join(directory, "journals"))
	if err != nil || len(entries) != 1 {
		t.Fatal(entries, err)
	}
	data, err := os.ReadFile(filepath.Join(directory, "journals", entries[0].Name()))
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(data), "private-") || strings.Contains(string(data), "answer-value") {
		t.Fatal("Pi answer fragments were retained in the native journal")
	}
}

func TestPiRPCCompletedPrivateMessagesRemainWholeAndReadable(t *testing.T) {
	for _, size := range []int{20 << 10, 80 << 10} {
		t.Run(fmt.Sprint(size), func(t *testing.T) {
			mode := "question-20k"
			if size > 64<<10 {
				mode = "question-80k"
			}
			a, directory, _ := piRPCFixture(t, mode)
			secret := "private-answer-value"
			expected := strings.ReplaceAll(piLongReply(secret, size), secret, "[REDACTED]")
			var text strings.Builder
			var messages, deltas, questions int
			var outcome engine.Outcome
			var owner *piRun
			err := a.Execute(t.Context(), engine.Request{OwnerID: "owner", Workspace: t.TempDir(), Prompt: "ask then return a long complete answer"}, func(event engine.Event) error {
				switch event.Kind {
				case engine.PermissionRequest:
					return a.RespondPermission(t.Context(), event.SessionID, event.ID, engine.PermissionDecision{Allow: true})
				case engine.UserInputRequested:
					questions++
					owner = a.piRun(event.SessionID)
					return a.RespondUserInput(t.Context(), event.SessionID, event.ID, engine.UserInputResponse{Answers: map[string][]string{"answer": {secret}}})
				case engine.Message:
					messages++
					text.Reset()
					text.WriteString(event.Text)
				case engine.MessageDelta:
					deltas++
					text.WriteString(event.Text)
				case engine.Completed:
					outcome = event.Outcome
				}
				if owner != nil && owner.mapper.piWholeMessage {
					t.Fatal("Pi internal complete-aggregate flag reached the external sink")
				}
				if strings.Contains(event.Text, "private-") || strings.Contains(event.Text, "answer-value") {
					t.Fatal("completed secret crossed a public chunk boundary", event.Kind, event.ID)
				}
				return nil
			})
			if err != nil || outcome != engine.Succeeded || questions != 1 || text.String() != expected {
				t.Fatal("long completed answer was lost, repeated or partially redacted", size, text.Len(), len(expected), outcome, questions, err)
			}
			if size <= 64<<10 && (messages != 1 || deltas != 0) {
				t.Fatal("complete bounded answer was treated as raw streaming", messages, deltas)
			}
			if size > 64<<10 && (messages != 0 || deltas < 2) {
				t.Fatal("large answer chunks were falsely marked as complete messages", messages, deltas)
			}
			if owner == nil || len(owner.mapper.texts) != 0 || owner.mapper.current != "" || owner.publishedID != "" || owner.publishedHash != nil || len(owner.streamTails) != 0 {
				t.Fatal("Pi completed private message retained its cumulative or published-prefix state")
			}
			entries, err := os.ReadDir(filepath.Join(directory, "journals"))
			if err != nil || len(entries) != 1 {
				t.Fatal(entries, err)
			}
			body, err := os.ReadFile(filepath.Join(directory, "journals", entries[0].Name()))
			if err != nil {
				t.Fatal(err)
			}
			if strings.Contains(string(body), "private-") || strings.Contains(string(body), "answer-value") {
				t.Fatal("raw long-answer secret fragments entered the private journal")
			}
		})
	}
}

func TestPiRPCLargePublicCompletionDoesNotRepeatPublishedDeltas(t *testing.T) {
	a, _, _ := piRPCFixture(t, "long-public")
	var text strings.Builder
	var outcome engine.Outcome
	err := a.Execute(t.Context(), engine.Request{OwnerID: "owner", Workspace: t.TempDir(), Prompt: "stream a long answer then its authoritative full message"}, func(event engine.Event) error {
		if event.Kind == engine.Message {
			t.Fatal("large public message was split into falsely complete messages")
		}
		if event.Kind == engine.MessageDelta {
			text.WriteString(event.Text)
		}
		if event.Kind == engine.Completed {
			outcome = event.Outcome
		}
		return nil
	})
	if err != nil || outcome != engine.Succeeded || text.String() != piLongReply("", 80<<10) {
		t.Fatal("completed public aggregate repeated or dropped its streamed prefix", text.Len(), outcome, err)
	}
}
func TestPiRPCMissingBridgeAndForeignStateCannotSubmitPrompt(t *testing.T) {
	for _, mode := range []string{"no-ready", "foreign-id", "foreign-file"} {
		t.Run(mode, func(t *testing.T) {
			a, _, env := piRPCFixture(t, mode)
			var terminal engine.Outcome
			err := a.Execute(t.Context(), engine.Request{OwnerID: "owner", Workspace: t.TempDir(), Prompt: "must not reach native model"}, func(event engine.Event) error {
				if event.Kind == engine.Completed {
					terminal = event.Outcome
				}
				if event.Kind == engine.PermissionRequest {
					t.Fatal("unverified bridge requested execution")
				}
				return nil
			})
			if err != nil || terminal != engine.Failed {
				t.Fatal("unverified native bridge/state claimed success", terminal, err)
			}
			if _, err := os.Stat(env["ALVA_PI_PROMPT_MARKER"]); !os.IsNotExist(err) {
				t.Fatal("unverified bridge or state received a prompt", err)
			}
		})
	}
}
func TestPiRPCNativeProtocolFailureCannotBecomeSuccess(t *testing.T) {
	for _, mode := range []string{"bad-nonce", "native-error", "after-settled"} {
		t.Run(mode, func(t *testing.T) {
			a, _, _ := piRPCFixture(t, mode)
			var terminal engine.Outcome
			err := a.Execute(t.Context(), engine.Request{OwnerID: "owner", Workspace: t.TempDir(), Prompt: "fixture"}, func(event engine.Event) error {
				if event.Kind == engine.PermissionRequest {
					return a.RespondPermission(t.Context(), event.SessionID, event.ID, engine.PermissionDecision{Allow: true})
				}
				if event.Kind == engine.Completed {
					terminal = event.Outcome
				}
				return nil
			})
			if err != nil || terminal != engine.Failed {
				t.Fatal("native protocol/result failure became success", terminal, err)
			}
		})
	}
}
func TestPiRPCStopJoinsAndOldReplyCannotAuthorizeNextRun(t *testing.T) {
	a, _, env := piRPCFixture(t, "approval")
	request := engine.Request{OwnerID: "owner", Workspace: t.TempDir(), Prompt: "wait for permission"}
	permission := make(chan engine.Event, 1)
	finished := make(chan error, 1)
	var terminal engine.Outcome
	go func() {
		finished <- a.Execute(t.Context(), request, func(event engine.Event) error {
			if event.Kind == engine.PermissionRequest {
				permission <- event
			}
			if event.Kind == engine.Completed {
				terminal = event.Outcome
			}
			return nil
		})
	}()
	var first engine.Event
	select {
	case first = <-permission:
	case <-time.After(5 * time.Second):
		t.Fatal("Pi did not request native permission")
	}
	owner := a.piRun(first.SessionID)
	if err := a.Cancel(t.Context(), first.SessionID); err != nil {
		t.Fatal(err)
	}
	select {
	case err := <-finished:
		if err != nil || terminal != engine.Cancelled {
			t.Fatal("Stop did not persist cancellation", terminal, err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("Stop did not join exact native owner")
	}
	if !owner.connection.proc.CleanupComplete() {
		t.Fatal("Stop returned before physical process cleanup")
	}
	if _, err := os.Stat(env["ALVA_PI_WRITE_TARGET"]); !os.IsNotExist(err) {
		t.Fatal("Stop authorized a pending write", err)
	}
	if _, err := os.Stat(env["ALVA_PI_ABORT_MARKER"]); err != nil {
		t.Fatal("Stop skipped native abort", err)
	}
	request.ResumeSession = first.SessionID
	var resumed engine.Outcome
	err := a.Execute(t.Context(), request, func(event engine.Event) error {
		if event.Kind == engine.PermissionRequest {
			if event.ID == first.ID {
				t.Fatal("native request identity was reused across product attempts")
			}
			if err := a.RespondPermission(t.Context(), first.SessionID, first.ID, engine.PermissionDecision{Allow: true}); !errors.Is(err, engine.ErrSessionNotFound) {
				t.Fatal("late old reply authorized replacement run", err)
			}
			return a.RespondPermission(t.Context(), event.SessionID, event.ID, engine.PermissionDecision{Allow: true})
		}
		if event.Kind == engine.Completed {
			resumed = event.Outcome
		}
		return nil
	})
	if err != nil || resumed != engine.Succeeded {
		t.Fatal("same native session failed after Stop", resumed, err)
	}
	config := a.config
	if err := a.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	store, err := bindings.Open(t.Context(), config.StateDirectory)
	if err != nil {
		t.Fatal(err)
	}
	config.Bindings = bindingPorts{store}
	reopened, err := New(config)
	if err != nil {
		store.Close()
		t.Fatal(err)
	}
	defer reopened.Close(context.Background())
	var coldOutcome engine.Outcome
	err = reopened.Execute(t.Context(), request, func(event engine.Event) error {
		if event.Kind == engine.SessionStarted && event.SessionID != first.SessionID {
			t.Fatal("cold RPC reopen changed the retained native identity", event)
		}
		if event.Kind == engine.PermissionRequest {
			return reopened.RespondPermission(t.Context(), event.SessionID, event.ID, engine.PermissionDecision{Allow: true})
		}
		if event.Kind == engine.Completed {
			coldOutcome = event.Outcome
		}
		return nil
	})
	if err != nil || coldOutcome != engine.Succeeded {
		t.Fatal("RPC exact native resume failed after adapter reopen", coldOutcome, err)
	}
}
func TestPiOwnedBridgeRejectsChangedSourceBeforeProcess(t *testing.T) {
	a, _, env := piRPCFixture(t, "approval")
	path, err := a.piBridge()
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(strings.Repeat("x", len(piBridgeSource))), 0600); err != nil {
		t.Fatal(err)
	}
	if err := a.Execute(t.Context(), engine.Request{OwnerID: "owner", Workspace: t.TempDir(), Prompt: "must not start"}, func(engine.Event) error { t.Fatal("changed bridge started a native process"); return nil }); err == nil {
		t.Fatal("changed bridge source was accepted")
	}
	if _, err := os.Stat(env["ALVA_PI_PROMPT_MARKER"]); !os.IsNotExist(err) {
		t.Fatal("changed bridge received native work", err)
	}
}

func TestPiRPCLargeNativeImageRecordsDoNotDuplicateBase64InJournal(t *testing.T) {
	a, directory, _ := piRPCFixture(t, "images")
	path := filepath.Join(t.TempDir(), "selected.png")
	data := []byte(strings.Repeat("x", 13<<20))
	if err := os.WriteFile(path, data, 0600); err != nil {
		t.Fatal(err)
	}
	sum := sha256.Sum256(data)
	files := []engine.InputFile{{ID: "image", Name: "selected.png", URI: "artifact://selected", Path: path, MediaType: "image/png", SHA256: hex.EncodeToString(sum[:]), Bytes: int64(len(data))}}
	var outcome engine.Outcome
	err := a.Execute(t.Context(), engine.Request{OwnerID: "owner", Workspace: t.TempDir(), Prompt: "receive captured image", InputFiles: files}, func(event engine.Event) error {
		if event.Kind == engine.Completed {
			outcome = event.Outcome
		}
		return nil
	})
	if err != nil || outcome != engine.Succeeded {
		t.Fatal("native image records larger than 16 MiB failed", outcome, err)
	}
	entries, err := os.ReadDir(filepath.Join(directory, "journals"))
	if err != nil || len(entries) != 1 {
		t.Fatal(entries, err)
	}
	journal, err := os.ReadFile(filepath.Join(directory, "journals", entries[0].Name()))
	if err != nil {
		t.Fatal(err)
	}
	if len(journal) > 1<<20 || strings.Contains(string(journal), strings.Repeat("eHh4", 128)) || !strings.Contains(string(journal), "omitted_encoded_bytes") {
		t.Fatal("large base64 bodies were duplicated into the native journal", len(journal))
	}
	info, err := os.Stat(path)
	if err != nil || info.Size() != int64(len(data)) {
		t.Fatal("selected original image was changed", info, err)
	}
}

func TestPiRPCStopJoinsWhileReadingAnIncompleteLargeRecord(t *testing.T) {
	a, _, env := piRPCFixture(t, "partial-large")
	done := make(chan error, 1)
	started := make(chan string, 1)
	var outcome engine.Outcome
	go func() {
		done <- a.Execute(t.Context(), engine.Request{OwnerID: "owner", Workspace: t.TempDir(), Prompt: "hold a large unfinished native record"}, func(event engine.Event) error {
			if event.Kind == engine.SessionStarted {
				started <- event.SessionID
			}
			if event.Kind == engine.Completed {
				outcome = event.Outcome
			}
			return nil
		})
	}()
	var session string
	select {
	case session = <-started:
	case <-time.After(5 * time.Second):
		t.Fatal("native session did not start")
	}
	deadline := time.Now().Add(5 * time.Second)
	for {
		if _, err := os.Stat(env["ALVA_PI_PROMPT_MARKER"] + ".partial"); err == nil {
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("native incomplete record did not reach its barrier")
		}
		time.Sleep(time.Millisecond)
	}
	owner := a.piRun(session)
	if err := a.Cancel(t.Context(), session); err != nil {
		t.Fatal(err)
	}
	select {
	case err := <-done:
		if err != nil || outcome != engine.Cancelled {
			t.Fatal("large-record Stop did not remain cancelled", outcome, err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("large native reader did not join")
	}
	if !owner.connection.proc.CleanupComplete() {
		t.Fatal("large native frame reader released a live process")
	}
}
