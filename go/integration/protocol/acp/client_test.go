// INPUT: bufio, context, encoding/json, errors, fmt, os, strings, sync/atomic, testing, time, kernel/abi, native/process
// OUTPUT: Actual ACP helper initialization, sessions, permission, cancellation, IO and fail-closed regressions
// POS: [integration / external protocol] Local subprocess wire acceptance without real agents, network, account discovery or paid models.
package acp

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/go/platform/system/process"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
)

func TestACPHelper(t *testing.T) {
	if os.Getenv("ALVA_ACP_TEST_HELPER") != "1" {
		return
	}
	mode := os.Getenv("ALVA_ACP_TEST_MODE")
	encoder := json.NewEncoder(os.Stdout)
	reply := func(id json.RawMessage, result any) {
		_ = encoder.Encode(map[string]any{"jsonrpc": "2.0", "id": id, "result": result})
	}
	update := func(session, text string) {
		_ = encoder.Encode(map[string]any{"jsonrpc": "2.0", "method": "session/update", "params": map[string]any{"sessionId": session, "update": map[string]any{"sessionUpdate": "agent_message_chunk", "content": map[string]any{"type": "text", "text": text}}}})
	}
	var promptID json.RawMessage
	session := "session-1"
	canceled := false
	permissionCancelled := false
	count := 0
	scanner := bufio.NewScanner(os.Stdin)
	scanner.Buffer(make([]byte, 4096), 4<<20)
	for scanner.Scan() {
		var frame struct {
			ID     json.RawMessage `json:"id"`
			Method string          `json:"method"`
			Params json.RawMessage `json:"params"`
			Result json.RawMessage `json:"result"`
			Error  *RPCError       `json:"error"`
		}
		if json.Unmarshal(scanner.Bytes(), &frame) != nil {
			os.Exit(2)
		}
		switch frame.Method {
		case "initialize":
			version := Version
			if mode == "version" {
				version = 99
			}
			reply(frame.ID, map[string]any{"protocolVersion": version, "agentCapabilities": map[string]any{"loadSession": true}, "agentInfo": map[string]any{"name": "helper", "version": "1"}, "authMethods": []any{map[string]any{"id": "local", "name": "Local"}}})
			fmt.Fprintln(os.Stderr, "helper stderr observed")
			if mode == "tail-stderr" {
				_, _ = os.Stderr.Write([]byte("private tail 尾"))
			}
		case "authenticate":
			reply(frame.ID, map[string]any{})
		case "session/new":
			count++
			session = fmt.Sprintf("session-%d", count)
			reply(frame.ID, map[string]any{"sessionId": session})
			update(session, "created")
		case "session/load":
			var params LoadSessionRequest
			_ = json.Unmarshal(frame.Params, &params)
			session = params.SessionID
			update(session, "replayed")
			reply(frame.ID, map[string]any{})
		case "session/prompt":
			promptID = frame.ID
			switch mode {
			case "permission", "cancel", "bad-option":
				_ = encoder.Encode(map[string]any{"jsonrpc": "2.0", "id": "permission-1", "method": "session/request_permission", "params": map[string]any{"sessionId": session, "toolCall": map[string]any{"toolCallId": "tool-1", "title": "write"}, "options": []any{map[string]any{"optionId": "yes", "name": "Allow", "kind": "allow_once"}, map[string]any{"optionId": "no", "name": "Reject", "kind": "reject_once"}}}})
			case "hang":
			case "flood":
				for n := 0; n < 65; n++ {
					update(session, "flood")
				}
			case "invalid":
				_, _ = os.Stdout.Write([]byte("{\"jsonrpc\":\"2.0\",\"method\":\"session/update\",\"params\":{\"sessionId\":\"session-1\",\"update\":{\"sessionUpdate\":\"agent_message_chunk\",\"content\":{\"type\":\"text\",\"text\":\"\xff\"}}}}\n"))
			case "bad-completion":
				reply(promptID, map[string]any{})
			case "unsupported":
				_ = encoder.Encode(map[string]any{"jsonrpc": "2.0", "id": -5, "method": "fs/write_text_file", "params": map[string]any{"sessionId": session, "path": "/not-granted", "content": "never"}})
			default:
				update(session, "answer")
				reply(promptID, map[string]any{"stopReason": "end_turn"})
			}
		case "session/cancel":
			canceled = true
			if mode == "cancel" && permissionCancelled {
				update(session, "cleanup after cancellation")
				reply(promptID, map[string]any{"stopReason": "cancelled"})
			}
			if mode != "cancel" && mode != "hang" {
				reply(promptID, map[string]any{"stopReason": "cancelled"})
			}
		case "":
			if mode == "unsupported" {
				if frame.Error == nil || frame.Error.Code != -32601 {
					os.Exit(3)
				}
				reply(promptID, map[string]any{"stopReason": "end_turn"})
				continue
			}
			var response PermissionResponse
			_ = json.Unmarshal(frame.Result, &response)
			if mode == "cancel" {
				if response.Outcome.Outcome != "cancelled" {
					os.Exit(4)
				}
				if !canceled {
					permissionCancelled = true
					continue
				}
				update(session, "cleanup after cancellation")
				reply(promptID, map[string]any{"stopReason": "cancelled"})
			} else {
				if response.Outcome.OptionID != "yes" {
					os.Exit(5)
				}
				update(session, "approved")
				reply(promptID, map[string]any{"stopReason": "end_turn"})
			}
		}
	}
	os.Exit(0)
}

func fixtureConfig(t *testing.T, mode string) Config {
	t.Helper()
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	return Config{ID: "actual-connection", Process: process.Config{Command: executable, Args: []string{"-test.run=^TestACPHelper$"}, Cwd: t.TempDir(), Env: map[string]string{"ALVA_ACP_TEST_HELPER": "1", "ALVA_ACP_TEST_MODE": mode}}, ClientInfo: Implementation{Name: "tests", Version: "1"}, ConnectTimeout: 3 * time.Second, RequestTimeout: 3 * time.Second, CancelTimeout: time.Second}
}
func openFixture(t *testing.T, mode string, handler PermissionHandler, observer Observer) *Client {
	t.Helper()
	client, err := Open(context.Background(), fixtureConfig(t, mode), handler, observer)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
		defer cancel()
		if err := client.Close(ctx); err != nil && !knownSideFailure(err, client.sideFailures()) {
			t.Error(err)
		}
	})
	return client
}
func knownSideFailure(err, known error) bool {
	if err == nil {
		return true
	}
	if known != nil && errors.Is(known, err) {
		return true
	}
	if joined, ok := err.(interface{ Unwrap() []error }); ok {
		for _, child := range joined.Unwrap() {
			if !knownSideFailure(child, known) {
				return false
			}
		}
		return true
	}
	return false
}
func newFixtureSession(t *testing.T, c *Client) *Session {
	t.Helper()
	s, err := c.NewSession(context.Background(), c.config.Process.Cwd)
	if err != nil {
		t.Fatal(err)
	}
	return s
}

func TestOfficialACPProcessSessionPromptAndCompleteObserver(t *testing.T) {
	var mu sync.Mutex
	var events []IOEvent
	observer := ObserverFunc(func(_ context.Context, event IOEvent) error {
		mu.Lock()
		defer mu.Unlock()
		event.Data = append([]byte(nil), event.Data...)
		events = append(events, event)
		return nil
	})
	c := openFixture(t, "normal", nil, observer)
	if !c.Connected() || c.Initialize().ProtocolVersion != 1 {
		t.Fatal("handshake failed")
	}
	if err := c.Authenticate(context.Background(), "local"); err != nil {
		t.Fatal(err)
	}
	s := newFixtureSession(t, c)
	response, err := s.Prompt(context.Background(), []ContentBlock{Text("hello"), {Type: "resource_link", Name: "log", URI: "artifact://not-opened"}})
	if err != nil || response.StopReason != "end_turn" || s.Running() {
		t.Fatal(response, err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	first, _ := s.NextUpdate(ctx)
	second, _ := s.NextUpdate(ctx)
	if first.Content == nil || *first.Content.Text != "created" || second.Content == nil || *second.Content.Text != "answer" {
		t.Fatal(first, second)
	}
	if err := c.Close(ctx); err != nil {
		t.Fatal(err)
	}
	mu.Lock()
	defer mu.Unlock()
	seen := map[string]bool{}
	for _, event := range events {
		seen[event.Direction+"/"+event.Stage] = true
		if event.ConnectionID != "actual-connection" {
			t.Fatal("observer lost authority")
		}
	}
	for _, name := range []string{"stdin/write_attempt", "stdin/write_result", "stdout/received", "stderr/received"} {
		if !seen[name] {
			t.Fatal("incomplete observer", name)
		}
	}
}

func TestACPRealPermissionAndUnsupportedClientMethod(t *testing.T) {
	for _, mode := range []string{"permission", "unsupported"} {
		calls := 0
		handler := PermissionFunc(func(_ context.Context, request PermissionRequest) (PermissionOutcome, error) {
			calls++
			if request.SessionID != "session-1" || request.ToolCall.ToolCallID != "tool-1" {
				t.Error("permission binding lost")
			}
			return PermissionOutcome{Outcome: "selected", OptionID: "yes"}, nil
		})
		c := openFixture(t, mode, handler, nil)
		s := newFixtureSession(t, c)
		response, err := s.Prompt(context.Background(), []ContentBlock{Text("go")})
		if err != nil || response.StopReason != "end_turn" || (mode == "permission" && calls != 1) || (mode == "unsupported" && calls != 0) {
			t.Fatal(mode, response, err, calls)
		}
	}
}

func TestACPCancelSettlesPeerAndDiscardsLateApproval(t *testing.T) {
	entered, release := make(chan struct{}), make(chan struct{})
	handler := PermissionFunc(func(context.Context, PermissionRequest) (PermissionOutcome, error) {
		close(entered)
		<-release
		return PermissionOutcome{Outcome: "selected", OptionID: "yes"}, nil
	})
	var selected, cancelled atomic.Int32
	observer := ObserverFunc(func(_ context.Context, event IOEvent) error {
		if event.Direction == "stdin" && event.Stage == "write_attempt" {
			var frame struct {
				Result PermissionResponse `json:"result"`
			}
			_ = json.Unmarshal(event.Data, &frame)
			if frame.Result.Outcome.Outcome == "selected" {
				selected.Add(1)
			}
			if frame.Result.Outcome.Outcome == "cancelled" {
				cancelled.Add(1)
			}
		}
		return nil
	})
	c := openFixture(t, "cancel", handler, observer)
	s := newFixtureSession(t, c)
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	type result struct {
		response PromptResponse
		err      error
	}
	done := make(chan result, 1)
	go func() { response, err := s.Prompt(ctx, []ContentBlock{Text("go")}); done <- result{response, err} }()
	select {
	case <-entered:
	case <-time.After(3 * time.Second):
		t.Fatal("permission not entered")
	}
	cancel()
	select {
	case got := <-done:
		if !errors.Is(got.err, context.Canceled) || got.response.StopReason != "cancelled" {
			t.Fatal("cancellation faked or lost", got)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("cancel did not settle")
	}
	close(release)
	deadline := time.Now().Add(time.Second)
	for {
		c.mu.Lock()
		active := len(c.callbackIDs)
		c.mu.Unlock()
		if active == 0 {
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("late handler did not finish")
		}
		time.Sleep(time.Millisecond)
	}
	if selected.Load() != 0 || cancelled.Load() != 1 {
		t.Fatal("late approval escaped", selected.Load(), cancelled.Load())
	}
	if !c.Connected() {
		t.Fatal("cooperative session cancel killed connection")
	}
}

func TestACPCancelUnresponsiveProcessActuallyReaped(t *testing.T) {
	config := fixtureConfig(t, "hang")
	config.CancelTimeout = 50 * time.Millisecond
	c, err := Open(context.Background(), config, nil, nil)
	if err != nil {
		t.Fatal(err)
	}
	s := newFixtureSession(t, c)
	ctx, cancel := context.WithTimeout(context.Background(), 50*time.Millisecond)
	defer cancel()
	_, err = s.Prompt(ctx, []ContentBlock{Text("go")})
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal(err)
	}
	select {
	case <-c.p.Done():
	default:
		t.Fatal("returned before owned process reaped")
	}
	if c.Connected() {
		t.Fatal("unresponsive connection still usable")
	}
}

func TestACPRejectsWrongVersionFramesQueueOverflowAndForgedApproval(t *testing.T) {
	if c, err := Open(context.Background(), fixtureConfig(t, "version"), nil, nil); err == nil || c != nil {
		t.Fatal("accepted wrong protocol")
	}
	for _, mode := range []string{"invalid", "flood", "bad-option", "bad-completion"} {
		c := openFixture(t, mode, PermissionFunc(func(context.Context, PermissionRequest) (PermissionOutcome, error) {
			return PermissionOutcome{Outcome: "selected", OptionID: "not-offered"}, nil
		}), nil)
		s := newFixtureSession(t, c)
		_, err := s.Prompt(context.Background(), []ContentBlock{Text("go")})
		if err == nil || c.Connected() {
			t.Fatal("malformed peer/approval accepted", mode, err)
		}
		select {
		case <-c.p.Done():
		default:
			t.Fatal("failure returned before reap", mode)
		}
	}
}

func TestACPObserverFailureAndLoadHistoryRemainExplicit(t *testing.T) {
	failure := errors.New("journal full")
	observer := ObserverFunc(func(_ context.Context, event IOEvent) error {
		if event.Direction == "stdout" && strings.Contains(string(event.Data), `"text":"answer"`) {
			return failure
		}
		return nil
	})
	c := openFixture(t, "normal", nil, observer)
	s := newFixtureSession(t, c)
	if _, err := s.Prompt(context.Background(), []ContentBlock{Text("go")}); !errors.Is(err, failure) {
		t.Fatal("observer failure hidden", err)
	}
	c = openFixture(t, "normal", nil, nil)
	loaded, err := c.LoadSession(context.Background(), "known-session", c.config.Process.Cwd)
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	update, err := loaded.NextUpdate(ctx)
	if err != nil || update.Content == nil || *update.Content.Text != "replayed" {
		t.Fatal("explicit load did not deliver history", update, err)
	}
}

func TestACPPermissionCallbackCannotRewriteOriginalOptions(t *testing.T) {
	handler := PermissionFunc(func(_ context.Context, request PermissionRequest) (PermissionOutcome, error) {
		request.Options[0].OptionID = "forged"
		return PermissionOutcome{Outcome: "selected", OptionID: "forged"}, nil
	})
	c := openFixture(t, "permission", handler, nil)
	s := newFixtureSession(t, c)
	if _, err := s.Prompt(context.Background(), []ContentBlock{Text("go")}); err == nil {
		t.Fatal("handler rewrote original offered authority")
	}
}

func TestACPCanceledBeforeRequestDoesNotCloseUnrelatedConnection(t *testing.T) {
	c := openFixture(t, "normal", nil, nil)
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := c.NewSession(ctx, c.config.Process.Cwd); !errors.Is(err, context.Canceled) || !c.Connected() {
		t.Fatal("pre-cancelled request killed connection", err)
	}
	s := newFixtureSession(t, c)
	if _, err := s.Prompt(context.Background(), []ContentBlock{Text("still live")}); err != nil {
		t.Fatal(err)
	}
}

func TestACPContentUnionCapabilitiesAndEnvelopesAreStrict(t *testing.T) {
	text := "text"
	data := "YQ=="
	for _, block := range []ContentBlock{{Type: "text"}, {Type: "text", Text: &text, Data: &data}, {Type: "resource_link", URI: "artifact://x"}, {Type: "image", Data: &data, MimeType: "image/png"}} {
		if err := block.Validate(PromptCapabilities{}); err == nil {
			t.Fatal("invalid content accepted", block)
		}
	}
	if err := uniqueEnvelopeFields([]byte(`{"jsonrpc":"2.0","id":1,"id":2,"result":{}}`)); err == nil {
		t.Fatal("duplicate RPC identity accepted")
	}
	if _, err := rpcID(json.RawMessage(`null`)); err == nil {
		t.Fatal("null request identity accepted")
	}
}

func TestACPExplicitCancelBoundsAnUnresponsiveBackgroundPrompt(t *testing.T) {
	config := fixtureConfig(t, "hang")
	config.CancelTimeout = 50 * time.Millisecond
	started := make(chan struct{})
	observer := ObserverFunc(func(_ context.Context, event IOEvent) error {
		if event.Direction == "stdin" && event.Stage == "write_attempt" && strings.Contains(string(event.Data), `"method":"session/prompt"`) {
			close(started)
		}
		return nil
	})
	c, err := Open(context.Background(), config, nil, observer)
	if err != nil {
		t.Fatal(err)
	}
	s := newFixtureSession(t, c)
	done := make(chan error, 1)
	go func() { _, err := s.Prompt(context.Background(), []ContentBlock{Text("go")}); done <- err }()
	select {
	case <-started:
	case <-time.After(time.Second):
		t.Fatal("prompt not started")
	}
	if err := s.Cancel(context.Background()); err != nil {
		t.Fatal(err)
	}
	select {
	case err := <-done:
		if !errors.Is(err, context.Canceled) {
			t.Fatal(err)
		}
	case <-time.After(time.Second):
		t.Fatal("explicit cancel waited for long request timeout")
	}
	select {
	case <-c.p.Done():
	default:
		t.Fatal("explicit cancellation returned before reap")
	}
}

func TestACPSequentialTurnsMayReuseCompletedPermissionID(t *testing.T) {
	c := openFixture(t, "permission", PermissionFunc(func(context.Context, PermissionRequest) (PermissionOutcome, error) {
		return PermissionOutcome{Outcome: "selected", OptionID: "yes"}, nil
	}), nil)
	s := newFixtureSession(t, c)
	for range 20 {
		if _, err := s.Prompt(context.Background(), []ContentBlock{Text("next")}); err != nil {
			t.Fatal("completed callback id could not be reused", err)
		}
	}
}

func TestACPLateNativeApprovalFailureSurvivesCancellationAndClose(t *testing.T) {
	disk := errors.New("approval storage flush failed")
	entered, release := make(chan struct{}), make(chan struct{})
	handler := PermissionFunc(func(context.Context, PermissionRequest) (PermissionOutcome, error) {
		close(entered)
		<-release
		return PermissionOutcome{}, &abi.SessionError{Operation: "flush", SessionID: "actual", Err: disk}
	})
	c, err := Open(context.Background(), fixtureConfig(t, "cancel"), handler, nil)
	if err != nil {
		t.Fatal(err)
	}
	s := newFixtureSession(t, c)
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	done := make(chan error, 1)
	go func() { _, err := s.Prompt(ctx, []ContentBlock{Text("go")}); done <- err }()
	select {
	case <-entered:
	case <-time.After(time.Second):
		t.Fatal("approval not entered")
	}
	cancel()
	select {
	case err := <-done:
		if !errors.Is(err, context.Canceled) {
			t.Fatal(err)
		}
	case <-time.After(time.Second):
		t.Fatal("peer cancellation did not settle")
	}
	close(release)
	cleanup, stop := context.WithTimeout(context.Background(), time.Second)
	defer stop()
	if err := c.Close(cleanup); !errors.Is(err, disk) {
		t.Fatal("late native error lost during cleanup", err)
	}
	select {
	case <-c.Done():
	case <-cleanup.Done():
		t.Fatal("actual closed signal missing")
	}
	if !errors.Is(c.Err(), disk) || abi.OnlyCancellation(c.Err()) {
		t.Fatal("native failure downgraded", c.Err())
	}
}
