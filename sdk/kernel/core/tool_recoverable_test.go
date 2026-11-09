// INPUT: context, errors, testing, kernel/abi, llm/wire
// OUTPUT: Regression tests for model-recoverable wrapper and tool errors
// POS: [L2 kernel runtime] Proves only explicitly typed vetoes and failures become tool results; everything else aborts.
package core

import (
	"context"
	"encoding/json"
	"errors"
	"testing"
	"time"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type recoverableTool struct{ err error }

func (recoverableTool) Name() string        { return "probe" }
func (recoverableTool) Description() string { return "probe" }
func (recoverableTool) ParametersSchema() json.RawMessage {
	return json.RawMessage(`{"type":"object"}`)
}
func (t recoverableTool) Execute(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
	return wire.ToolResult{}, t.err
}
func (recoverableTool) Scheduling(wire.ToolCall) abi.Scheduling { return abi.Scheduling{} }
func (recoverableTool) DescribeEffect(wire.ToolCall) abi.Effect { return abi.Effect{} }
func (recoverableTool) IsReadOnly(wire.ToolCall) bool           { return true }

type wrapFixture struct{ err error }

func (wrapFixture) Name() string { return "wrap-fixture" }
func (w wrapFixture) WrapTool(context.Context, abi.ToolExecutionContext, wire.ToolCall, abi.ToolFunc) (wire.ToolResult, error) {
	return wire.ToolResult{}, w.err
}

func TestWrapperVetoAndToolFailureBecomeModelResults(t *testing.T) {
	call := wire.ToolCall{ID: "c", Name: "probe", Arguments: json.RawMessage(`{}`)}
	cause := errors.New("cause")
	for name, run := range map[string]func() (wire.ToolResult, error){
		"wrapper veto": func() (wire.ToolResult, error) {
			return executeTool(context.Background(), abi.ToolExecutionContext{}, recoverableTool{}, call, []abi.Middleware{wrapFixture{&abi.ToolBlockedError{Reason: "declined", Cause: cause}}}, nil)
		},
		"tool failure": func() (wire.ToolResult, error) {
			return executeTool(context.Background(), abi.ToolExecutionContext{}, recoverableTool{&abi.ToolFailedError{Reason: "declined"}}, call, nil, nil)
		},
	} {
		result, err := run()
		if err != nil || !result.IsError || result.CallID != "c" || len(result.Content) != 1 || result.Content[0].Text != "declined" {
			t.Fatalf("%s: %+v %v", name, result, err)
		}
	}
}

func TestUntypedMixedAndCanceledErrorsStillAbort(t *testing.T) {
	call := wire.ToolCall{ID: "c", Name: "probe", Arguments: json.RawMessage(`{}`)}
	internal := errors.New("store write failed")
	canceled, cancel := context.WithCancel(context.Background())
	cancel()
	for name, err := range map[string]error{
		"untyped":     internal,
		"mixed":       errors.Join(&abi.ToolFailedError{Reason: "x"}, internal),
		"cancel":      errors.Join(&abi.ToolBlockedError{Reason: "x"}, context.Canceled),
		"session":     errors.Join(&abi.ToolBlockedError{Reason: "x"}, &abi.SessionError{Operation: "append", Err: internal}),
		"wrapped-cap": &abi.SessionError{Operation: "append", Err: &abi.ToolFailedError{Reason: "x"}},
	} {
		for _, ctx := range []context.Context{context.Background(), canceled} {
			_, got := executeTool(ctx, abi.ToolExecutionContext{}, recoverableTool{}, call, []abi.Middleware{wrapFixture{err}}, nil)
			if got == nil {
				t.Fatalf("%s converted to a result", name)
			}
		}
	}
}

func TestAuditSeparatesToolFailureFromPolicyVeto(t *testing.T) {
	for name, test := range map[string]struct {
		err  error
		want string
	}{
		"veto":    {&abi.ToolBlockedError{Reason: "x"}, "intentional_tool_veto"},
		"failure": {&abi.ToolFailedError{Reason: "x"}, "tool_failure"},
		"mixed":   {errors.Join(&abi.ToolFailedError{Reason: "x"}, &abi.ToolBlockedError{Reason: "y"}), "intentional_tool_veto"},
		"fault":   {errors.New("x"), "hook_failure"},
	} {
		batch := newHookBatch("run", "BeforeTool", "call")
		batch.observe(wrapFixture{}, 0, time.Now(), test.err)
		trace, ok := batch.trace()
		if !ok || trace.Calls[0].ErrorSummary != test.want {
			t.Errorf("%s: %+v", name, trace)
		}
	}
}
