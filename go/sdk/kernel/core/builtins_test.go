// INPUT: context, encoding/json, errors, strings, testing, time, kernel/abi, llm/wire
// OUTPUT: Timeout, recoverable veto, loop-state isolation and cancellation regressions
// POS: [L2 kernel runtime] Exercises generic hygiene without detaching execution or hiding real failures.
package core

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
	"strings"
	"testing"
	"time"
)

func TestTimeoutWaitsForActualReturnAndRetainsError(t *testing.T) {
	fixture := errors.New("underlying failure")
	returned := false
	middleware := ToolTimeoutMiddleware{Timeout: time.Millisecond}
	next := func(ctx context.Context, _ abi.ToolExecutionContext, _ wire.ToolCall) (wire.ToolResult, error) {
		<-ctx.Done()
		time.Sleep(time.Millisecond)
		returned = true
		return wire.ToolResult{}, fixture
	}
	_, err := middleware.WrapTool(context.Background(), abi.ToolExecutionContext{}, wire.ToolCall{}, next)
	if !returned || !errors.Is(err, ErrToolTimeout) || !errors.Is(err, fixture) || !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal(err, returned)
	}
	called := false
	_, err = middleware.WrapTool(context.Background(), abi.ToolExecutionContext{Scheduling: abi.Scheduling{ManagesOwnTimeout: true}}, wire.ToolCall{}, func(ctx context.Context, _ abi.ToolExecutionContext, _ wire.ToolCall) (wire.ToolResult, error) {
		called = true
		if _, ok := ctx.Deadline(); ok {
			t.Fatal("own timeout replaced")
		}
		return wire.ToolResult{}, nil
	})
	if !called || err != nil {
		t.Fatal(err)
	}
}
func TestLoopDetectionSharedInstanceIsAgentLocalAndResets(t *testing.T) {
	middleware, err := NewLoopDetectionMiddleware(1, 2, 4)
	if err != nil {
		t.Fatal(err)
	}
	first := abi.RuntimeContext{State: testState(t, "one")}
	second := abi.RuntimeContext{State: testState(t, "two")}
	message := func(id string) wire.Message {
		return wire.Message{Role: wire.RoleAssistant, Content: []wire.ContentBlock{{Kind: wire.ContentToolCall, ToolCall: &wire.ToolCall{ID: id, Name: "test", Arguments: json.RawMessage(`{"n":1}`)}}}}
	}
	for _, runtime := range []abi.RuntimeContext{first, second} {
		if err := middleware.OnRunStart(context.Background(), runtime); err != nil {
			t.Fatal(err)
		}
		value := message("1")
		if err := middleware.AfterModel(context.Background(), runtime, &value); err != nil {
			t.Fatal(err)
		}
		if value.Content[0].ToolCall == nil {
			t.Fatal("cross-Agent loop history")
		}
	}
	value := message("changed-id")
	_ = middleware.AfterModel(context.Background(), first, &value)
	if !strings.Contains(value.Text(), "Loop detected") {
		t.Fatal(value)
	}
	_ = middleware.OnRunEnd(context.Background(), first, nil)
	value = message("next-run")
	_ = middleware.AfterModel(context.Background(), first, &value)
	if value.Content[0].ToolCall == nil {
		t.Fatal("run history leaked")
	}
}

type fixtureModel struct{}

func (fixtureModel) Stream(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error {
	return nil
}

type fixtureLog string

func (log fixtureLog) ID() string                                   { return string(log) }
func (fixtureLog) Messages(context.Context) ([]wire.Message, error) { return nil, nil }
func (fixtureLog) AppendRecords(context.Context, []abi.SessionRecord) ([]abi.SessionRecord, error) {
	return nil, nil
}
func (fixtureLog) Flush(context.Context) error { return nil }
func testState(t *testing.T, id string) *abi.AgentState {
	t.Helper()
	state, err := abi.NewAgentState(fixtureModel{}, fixtureLog(id), nil)
	if err != nil {
		t.Fatal(err)
	}
	return state
}
func TestCanceledModelStreamDoesNotInventMissingFinish(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	_, _, _, err := collect(ctx, canceledFixtureModel{}, abi.ModelRequest{}, 1024, func(abi.Event) error { return nil })
	if !errors.Is(err, context.Canceled) || strings.Contains(err.Error(), "without finish") {
		t.Fatal(err)
	}
}

type canceledFixtureModel struct{}

func (canceledFixtureModel) Stream(ctx context.Context, _ abi.ModelRequest, _ func(wire.StreamChunk) error) error {
	return ctx.Err()
}

type beforeToolFixture struct{ err error }

func (beforeToolFixture) Name() string { return "fixture-before-tool" }
func (hook beforeToolFixture) BeforeTool(context.Context, abi.ToolExecutionContext, wire.ToolCall) error {
	return hook.err
}
func TestOnlyExplicitBeforeToolVetoAndUnknownToolAreRecoverable(t *testing.T) {
	call := wire.ToolCall{ID: "call", Name: "missing", Arguments: json.RawMessage(`{}`)}
	fixture := errors.New("infrastructure failure")
	for _, test := range []struct {
		hookError   error
		recoverable bool
	}{{&abi.ToolBlockedError{Reason: "intentional policy veto"}, true}, {fixture, false}} {
		result, err := executeTool(context.Background(), abi.ToolExecutionContext{}, nil, call, []abi.Middleware{beforeToolFixture{test.hookError}}, nil)
		if test.recoverable {
			if err != nil || !result.IsError || result.CallID != call.ID {
				t.Fatal(result, err)
			}
		} else if !errors.Is(err, fixture) {
			t.Fatal("infrastructure error converted to business feedback", err)
		}
	}
	result, err := executeTool(context.Background(), abi.ToolExecutionContext{}, nil, call, nil, nil)
	if err != nil || !result.IsError || result.CallID != call.ID {
		t.Fatal("unknown tool did not give safe model feedback", result, err)
	}
}
