// INPUT: An owned helper child speaking Claude Code's stream-json control protocol (control_request can_use_tool, control_response) and typed engine events
// OUTPUT: Approve, deny, late, duplicate, cancel-with-pending, cancel-request and unsupported-request regressions plus argv/capability proof for the Claude permission channel
// POS: [integration / native external-engine adapter tests] Frame shapes were captured from claude 2.1.319 against a local fake model server; no user CLI, account or real model is used
package process

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage/bindings"
)

const claudeStreamSecret = "stream-secret-98765"

// TestClaudeStreamFixtureProcess is the helper child. It echoes the frame
// sequence a real claude emits: init, assistant tool_use, control_request,
// then (after the host's control_response) tool_result and result.
func TestClaudeStreamFixtureProcess(t *testing.T) {
	mode := os.Getenv("ALVA_CLAUDE_STREAM")
	if mode == "" {
		return
	}
	recordFixtureArgv()
	in := bufio.NewReader(os.Stdin)
	line, _ := in.ReadString('\n')
	if path := os.Getenv("ALVA_FIXTURE_STDIN_FILE"); path != "" {
		_ = os.WriteFile(path, []byte(line), 0600)
	}
	out := func(frame string) { fmt.Println(frame) }
	if mode == "old-claude" {
		fmt.Fprintln(os.Stderr, "error: unknown option '--permission-prompt-tool'")
		os.Exit(1)
	}
	if mode == "auth-failure" {
		fmt.Fprintln(os.Stderr, "Invalid API key - please run /login")
		os.Exit(1)
	}
	out(`{"type":"system","subtype":"init","session_id":"stream-native"}`)
	tool, input := "Write", `{"file_path":"/work/out.txt","content":"hi `+claudeStreamSecret+`"}`
	if mode == "big" {
		input = `{"file_path":"/work/big.txt","content":"` + strings.Repeat("x", claudeMaxInputSize+10) + `"}`
	}
	if mode == "bash" {
		tool, input = "Bash", `{"command":"echo hi"}`
	}
	if mode == "question" {
		tool, input = "AskUserQuestion", `{"questions":[{"question":"which","options":[]}]}`
	}
	out(`{"type":"assistant","message":{"content":[{"type":"tool_use","id":"t1","name":"` + tool + `","input":` + input + `}]}}`)
	request := func(id string) {
		out(`{"type":"control_request","request_id":"` + id + `","request":{"subtype":"can_use_tool","tool_name":"` + tool + `","input":` + input + `,"tool_use_id":"t1","permission_suggestions":[{"type":"setMode","mode":"acceptEdits","destination":"session"}]}}`)
	}
	request("req1")
	if mode == "duplicate" {
		request("req1")
	}
	if mode == "unsupported" {
		out(`{"type":"control_request","request_id":"hook1","request":{"subtype":"hook_callback"}}`)
	}
	if mode == "cancel-request" {
		out(`{"type":"control_cancel_request","request_id":"req1"}`)
		out(`{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"gone","is_error":true}]}}`)
		out(`{"type":"result","subtype":"success","session_id":"stream-native"}`)
		_, _ = in.ReadString('\n')
		os.Exit(0)
	}
	if mode == "hang" {
		for {
			time.Sleep(time.Second)
		}
	}
	for {
		reply, err := in.ReadString('\n')
		if err != nil {
			os.Exit(3)
		}
		if path := os.Getenv("ALVA_FIXTURE_REPLY_FILE"); path != "" {
			f, _ := os.OpenFile(path, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0600)
			_, _ = f.WriteString(reply)
			f.Close()
		}
		var frame struct {
			Response struct {
				RequestID string `json:"request_id"`
				Response  struct{ Behavior string }
			}
		}
		_ = json.Unmarshal([]byte(reply), &frame)
		if frame.Response.RequestID == "hook1" {
			continue
		}
		if frame.Response.Response.Behavior == "allow" {
			out(`{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"done"}]}}`)
		} else {
			out(`{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"denied","is_error":true}]}}`)
		}
		break
	}
	out(`{"type":"assistant","message":{"content":[{"type":"text","text":"finished"}]}}`)
	out(`{"type":"result","subtype":"success","session_id":"stream-native","usage":{"input_tokens":3,"output_tokens":2}}`)
	// A real claude exits when its input closes after the result.
	_, _ = in.ReadString('\n')
	os.Exit(0)
}

type streamHarness struct {
	adapter *Adapter
	dir     string
	argv    string
	stdin   string
	replies string
}

func newClaudeStreamAdapter(t *testing.T, mode string, transport string) *streamHarness {
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
	scratch := t.TempDir()
	h := &streamHarness{dir: dir, argv: filepath.Join(scratch, "argv"), stdin: filepath.Join(scratch, "stdin"), replies: filepath.Join(scratch, "replies")}
	a, err := New(Config{Bindings: bindingPorts{store}, Vendor: "claude", Command: binary, PrefixArgs: []string{"-test.run=^TestClaudeStreamFixtureProcess$", "--"}, Transport: transport, StateDirectory: dir, ApprovedHostAccess: true,
		ProductInstructions: "chart format",
		Env:                 map[string]string{"ALVA_CLAUDE_STREAM": mode, "FIXTURE_TOKEN": claudeStreamSecret, "ALVA_FIXTURE_ARGV_FILE": h.argv, "ALVA_FIXTURE_STDIN_FILE": h.stdin, "ALVA_FIXTURE_REPLY_FILE": h.replies}})
	if err != nil {
		store.Close()
		t.Fatal(err)
	}
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		_ = a.Close(ctx)
	})
	h.adapter = a
	return h
}

type streamRun struct {
	mu        sync.Mutex
	events    []engine.Event
	requests  chan engine.Event
	done      chan error
	cancel    context.CancelFunc
	completed engine.Event
}

func (h *streamHarness) start(t *testing.T, request engine.Request) *streamRun {
	t.Helper()
	ctx, cancel := context.WithCancel(t.Context())
	run := &streamRun{requests: make(chan engine.Event, 8), done: make(chan error, 1), cancel: cancel}
	if request.OwnerID == "" {
		request.OwnerID, request.Prompt, request.Workspace = "owner", "do it", t.TempDir()
	}
	go func() {
		var contract engine.StreamContract
		run.done <- h.adapter.Execute(ctx, request, func(event engine.Event) error {
			if err := contract.Accept(event); err != nil {
				return err
			}
			run.mu.Lock()
			run.events = append(run.events, event)
			if event.Kind == engine.Completed {
				run.completed = event
			}
			run.mu.Unlock()
			if event.Kind == engine.PermissionRequest {
				run.requests <- event
			}
			return nil
		})
	}()
	t.Cleanup(cancel)
	return run
}

func (r *streamRun) request(t *testing.T) engine.Event {
	t.Helper()
	select {
	case event := <-r.requests:
		return event
	case <-time.After(10 * time.Second):
		t.Fatal("no permission request reached the product")
	}
	return engine.Event{}
}

func (r *streamRun) finish(t *testing.T) []engine.Event {
	t.Helper()
	select {
	case err := <-r.done:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(10 * time.Second):
		t.Fatal("run did not finish")
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	return append([]engine.Event(nil), r.events...)
}

func kinds(events []engine.Event, kind engine.Kind) []engine.Event {
	var out []engine.Event
	for _, event := range events {
		if event.Kind == kind {
			out = append(out, event)
		}
	}
	return out
}

func TestClaudeStreamCapabilitiesAndArgv(t *testing.T) {
	h := newClaudeStreamAdapter(t, "ask", "stream-json")
	c := h.adapter.Capabilities()
	if !c.PermissionCallback || c.UserInputCallback || c.Interjection || c.Fork || c.FileInputs || c.SkillInputs || !c.Resume || !c.Cancel {
		t.Fatalf("stream-json declares more or less than it implements: %+v", c)
	}
	if exec := newClaudeStreamAdapter(t, "ask", "").adapter.Capabilities(); exec.PermissionCallback {
		t.Fatal("print transport must not claim permission callbacks")
	}
	run := h.start(t, engine.Request{OwnerID: "owner", Prompt: "do it", Workspace: t.TempDir(), SystemPrompt: "sys", Model: "opus", MaxTurns: 3})
	_ = run.request(t)
	argv := recordedArgv(t, h.argv)
	for flag, want := range map[string]string{"--permission-mode": "manual", "--permission-prompt-tool": "stdio", "--input-format": "stream-json", "--output-format": "stream-json", "--model": "opus", "--max-turns": "3"} {
		if got, n := flagValue(argv, flag); got != want || n != 1 {
			t.Fatalf("%s = %q (%d times), want %q", flag, got, n, want)
		}
	}
	if slices.Contains(argv, "dontAsk") || !slices.Contains(argv, "--safe-mode") {
		t.Fatal("dontAsk must be gone and --safe-mode kept", argv)
	}
	if got, n := flagValue(argv, "--append-system-prompt"); n != 1 || got != "sys\n\nchart format" {
		t.Fatalf("system prompt and product instructions must merge into one append flag, got %q x%d", got, n)
	}
	run.cancel()
	<-run.done
}

func TestClaudeStreamApproveIsOneShotAndPromptReachesChildAsUserFrame(t *testing.T) {
	h := newClaudeStreamAdapter(t, "ask", "stream-json")
	run := h.start(t, engine.Request{OwnerID: "owner", Prompt: "do it", Workspace: t.TempDir()})
	request := run.request(t)
	if request.Name != "Write" || request.Effect != engine.EffectWrite || request.Display == nil || request.Display.Kind != engine.ToolEdit {
		t.Fatalf("request lacks typed effect/display: %+v", request)
	}
	if strings.Contains(string(request.Input), claudeStreamSecret) || request.Display != nil && strings.Contains(fmt.Sprint(*request.Display), claudeStreamSecret) {
		t.Fatal("request leaked a configured secret")
	}
	if err := h.adapter.RespondPermission(t.Context(), "stream-native", request.ID, engine.PermissionDecision{Allow: true}); err != nil {
		t.Fatal(err)
	}
	if err := h.adapter.RespondPermission(t.Context(), "stream-native", request.ID, engine.PermissionDecision{Allow: true}); !errors.Is(err, engine.ErrSessionNotFound) {
		t.Fatalf("a second decision for the same request must be refused, got %v", err)
	}
	events := run.finish(t)
	resolved := kinds(events, engine.InteractionResolved)
	if len(resolved) != 1 || resolved[0].ID != request.ID || resolved[0].Resolution == nil || resolved[0].Resolution.Allowed == nil || !*resolved[0].Resolution.Allowed {
		t.Fatalf("resolution does not record the allow: %+v", resolved)
	}
	if run.completed.Outcome != engine.Succeeded {
		t.Fatal("approved run did not succeed", run.completed)
	}
	data, _ := os.ReadFile(h.replies)
	var reply struct {
		Response struct {
			Subtype   string
			RequestID string `json:"request_id"`
			Response  map[string]json.RawMessage
		}
	}
	if err := json.Unmarshal([]byte(strings.TrimSpace(string(data))), &reply); err != nil || reply.Response.RequestID != "req1" || reply.Response.Subtype != "success" {
		t.Fatalf("child received %q", data)
	}
	if _, grants := reply.Response.Response["updatedPermissions"]; grants || strings.Count(string(data), "\n") != 1 {
		t.Fatal("an approval must be one-shot: no persisted permission update, exactly one reply", string(data))
	}
	stdin, _ := os.ReadFile(h.stdin)
	if !strings.Contains(string(stdin), `"type":"user"`) || !strings.Contains(string(stdin), "do it") {
		t.Fatalf("prompt did not arrive as a stream-json user message: %q", stdin)
	}
}

func TestClaudeStreamDenyReachesChildAndLateClickIsRefused(t *testing.T) {
	h := newClaudeStreamAdapter(t, "bash", "stream-json")
	run := h.start(t, engine.Request{OwnerID: "owner", Prompt: "do it", Workspace: t.TempDir()})
	request := run.request(t)
	if request.Effect != engine.EffectExecute {
		t.Fatal("Bash must be an execute effect", request.Effect)
	}
	if err := h.adapter.RespondPermission(t.Context(), "stream-native", request.ID, engine.PermissionDecision{Allow: false, Reason: "no thanks"}); err != nil {
		t.Fatal(err)
	}
	events := run.finish(t)
	data, _ := os.ReadFile(h.replies)
	if !strings.Contains(string(data), `"behavior":"deny"`) || !strings.Contains(string(data), "no thanks") || strings.Contains(string(data), `"allow"`) {
		t.Fatalf("deny not delivered: %q", data)
	}
	ended := kinds(events, engine.ToolEnd)
	if len(ended) != 1 || !ended[0].IsError {
		t.Fatal("a denied tool must end as an error result handed back to the model", ended)
	}
	if run.completed.Outcome != engine.Succeeded {
		t.Fatal("a refusal does not fail the turn", run.completed)
	}
	// The run is over: a click arriving now is late and must not be delivered.
	if err := h.adapter.RespondPermission(t.Context(), "stream-native", request.ID, engine.PermissionDecision{Allow: true}); !errors.Is(err, engine.ErrSessionNotFound) {
		t.Fatalf("late allow after the run must be refused, got %v", err)
	}
}

func TestClaudeStreamCancelWithPendingApprovalSettlesAsDeniedAndRefusesLateAllow(t *testing.T) {
	h := newClaudeStreamAdapter(t, "hang", "stream-json")
	run := h.start(t, engine.Request{OwnerID: "owner", Prompt: "do it", Workspace: t.TempDir()})
	request := run.request(t)
	run.cancel()
	select {
	case <-run.done:
	case <-time.After(10 * time.Second):
		t.Fatal("cancel did not end the run while an approval was waiting")
	}
	run.mu.Lock()
	events, completed := append([]engine.Event(nil), run.events...), run.completed
	run.mu.Unlock()
	if completed.Outcome != engine.Cancelled {
		t.Fatal("cancel must end as cancelled", completed)
	}
	resolved := kinds(events, engine.InteractionResolved)
	if len(resolved) != 1 || resolved[0].ID != request.ID || resolved[0].Resolution == nil || *resolved[0].Resolution.Allowed {
		t.Fatalf("the waiting approval must settle as denied: %+v", resolved)
	}
	if err := h.adapter.RespondPermission(t.Context(), "stream-native", request.ID, engine.PermissionDecision{Allow: true}); !errors.Is(err, engine.ErrSessionNotFound) {
		t.Fatalf("an allow arriving after cancel must be refused, got %v", err)
	}
	if data, _ := os.ReadFile(h.replies); len(data) != 0 {
		t.Fatal("nothing may be written to a cancelled child", string(data))
	}
}

func TestClaudeStreamNativeCancelRequestSettlesTheCard(t *testing.T) {
	h := newClaudeStreamAdapter(t, "cancel-request", "stream-json")
	run := h.start(t, engine.Request{OwnerID: "owner", Prompt: "do it", Workspace: t.TempDir()})
	request := run.request(t)
	events := run.finish(t)
	resolved := kinds(events, engine.InteractionResolved)
	if len(resolved) != 1 || resolved[0].ID != request.ID {
		t.Fatalf("native control_cancel_request must settle the pending card: %+v", resolved)
	}
	if err := h.adapter.RespondPermission(t.Context(), "stream-native", request.ID, engine.PermissionDecision{Allow: true}); !errors.Is(err, engine.ErrSessionNotFound) {
		t.Fatal("click on a cancelled request must be refused", err)
	}
}

func TestClaudeStreamDuplicateRequestFailsRunAndNeverDoubleAsks(t *testing.T) {
	h := newClaudeStreamAdapter(t, "duplicate", "stream-json")
	run := h.start(t, engine.Request{OwnerID: "owner", Prompt: "do it", Workspace: t.TempDir()})
	_ = run.request(t)
	select {
	case <-run.done:
	case <-time.After(10 * time.Second):
		t.Fatal("duplicate request id did not end the run")
	}
	run.mu.Lock()
	defer run.mu.Unlock()
	if run.completed.Outcome != engine.Failed || len(kinds(run.events, engine.PermissionRequest)) != 1 {
		t.Fatal("a repeated request id must fail the run with one card", run.completed, len(kinds(run.events, engine.PermissionRequest)))
	}
}

func TestClaudeStreamUnsupportedControlRequestAndQuestionsAreRefusedWithoutCards(t *testing.T) {
	for _, mode := range []string{"question"} {
		h := newClaudeStreamAdapter(t, mode, "stream-json")
		run := h.start(t, engine.Request{OwnerID: "owner", Prompt: "do it", Workspace: t.TempDir()})
		events := run.finish(t)
		if len(kinds(events, engine.PermissionRequest)) != 0 || len(kinds(events, engine.UserInputRequested)) != 0 {
			t.Fatal("AskUserQuestion has no card on this transport and must be refused", mode)
		}
		data, _ := os.ReadFile(h.replies)
		if !strings.Contains(string(data), `"behavior":"deny"`) {
			t.Fatalf("question must be refused to the model, got %q", data)
		}
	}
	h := newClaudeStreamAdapter(t, "unsupported", "stream-json")
	run := h.start(t, engine.Request{OwnerID: "owner", Prompt: "do it", Workspace: t.TempDir()})
	request := run.request(t)
	deadline := time.Now().Add(5 * time.Second)
	for {
		data, _ := os.ReadFile(h.replies)
		if strings.Contains(string(data), `"hook1"`) && strings.Contains(string(data), `"subtype":"error"`) {
			break
		}
		if time.Now().After(deadline) {
			t.Fatalf("a non-permission control request must get an explicit error reply, got %q", data)
		}
		time.Sleep(20 * time.Millisecond)
	}
	if err := h.adapter.RespondPermission(t.Context(), "stream-native", request.ID, engine.PermissionDecision{Allow: false}); err != nil {
		t.Fatal(err)
	}
	run.finish(t)
}

func TestClaudeStreamResumeAndOtherNativeIdentityRules(t *testing.T) {
	h := newClaudeStreamAdapter(t, "ask", "stream-json")
	if _, err := h.adapter.args(engine.Request{ResumeSession: "stream-native", Model: "opus"}, "r"); err != nil {
		t.Fatal(err)
	}
	args, _ := h.adapter.args(engine.Request{ResumeSession: "stream-native", Model: "opus"}, "r")
	if v, _ := flagValue(args, "--resume"); v != "stream-native" {
		t.Fatal("--resume must be kept on the stream-json transport", args)
	}
	if err := h.adapter.Execute(t.Context(), engine.Request{OwnerID: "o", Prompt: "p", Workspace: t.TempDir(), InputFiles: []engine.InputFile{{}}}, func(engine.Event) error { return nil }); !errors.Is(err, engine.ErrUnsupported) {
		t.Fatal("typed files stay unsupported for Claude", err)
	}
	if _, err := New(Config{Vendor: "codex", Transport: "stream-json"}); err == nil {
		t.Fatal("stream-json is a Claude transport only")
	}
}

func TestClaudeStreamTooOldBinaryFailsAsTypedStartupWithoutFallback(t *testing.T) {
	h := newClaudeStreamAdapter(t, "old-claude", "stream-json")
	run := h.start(t, engine.Request{OwnerID: "owner", Prompt: "do it", Workspace: t.TempDir()})
	select {
	case <-run.done:
	case <-time.After(10 * time.Second):
		t.Fatal("run did not end")
	}
	run.mu.Lock()
	defer run.mu.Unlock()
	failures := kinds(run.events, engine.Error)
	if run.completed.Outcome != engine.Failed || len(failures) != 1 || failures[0].Failure != engine.FailureStartup || !strings.Contains(failures[0].Text, "版本过旧") {
		t.Fatalf("flags rejected by an old Claude must surface as a startup failure: %+v %+v", failures, run.completed)
	}
}

func TestClaudeStreamOversizedInputIsDeniedNotAsked(t *testing.T) {
	h := newClaudeStreamAdapter(t, "big", "stream-json")
	run := h.start(t, engine.Request{OwnerID: "owner", Prompt: "do it", Workspace: t.TempDir()})
	events := run.finish(t)
	if len(kinds(events, engine.PermissionRequest)) != 0 {
		t.Fatal("an input over the cap must not become a card")
	}
	data, _ := os.ReadFile(h.replies)
	if !strings.Contains(string(data), `"behavior":"deny"`) || !strings.Contains(string(data), "too large") || strings.Contains(string(data), "updatedInput") {
		t.Fatalf("oversized input must be denied with a reason and never echoed back: %.200q", data)
	}
}
