// INPUT: context, encoding/json, errors, reflect, strings, sync, testing, kernel/abi, kernel/bus, kernel/core, llm/wire
// OUTPUT: SDK assembly and complete execution regression tests
// POS: [L2.5 SDK assembly] Verifies executable model/tool orchestration, lifecycle failures, and plugin boundaries.
package core_test

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"reflect"
	"strings"
	"sync"
	"testing"

	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/kernel/bus"
	kernel "github.com/QuincySx/Alva/go/sdk/kernel/core"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type modelFunc func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error

func (model modelFunc) Stream(ctx context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	return model(ctx, request, emit)
}

func textModel(text string) abi.LanguageModel {
	return modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: text}); err != nil {
			return err
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
}
func callChunk(id string) wire.StreamChunk {
	return wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: id, Name: "test", Arguments: json.RawMessage(`{"path":"notes"}`)}}
}

type testTool struct {
	execute func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error)
}

func (*testTool) Name() string                      { return "test" }
func (*testTool) Description() string               { return "test tool" }
func (*testTool) ParametersSchema() json.RawMessage { return json.RawMessage(`{"type":"object"}`) }
func (tool *testTool) Execute(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	if tool.execute != nil {
		return tool.execute(ctx, execution, call)
	}
	return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: "tool value"}}}, nil
}
func (*testTool) Scheduling(call wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{Resources: []abi.ResourceClaim{{Resource: string(call.Arguments), Access: abi.AccessRead}}}
}
func (*testTool) DescribeEffect(call wire.ToolCall) abi.Effect {
	return abi.Effect{Kind: abi.EffectRead, Description: call.ID}
}
func (*testTool) IsReadOnly(call wire.ToolCall) bool {
	return strings.Contains(string(call.Arguments), "notes")
}

func build(t *testing.T, builder *core.Builder) *core.Agent {
	t.Helper()
	agent, err := builder.Build(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	return agent
}
func discard(abi.Event) error { return nil }
func records(t *testing.T, session abi.SessionStore) []abi.SessionRecord {
	t.Helper()
	result, err := session.Records(context.Background(), 0)
	if err != nil {
		t.Fatal(err)
	}
	return result
}

func TestModelToolModelCommitsIntentBeforeSideEffect(t *testing.T) {
	session := core.NewMemorySession("native")
	turn := 0
	model := modelFunc(func(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		turn++
		stored := records(t, session)
		if stored[len(stored)-1].Kind != abi.RecordModelIntent {
			t.Fatal("model called before durable intent")
		}
		if len(request.Tools) != 1 || request.Tools[0].Name != "test" {
			t.Fatal("tool definitions missing")
		}
		if turn == 1 {
			if err := emit(callChunk("call")); err != nil {
				return err
			}
		} else {
			last := request.Messages[len(request.Messages)-1]
			if last.Role != wire.RoleTool || last.Content[0].ToolResult.Content[0].Text != "tool value" {
				t.Fatalf("tool result not returned to model: %#v", last)
			}
			if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: "finished"}); err != nil {
				return err
			}
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	sideEffects := 0
	tool := &testTool{execute: func(_ context.Context, execution abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
		sideEffects++
		stored := records(t, session)
		intent := stored[len(stored)-1].ToolIntent
		if intent == nil || intent.Call.ID != call.ID || intent.RunID != execution.RunID || !intent.ReadOnly || intent.Scheduling.Resources[0].Resource != string(call.Arguments) {
			t.Fatal("missing invocation-specific durable intent")
		}
		if execution.SessionID != "native" || execution.RunID == "" || execution.CallID != "call" || execution.Workspace != "/workspace" || execution.Progress == nil {
			t.Fatalf("incorrect execution context: %#v", execution)
		}
		return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: "tool value"}}}, nil
	}}
	agent := build(t, core.NewBuilder(model, session).Tool(tool).Workspace("/workspace"))
	var events []abi.Event
	if err := agent.Run(context.Background(), []wire.Message{wire.TextMessage(wire.RoleUser, "start")}, func(event abi.Event) error { events = append(events, event); return nil }); err != nil {
		t.Fatal(err)
	}
	if turn != 2 || sideEffects != 1 || len(withoutHookAudits(records(t, session))) != 10 {
		t.Fatalf("turn=%d effects=%d records=%d", turn, sideEffects, len(records(t, session)))
	}
	if events[len(events)-1].Kind != abi.EventRunCompleted {
		t.Fatal("terminal missing")
	}
	for _, event := range events {
		if event.Kind == abi.EventToolStarted {
			if event.Record == nil {
				t.Fatal("intent reference missing")
			}
			stored := records(t, session)
			if event.Record.Sequence == 0 || event.Record.Sequence > uint64(len(stored)) || stored[event.Record.Sequence-1].ToolIntent == nil || stored[event.Record.Sequence-1].ToolIntent.Call.ID != "call" {
				t.Fatal("intent reference points to another fact")
			}
		}
	}
}

func TestCancellationSettlesExecutedAndUnexecutedCalls(t *testing.T) {
	session := core.NewMemorySession("cancel")
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	model := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		for _, id := range []string{"first", "second"} {
			if err := emit(callChunk(id)); err != nil {
				return err
			}
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	calls := 0
	tool := &testTool{execute: func(ctx context.Context, _ abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
		calls++
		cancel()
		<-ctx.Done()
		return wire.ToolResult{}, ctx.Err()
	}}
	err := build(t, core.NewBuilder(model, session).Tool(tool)).Run(ctx, []wire.Message{wire.TextMessage(wire.RoleUser, "go")}, discard)
	if !errors.Is(err, context.Canceled) || calls != 1 {
		t.Fatalf("err=%v calls=%d", err, calls)
	}
	var outcomes []*abi.ToolOutcome
	for _, record := range records(t, session) {
		if record.ToolOutcome != nil {
			outcomes = append(outcomes, record.ToolOutcome)
		}
	}
	if len(outcomes) != 2 || outcomes[0].Status != abi.OutcomeCancelled || outcomes[1].Status != abi.OutcomeCancelled || outcomes[1].CallID != "second" {
		t.Fatalf("unmatched cancellation: %#v", outcomes)
	}
	if strings.HasPrefix(outcomes[0].Result.Content[0].Text, "Not executed") {
		t.Fatal("executed tool falsely labeled unexecuted")
	}
}

type failingSession struct {
	*core.MemorySession
	appendErrAt int
	appendCalls int
	readErr     error
	flushErr    error
	flushed     bool
}

func (session *failingSession) AppendRecords(ctx context.Context, batch []abi.SessionRecord) ([]abi.SessionRecord, error) {
	// Inject failures at semantic fact writes, independently of compact audit
	// persistence. Dedicated audit regressions exercise audit append failures.
	semantic := false
	for _, record := range batch {
		if record.Kind != abi.RecordExtension {
			session.appendCalls++
			semantic = true
			break
		}
	}
	if semantic && session.appendCalls == session.appendErrAt {
		return nil, errors.New("append fixture failure")
	}
	return session.MemorySession.AppendRecords(ctx, batch)
}
func (session *failingSession) Messages(ctx context.Context) ([]wire.Message, error) {
	if session.readErr != nil {
		return nil, session.readErr
	}
	return session.MemorySession.Messages(ctx)
}
func (session *failingSession) Flush(ctx context.Context) error {
	session.flushed = true
	if err := ctx.Err(); err != nil {
		return err
	}
	return session.flushErr
}

func TestInputAppendFailurePreventsModelAndStillFlushes(t *testing.T) {
	session := &failingSession{MemorySession: core.NewMemorySession("fail"), appendErrAt: 1}
	called := false
	model := modelFunc(func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error { called = true; return nil })
	err := build(t, core.NewBuilder(model, session)).Run(context.Background(), []wire.Message{wire.TextMessage(wire.RoleUser, "x")}, discard)
	if err == nil || called || !session.flushed || len(records(t, session)) != 0 {
		t.Fatalf("err=%v called=%v flushed=%v", err, called, session.flushed)
	}
	var failure *abi.SessionError
	if !errors.As(err, &failure) || failure.Operation != "append_records" || failure.SessionID != "fail" {
		t.Fatalf("missing typed session failure: %v", err)
	}
}

func TestSessionTimeoutIsNotOrdinaryTaskCancellation(t *testing.T) {
	for _, operation := range []string{"messages", "flush"} {
		t.Run(operation, func(t *testing.T) {
			session := &failingSession{MemorySession: core.NewMemorySession("timeout")}
			if operation == "messages" {
				session.readErr = context.DeadlineExceeded
			} else {
				session.flushErr = context.DeadlineExceeded
			}
			model := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
				return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
			})
			err := build(t, core.NewBuilder(model, session)).Run(context.Background(), nil, discard)
			var failure *abi.SessionError
			if !errors.As(err, &failure) || failure.Operation != operation || failure.SessionID != "timeout" || !errors.Is(err, context.DeadlineExceeded) || abi.OnlyCancellation(err) {
				t.Fatalf("storage timeout incorrectly classified: %v", err)
			}
		})
	}
}

func TestResponseCommitFailurePreventsToolAndSettlesModel(t *testing.T) {
	session := &failingSession{MemorySession: core.NewMemorySession("fail"), appendErrAt: 3}
	called := false
	model := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		if err := emit(callChunk("call")); err != nil {
			return err
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	tool := &testTool{execute: func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
		called = true
		return wire.ToolResult{}, nil
	}}
	err := build(t, core.NewBuilder(model, session).Tool(tool)).Run(context.Background(), []wire.Message{wire.TextMessage(wire.RoleUser, "x")}, discard)
	stored := withoutHookAudits(records(t, session))
	if err == nil || called || len(stored) != 3 || stored[2].ModelOutcome == nil || stored[2].ModelOutcome.Status != abi.OutcomeFailed {
		t.Fatalf("err=%v called=%v records=%#v", err, called, stored)
	}
}

func TestFailedOutcomeWriteDoesNotFalselyCancelExecutedCall(t *testing.T) {
	session := &failingSession{MemorySession: core.NewMemorySession("fail"), appendErrAt: 4}
	model := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		for _, id := range []string{"first", "second"} {
			if err := emit(callChunk(id)); err != nil {
				return err
			}
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	calls := 0
	tool := &testTool{execute: func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
		calls++
		return wire.ToolResult{}, nil
	}}
	err := build(t, core.NewBuilder(model, session).Tool(tool)).Run(context.Background(), []wire.Message{wire.TextMessage(wire.RoleUser, "x")}, discard)
	if err == nil || calls != 1 {
		t.Fatalf("err=%v calls=%d", err, calls)
	}
	var outcomes []*abi.ToolOutcome
	for _, record := range records(t, session) {
		if record.ToolOutcome != nil {
			outcomes = append(outcomes, record.ToolOutcome)
		}
	}
	if len(outcomes) != 1 || outcomes[0].CallID != "second" {
		t.Fatalf("executed but uncommitted outcome must remain explicitly unknown: %#v", outcomes)
	}
}

func TestFlushFailureCannotEmitSuccess(t *testing.T) {
	fixture := errors.New("flush fixture")
	session := &failingSession{MemorySession: core.NewMemorySession("flush"), flushErr: fixture}
	var terminal abi.Event
	err := build(t, core.NewBuilder(textModel("done"), session)).Run(context.Background(), nil, func(event abi.Event) error { terminal = event; return nil })
	if !errors.Is(err, fixture) || terminal.Kind != abi.EventRunFailed || !strings.Contains(terminal.Error, "flush fixture") {
		t.Fatalf("err=%v terminal=%#v", err, terminal)
	}
}

func TestReadErrorDoesNotReachModel(t *testing.T) {
	fixture := errors.New("read fixture")
	session := &failingSession{MemorySession: core.NewMemorySession("read"), readErr: fixture}
	called := false
	model := modelFunc(func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error { called = true; return nil })
	if err := build(t, core.NewBuilder(model, session)).Run(context.Background(), nil, discard); !errors.Is(err, fixture) || called {
		t.Fatalf("err=%v called=%v", err, called)
	}
}

func TestModelFailurePreservesPartialTextWithoutDanglingCall(t *testing.T) {
	fixture := errors.New("stream fixture")
	session := core.NewMemorySession("stream")
	model := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: "partial"}); err != nil {
			return err
		}
		if err := emit(callChunk("call")); err != nil {
			return err
		}
		return fixture
	})
	err := build(t, core.NewBuilder(model, session).Tool(&testTool{})).Run(context.Background(), nil, discard)
	if !errors.Is(err, fixture) {
		t.Fatal(err)
	}
	messages, _ := session.Messages(context.Background())
	if len(messages) != 1 || len(messages[0].Content) != 1 || messages[0].Text() != "partial" {
		t.Fatalf("partial conversation: %#v", messages)
	}
	for _, record := range records(t, session) {
		if record.ToolIntent != nil {
			t.Fatal("failed model response must not authorize tool intent")
		}
	}
}

func TestModelIgnoringCallbackErrorCannotExecuteTool(t *testing.T) {
	fixture := errors.New("subscriber fixture")
	called := false
	model := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		_ = emit(wire.StreamChunk{Kind: wire.ChunkText, Text: "x"})
		_ = emit(callChunk("call"))
		_ = emit(wire.StreamChunk{Kind: wire.ChunkFinish})
		return nil
	})
	tool := &testTool{execute: func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
		called = true
		return wire.ToolResult{}, nil
	}}
	err := build(t, core.NewBuilder(model, core.NewMemorySession("subscriber")).Tool(tool)).Run(context.Background(), nil, func(event abi.Event) error {
		if event.Kind == abi.EventTextDelta {
			return fixture
		}
		return nil
	})
	if !errors.Is(err, fixture) || called {
		t.Fatalf("err=%v called=%v", err, called)
	}
}

func TestInvalidStreamsDoNotExecuteTools(t *testing.T) {
	for _, fixture := range []string{"duplicate", "unfinished", "oversized", "after-finish"} {
		t.Run(fixture, func(t *testing.T) {
			called := false
			model := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
				switch fixture {
				case "duplicate":
					if err := emit(callChunk("same")); err != nil {
						return err
					}
					return emit(callChunk("same"))
				case "unfinished":
					return emit(callChunk("call"))
				case "oversized":
					return emit(wire.StreamChunk{Kind: wire.ChunkText, Text: strings.Repeat("x", 101)})
				default:
					if err := emit(wire.StreamChunk{Kind: wire.ChunkFinish}); err != nil {
						return err
					}
					return emit(callChunk("call"))
				}
			})
			tool := &testTool{execute: func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
				called = true
				return wire.ToolResult{}, nil
			}}
			err := build(t, core.NewBuilder(model, core.NewMemorySession(fixture)).Tool(tool).MaxResponseBytes(100)).Run(context.Background(), nil, discard)
			if err == nil || called {
				t.Fatalf("err=%v called=%v", err, called)
			}
		})
	}
}

func TestInvalidToolResultIsRecordedAndTerminatesRun(t *testing.T) {
	turn := 0
	model := modelFunc(func(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		turn++
		if turn == 1 {
			if err := emit(callChunk("call")); err != nil {
				return err
			}
		} else {
			t.Fatal("invalid protocol result reached another model request")
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	tool := &testTool{execute: func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
		return wire.ToolResult{CallID: "wrong"}, nil
	}}
	session := core.NewMemorySession("invalid-result")
	err := build(t, core.NewBuilder(model, session).Tool(tool)).Run(context.Background(), nil, discard)
	if err == nil || !strings.Contains(err.Error(), "mismatched") || turn != 1 {
		t.Fatalf("err=%v turn=%d", err, turn)
	}
	stored := records(t, session)
	if outcome := stored[len(stored)-2].ToolOutcome; outcome == nil || outcome.Status != abi.OutcomeFailed || !outcome.Result.IsError {
		t.Fatal("protocol failure evidence missing")
	}
}

func TestToolBusinessErrorCanReturnToModel(t *testing.T) {
	turn := 0
	model := modelFunc(func(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		turn++
		if turn == 1 {
			if err := emit(callChunk("business")); err != nil {
				return err
			}
		} else {
			result := request.Messages[len(request.Messages)-1].Content[0].ToolResult
			if !result.IsError || result.Content[0].Text != "file not found" {
				t.Fatal("business error did not reach model unchanged")
			}
			if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: "handled"}); err != nil {
				return err
			}
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	tool := &testTool{execute: func(_ context.Context, _ abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
		return wire.ToolResult{CallID: call.ID, IsError: true, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: "file not found"}}}, nil
	}}
	session := core.NewMemorySession("business")
	if err := build(t, core.NewBuilder(model, session).Tool(tool)).Run(context.Background(), nil, discard); err != nil {
		t.Fatal(err)
	}
	if turn != 2 {
		t.Fatalf("turn=%d", turn)
	}
	var found bool
	for _, record := range records(t, session) {
		if record.ToolOutcome != nil {
			found = true
			if record.ToolOutcome.Status != abi.OutcomeFailed || !record.ToolOutcome.Result.IsError {
				t.Fatal("business outcome must remain explicit failure")
			}
		}
	}
	if !found {
		t.Fatal("missing business outcome")
	}
}

func twoCallModel(turns *int) abi.LanguageModel {
	return modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		*turns++
		if *turns > 1 {
			return errors.New("unexpected second model request")
		}
		for _, id := range []string{"first", "second"} {
			if err := emit(callChunk(id)); err != nil {
				return err
			}
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
}

func TestToolExecutionErrorHaltsAndBackfillsRemainingCalls(t *testing.T) {
	fixture := errors.New("execution transport fixture")
	turns, calls := 0, 0
	session := core.NewMemorySession("fatal-tool")
	tool := &testTool{execute: func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
		calls++
		return wire.ToolResult{}, fixture
	}}
	err := build(t, core.NewBuilder(twoCallModel(&turns), session).Tool(tool)).Run(context.Background(), nil, discard)
	if !errors.Is(err, fixture) || turns != 1 || calls != 1 {
		t.Fatalf("err=%v turns=%d calls=%d", err, turns, calls)
	}
	assertAbortedToolPair(t, session, abi.OutcomeFailed, fixture.Error())
}

type deniedToolMiddleware struct{ failure error }

func (deniedToolMiddleware) Name() string { return "deny" }
func (middleware deniedToolMiddleware) WrapTool(context.Context, abi.ToolExecutionContext, wire.ToolCall, abi.ToolFunc) (wire.ToolResult, error) {
	return wire.ToolResult{}, middleware.failure
}

func TestToolWrapperDenialHasNoSideEffectAndBackfills(t *testing.T) {
	fixture := errors.New("approval denied fixture")
	turns, calls := 0, 0
	session := core.NewMemorySession("denied-tool")
	tool := &testTool{execute: func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
		calls++
		return wire.ToolResult{}, nil
	}}
	err := build(t, core.NewBuilder(twoCallModel(&turns), session).Tool(tool).Middleware(deniedToolMiddleware{fixture})).Run(context.Background(), nil, discard)
	if !errors.Is(err, fixture) || turns != 1 || calls != 0 {
		t.Fatalf("err=%v turns=%d calls=%d", err, turns, calls)
	}
	assertAbortedToolPair(t, session, abi.OutcomeFailed, fixture.Error())
}

func TestCancellationPreservesConcurrentExecutionFailure(t *testing.T) {
	fixture := errors.New("concurrent connection failure")
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	started, cancelled := make(chan struct{}), make(chan struct{})
	go func() { <-started; cancel(); close(cancelled) }()
	turns, calls := 0, 0
	session := core.NewMemorySession("cancel-and-failure")
	tool := &testTool{execute: func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
		calls++
		close(started)
		<-cancelled
		return wire.ToolResult{}, fixture
	}}
	err := build(t, core.NewBuilder(twoCallModel(&turns), session).Tool(tool)).Run(ctx, nil, discard)
	if !errors.Is(err, fixture) || !errors.Is(err, context.Canceled) || turns != 1 || calls != 1 {
		t.Fatalf("err=%v turns=%d calls=%d", err, turns, calls)
	}
	assertAbortedToolPair(t, session, abi.OutcomeFailed, fixture.Error())
}

func assertAbortedToolPair(t *testing.T, session *core.MemorySession, firstStatus abi.OutcomeStatus, failure string) {
	t.Helper()
	var outcomes []*abi.ToolOutcome
	for _, record := range records(t, session) {
		if record.ToolOutcome != nil {
			outcomes = append(outcomes, record.ToolOutcome)
		}
	}
	if len(outcomes) != 2 || outcomes[0].CallID != "first" || outcomes[0].Status != firstStatus || !strings.Contains(outcomes[0].Error, failure) || outcomes[1].CallID != "second" || outcomes[1].Status != abi.OutcomeCancelled || !strings.HasPrefix(outcomes[1].Result.Content[0].Text, "Not executed:") {
		t.Fatalf("unmatched abort outcomes: %#v", outcomes)
	}
}

type plugin struct {
	name      string
	register  func(context.Context, *core.Registrar) error
	finalize  func(context.Context, core.LateContext) error
	lateTools func(context.Context, core.LateContext) ([]abi.Tool, error)
}

func (p plugin) Name() string { return p.name }
func (p plugin) Register(ctx context.Context, registrar *core.Registrar) error {
	if p.register != nil {
		return p.register(ctx, registrar)
	}
	return nil
}
func (p plugin) Finalize(ctx context.Context, late core.LateContext) ([]abi.Tool, error) {
	if p.lateTools != nil {
		return p.lateTools(ctx, late)
	}
	if p.finalize != nil {
		return nil, p.finalize(ctx, late)
	}
	return nil, nil
}

func TestPluginDuplicatesFailBeforeAnyRegistration(t *testing.T) {
	called := false
	p := plugin{name: "same", register: func(context.Context, *core.Registrar) error { called = true; return nil }}
	_, err := core.NewBuilder(textModel(""), core.NewMemorySession("plugins")).Plugin(p).Plugin(p).Build(context.Background())
	if err == nil || called {
		t.Fatalf("err=%v called=%v", err, called)
	}
}

func TestToolDuplicatesFailAssembly(t *testing.T) {
	p := plugin{name: "duplicate-tool", register: func(_ context.Context, registrar *core.Registrar) error { return registrar.Tool(&testTool{}) }}
	if _, err := core.NewBuilder(textModel(""), core.NewMemorySession("tools")).Tool(&testTool{}).Plugin(p).Build(context.Background()); err == nil {
		t.Fatal("duplicate tool accepted")
	}
}

type capability interface{ Value() string }
type capabilityImpl struct{}

func (capabilityImpl) Value() string { return "late value" }

func TestFinalizeSeesAllContributionsAndReadOnlyBus(t *testing.T) {
	var retained *core.Registrar
	first := plugin{name: "first", register: func(_ context.Context, registrar *core.Registrar) error {
		retained = registrar
		return registrar.SystemPrompt("policy")
	}, finalize: func(_ context.Context, late core.LateContext) error {
		service, ok := bus.Resolve[capability](late.Bus)
		if !ok || service.Value() != "late value" {
			t.Fatal("late capability missing")
		}
		if _, writable := late.Session.(abi.ConversationLog); writable {
			t.Fatal("finalizer recovered history write authority")
		}
		if len(late.Assembly.Plugins) != 2 || len(late.Assembly.Tools) != 1 {
			t.Fatal("late assembly incomplete")
		}
		late.Assembly.Tools[0] = "mutated"
		if err := retained.Tool(&testTool{}); err == nil {
			t.Fatal("late registration accepted")
		}
		if err := bus.Provide[string](retained.BusWriter(), "late"); err == nil {
			t.Fatal("late bus write accepted")
		}
		return nil
	}}
	second := plugin{name: "second", register: func(_ context.Context, registrar *core.Registrar) error {
		if err := registrar.Tool(&testTool{}); err != nil {
			return err
		}
		return bus.Provide[capability](registrar.BusWriter(), capabilityImpl{})
	}}
	agent := build(t, core.NewBuilder(textModel(""), core.NewMemorySession("finalize")).Plugin(first).Plugin(second))
	snapshot := agent.Snapshot()
	if snapshot.Tools[0] != "test" || snapshot.Plugins[0].SystemPromptCount != 1 {
		t.Fatalf("bad snapshot: %#v", snapshot)
	}
	snapshot.Plugins[1].Tools[0] = "mutated"
	if agent.Snapshot().Plugins[1].Tools[0] != "test" {
		t.Fatal("snapshot ownership leaked")
	}
}

func TestPluginFailurePropagatesStage(t *testing.T) {
	for _, stage := range []string{"register", "finalize"} {
		t.Run(stage, func(t *testing.T) {
			fixture := errors.New("fixture")
			p := plugin{name: "failure"}
			if stage == "register" {
				p.register = func(context.Context, *core.Registrar) error { return fixture }
			} else {
				p.finalize = func(context.Context, core.LateContext) error { return fixture }
			}
			_, err := core.NewBuilder(textModel(""), core.NewMemorySession(stage)).Plugin(p).Build(context.Background())
			if !errors.Is(err, fixture) || !strings.Contains(err.Error(), stage) {
				t.Fatal(err)
			}
		})
	}
}

func TestFinalizeDiscoversLaterServiceAndReturnsUsableTool(t *testing.T) {
	session := core.NewMemorySession("late-tools")
	turn := 0
	model := modelFunc(func(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		turn++
		if len(request.Tools) != 1 || request.Tools[0].Name != "test" {
			t.Fatal("late tool absent from model definitions")
		}
		if turn == 1 {
			if err := emit(callChunk("late")); err != nil {
				return err
			}
		} else if request.Messages[len(request.Messages)-1].Content[0].ToolResult.Content[0].Text != "late value" {
			t.Fatal("late tool was not actually invoked")
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	first := plugin{name: "discover", lateTools: func(_ context.Context, late core.LateContext) ([]abi.Tool, error) {
		service, ok := bus.Resolve[capability](late.Bus)
		if !ok || late.Model == nil || late.Session.ID() != "late-tools" || len(late.ToolDefinitions) != 0 {
			t.Fatal("late context did not include complete registered capabilities")
		}
		return []abi.Tool{&testTool{execute: func(_ context.Context, _ abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
			return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: service.Value()}}}, nil
		}}}, nil
	}}
	second := plugin{name: "provide", register: func(_ context.Context, registrar *core.Registrar) error {
		return bus.Provide[capability](registrar.BusWriter(), capabilityImpl{})
	}}
	agent := build(t, core.NewBuilder(model, session).Plugin(first).Plugin(second))
	if err := agent.Run(context.Background(), nil, discard); err != nil {
		t.Fatal(err)
	}
	snapshot := agent.Snapshot()
	if turn != 2 || len(snapshot.Plugins[0].Tools) != 0 || !reflect.DeepEqual(snapshot.Plugins[0].FinalizedTools, []string{"test"}) {
		t.Fatalf("snapshot=%#v turn=%d", snapshot, turn)
	}
	snapshot.Plugins[0].FinalizedTools[0] = "mutated"
	if agent.Snapshot().Plugins[0].FinalizedTools[0] != "test" {
		t.Fatal("finalized tools snapshot leaked ownership")
	}
}

func TestFinalizeRejectsDuplicateToolNames(t *testing.T) {
	for _, fixture := range []string{"registered", "finalized"} {
		t.Run(fixture, func(t *testing.T) {
			late := plugin{name: "late", lateTools: func(context.Context, core.LateContext) ([]abi.Tool, error) { return []abi.Tool{&testTool{}}, nil }}
			builder := core.NewBuilder(textModel(""), core.NewMemorySession("duplicate-late"))
			if fixture == "registered" {
				builder.Tool(&testTool{})
			} else {
				builder.Plugin(plugin{name: "earlier-late", lateTools: late.lateTools})
			}
			if _, err := builder.Plugin(late).Build(context.Background()); err == nil || !strings.Contains(err.Error(), "duplicate tool") {
				t.Fatalf("duplicate late contribution accepted: %v", err)
			}
		})
	}
}

type customLoop struct{ called bool }

func (loop *customLoop) Run(context.Context, kernel.RunConfig, []wire.Message, func(abi.Event) error) error {
	loop.called = true
	return nil
}
func TestAgentLoopIsReplaceable(t *testing.T) {
	loop := &customLoop{}
	if err := build(t, core.NewBuilder(textModel(""), core.NewMemorySession("loop")).Loop(loop)).Run(context.Background(), nil, discard); err != nil || !loop.called {
		t.Fatalf("err=%v called=%v", err, loop.called)
	}
}

type projection struct {
	calls *[]string
	name  string
}

func (middleware projection) Name() string { return middleware.name }
func (middleware projection) BeforeModel(_ context.Context, _ abi.RuntimeContext, request *abi.ModelRequest) error {
	*middleware.calls = append(*middleware.calls, "before:"+middleware.name)
	request.Messages[0].Content[0].Text = "projected"
	return nil
}
func (middleware projection) AfterModel(context.Context, abi.RuntimeContext, *wire.Message) error {
	*middleware.calls = append(*middleware.calls, "after:"+middleware.name)
	return nil
}
func (middleware projection) WrapTool(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall, next abi.ToolFunc) (wire.ToolResult, error) {
	*middleware.calls = append(*middleware.calls, "enter:"+middleware.name)
	result, err := next(ctx, execution, call)
	*middleware.calls = append(*middleware.calls, "exit:"+middleware.name)
	return result, err
}

func TestMiddlewareProjectionAndOnionOrder(t *testing.T) {
	var order []string
	turn := 0
	model := modelFunc(func(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		turn++
		if request.Messages[0].Text() != "projected" {
			t.Fatal("projection not applied")
		}
		if turn == 1 {
			if err := emit(callChunk("call")); err != nil {
				return err
			}
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	session := core.NewMemorySession("middleware")
	agent := build(t, core.NewBuilder(model, session).Tool(&testTool{}).Middleware(projection{&order, "outer"}).Middleware(projection{&order, "inner"}))
	if err := agent.Run(context.Background(), []wire.Message{wire.TextMessage(wire.RoleUser, "original")}, discard); err != nil {
		t.Fatal(err)
	}
	messages, _ := session.Messages(context.Background())
	if messages[0].Text() != "original" {
		t.Fatal("projection mutated durable transcript")
	}
	want := []string{"before:outer", "before:inner", "after:inner", "after:outer", "enter:outer", "enter:inner", "exit:inner", "exit:outer", "before:outer", "before:inner", "after:inner", "after:outer"}
	if !reflect.DeepEqual(order, want) {
		t.Fatalf("order=%v", order)
	}
}

type lifecycleProjection struct{ projection }

func (middleware lifecycleProjection) OnRunStart(context.Context, abi.RuntimeContext) error {
	*middleware.calls = append(*middleware.calls, "start:"+middleware.name)
	return nil
}
func (middleware lifecycleProjection) OnRunEnd(_ context.Context, _ abi.RuntimeContext, _ error) error {
	*middleware.calls = append(*middleware.calls, "end:"+middleware.name)
	return nil
}

func TestMiddlewareUnwindsInReverseOnSuccessAndStreamFailure(t *testing.T) {
	for _, fail := range []bool{false, true} {
		t.Run(fmt.Sprint(fail), func(t *testing.T) {
			var order []string
			fixture := errors.New("stream fixture")
			model := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
				if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: "partial"}); err != nil {
					return err
				}
				if fail {
					return fixture
				}
				return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
			})
			agent := build(t, core.NewBuilder(model, core.NewMemorySession("unwind")).Middleware(lifecycleProjection{projection{&order, "outer"}}).Middleware(lifecycleProjection{projection{&order, "inner"}}))
			err := agent.Run(context.Background(), []wire.Message{wire.TextMessage(wire.RoleUser, "input")}, discard)
			want := []string{"start:outer", "start:inner", "before:outer", "before:inner"}
			if fail {
				if !errors.Is(err, fixture) {
					t.Fatal(err)
				}
			} else {
				if err != nil {
					t.Fatal(err)
				}
				want = append(want, "after:inner", "after:outer")
			}
			want = append(want, "end:inner", "end:outer")
			if !reflect.DeepEqual(order, want) {
				t.Fatalf("order=%v want=%v", order, want)
			}
		})
	}
}

func TestConcurrentRunsAreRejected(t *testing.T) {
	started := make(chan struct{})
	release := make(chan struct{})
	model := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		close(started)
		<-release
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	agent := build(t, core.NewBuilder(model, core.NewMemorySession("concurrent")))
	var wait sync.WaitGroup
	wait.Add(1)
	go func() {
		defer wait.Done()
		if err := agent.Run(context.Background(), nil, discard); err != nil {
			t.Error(err)
		}
	}()
	<-started
	if err := agent.Run(context.Background(), nil, discard); !errors.Is(err, core.ErrRunInProgress) {
		t.Fatal(err)
	}
	close(release)
	wait.Wait()
}

type logOnly struct{ session *core.MemorySession }

func (log logOnly) ID() string { return log.session.ID() }
func (log logOnly) Messages(ctx context.Context) ([]wire.Message, error) {
	return log.session.Messages(ctx)
}
func (log logOnly) AppendRecords(ctx context.Context, records []abi.SessionRecord) ([]abi.SessionRecord, error) {
	return log.session.AppendRecords(ctx, records)
}
func (log logOnly) Flush(ctx context.Context) error { return log.session.Flush(ctx) }

func TestSDKAndKernelRequireOnlyConversationLogNotRecordQueries(t *testing.T) {
	log := logOnly{session: core.NewMemorySession("log-only")}
	if _, rich := any(log).(abi.SessionStore); rich {
		t.Fatal("fixture unexpectedly implements rich query interface")
	}
	agent := build(t, core.NewBuilder(textModel("works"), log))
	if err := agent.Run(context.Background(), []wire.Message{wire.TextMessage(wire.RoleUser, "hello")}, discard); err != nil {
		t.Fatal(err)
	}
	messages, err := log.Messages(context.Background())
	if err != nil || len(messages) != 2 || messages[1].Text() != "works" {
		t.Fatalf("messages=%#v err=%v", messages, err)
	}
}
