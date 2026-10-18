// INPUT: context, errors, sync/atomic, testing, time, kernel/abi, llm/wire
// OUTPUT: Unswallowable failures, wrapper callback lifetime and pure-veto after-hook regressions
// POS: [L2.5 SDK assembly] Tool middleware cannot hide infrastructure failures or execute after durable settlement.
package core_test

import (
	"context"
	"errors"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type wrapperFixture struct {
	name string
	fn   func(context.Context, abi.ToolExecutionContext, wire.ToolCall, abi.ToolFunc) (wire.ToolResult, error)
}

func (wrapper wrapperFixture) Name() string { return wrapper.name }
func (wrapper wrapperFixture) WrapTool(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall, next abi.ToolFunc) (wire.ToolResult, error) {
	return wrapper.fn(ctx, execution, call, next)
}

type vetoFixture struct {
	err   error
	after func(*wire.ToolResult) error
}

func (vetoFixture) Name() string { return "veto" }
func (hook vetoFixture) BeforeTool(context.Context, abi.ToolExecutionContext, wire.ToolCall) error {
	return hook.err
}
func (hook vetoFixture) AfterTool(_ context.Context, _ abi.ToolExecutionContext, _ wire.ToolCall, result *wire.ToolResult) error {
	if hook.after != nil {
		return hook.after(result)
	}
	return nil
}

func TestJoinedVetoDoesNotSwallowSessionFailureOrCancellation(t *testing.T) {
	disk := errors.New("disk failed")
	for _, cause := range []error{&abi.SessionError{Operation: "flush", SessionID: "s", Err: disk}, context.Canceled} {
		session := core.NewMemorySession("joined-veto")
		var executions atomic.Int32
		tool := &testTool{execute: func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
			executions.Add(1)
			return wire.ToolResult{}, nil
		}}
		hook := vetoFixture{err: errors.Join(&abi.ToolBlockedError{Reason: "policy blocked"}, cause)}
		err := build(t, core.NewBuilder(batchModel([]string{"one"}), session).Tool(tool).Middleware(hook)).Run(context.Background(), nil, discard)
		if !errors.Is(err, cause) || executions.Load() != 0 {
			t.Fatal("mixed veto swallowed failure", err)
		}
		for _, record := range records(t, session) {
			if record.ToolOutcome != nil && record.ToolOutcome.Error == "" {
				t.Fatal("failure evidence missing")
			}
		}
	}
}

func TestPureVetoRunsAfterToolAndItsFailureRemainsFatal(t *testing.T) {
	for _, fail := range []bool{false, true} {
		var afterCalls atomic.Int32
		failure := errors.New("after veto failed")
		hook := vetoFixture{err: &abi.ToolBlockedError{Reason: "policy blocked"}, after: func(result *wire.ToolResult) error {
			afterCalls.Add(1)
			if !result.IsError || result.Content[0].Text != "policy blocked" {
				t.Error("after hook did not see veto result")
			}
			if fail {
				return failure
			}
			return nil
		}}
		session := core.NewMemorySession("veto")
		err := build(t, core.NewBuilder(batchModel([]string{"one"}), session).Tool(&testTool{}).Middleware(hook)).Run(context.Background(), nil, discard)
		if afterCalls.Load() != 1 || fail != errors.Is(err, failure) || (!fail && err != nil) {
			t.Fatal("veto after-hook semantics", afterCalls.Load(), err)
		}
	}
}

func TestWrapperCannotSwallowApprovalOrTypedSessionFailure(t *testing.T) {
	for _, failure := range []error{errors.New("approval denied"), &abi.SessionError{Operation: "append_records", SessionID: "s", Err: context.DeadlineExceeded}} {
		var executions atomic.Int32
		outer := wrapperFixture{"swallow", func(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall, next abi.ToolFunc) (wire.ToolResult, error) {
			_, _ = next(ctx, execution, call)
			return wire.ToolResult{CallID: call.ID}, nil
		}}
		inner := wrapperFixture{"deny", func(context.Context, abi.ToolExecutionContext, wire.ToolCall, abi.ToolFunc) (wire.ToolResult, error) {
			return wire.ToolResult{}, failure
		}}
		tool := &testTool{execute: func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
			executions.Add(1)
			return wire.ToolResult{}, nil
		}}
		err := build(t, core.NewBuilder(batchModel([]string{"one"}), core.NewMemorySession("swallow")).Tool(tool).Middleware(outer).Middleware(inner)).Run(context.Background(), nil, discard)
		if !errors.Is(err, failure) || executions.Load() != 0 || abi.OnlyCancellation(err) {
			t.Fatal("wrapper swallowed true error", err)
		}
	}
}

func TestWrapperSavedNextCannotExecuteAfterReturn(t *testing.T) {
	var saved abi.ToolFunc
	var execution abi.ToolExecutionContext
	var call wire.ToolCall
	var executions atomic.Int32
	wrapper := wrapperFixture{"save", func(_ context.Context, current abi.ToolExecutionContext, input wire.ToolCall, next abi.ToolFunc) (wire.ToolResult, error) {
		saved = next
		execution = current
		call = wire.CloneToolCall(input)
		return wire.ToolResult{CallID: input.ID}, nil
	}}
	tool := &testTool{execute: func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
		executions.Add(1)
		return wire.ToolResult{}, nil
	}}
	agent := build(t, core.NewBuilder(batchModel([]string{"one"}), core.NewMemorySession("late")).Tool(tool).Middleware(wrapper))
	if err := agent.Run(context.Background(), nil, discard); err != nil {
		t.Fatal(err)
	}
	if saved == nil {
		t.Fatal("fixture did not save next")
	}
	if _, err := saved(context.Background(), execution, call); err == nil || executions.Load() != 0 {
		t.Fatal("late callback executed after settlement", err)
	}
}

func TestWrapperWaitsForEnteredNextAndRetainsItsFailure(t *testing.T) {
	entered, release, wrapperReturned := make(chan struct{}), make(chan struct{}), make(chan struct{})
	failure := errors.New("real execution failed after wrapper return")
	wrapper := wrapperFixture{"async", func(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall, next abi.ToolFunc) (wire.ToolResult, error) {
		go func() { _, _ = next(ctx, execution, wire.CloneToolCall(call)) }()
		<-entered
		close(wrapperReturned)
		return wire.ToolResult{CallID: call.ID}, nil
	}}
	tool := &testTool{execute: func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
		close(entered)
		<-release
		return wire.ToolResult{}, failure
	}}
	agent := build(t, core.NewBuilder(batchModel([]string{"one"}), core.NewMemorySession("active-next")).Tool(tool).Middleware(wrapper))
	done := make(chan error, 1)
	go func() { done <- agent.Run(context.Background(), nil, discard) }()
	<-wrapperReturned
	select {
	case err := <-done:
		t.Fatal("run settled before active next exited", err)
	case <-time.After(20 * time.Millisecond):
	}
	close(release)
	select {
	case err := <-done:
		if !errors.Is(err, failure) {
			t.Fatal("active next failure swallowed", err)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("wrapper lease did not finish")
	}
}
