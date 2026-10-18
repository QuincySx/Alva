// INPUT: context, errors, fmt, reflect, sync, kernel/abi, llm/wire
// OUTPUT: Frozen post-intent tool hook and wrapper execution
// POS: [L2 kernel runtime] Prevents middleware argument changes after scheduling/admission and durable intent.
package core

import (
	"context"
	"errors"
	"fmt"
	"reflect"
	"sync"
	"time"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func cloneExecution(execution abi.ToolExecutionContext) abi.ToolExecutionContext {
	execution.Scheduling.Resources = append([]abi.ResourceClaim(nil), execution.Scheduling.Resources...)
	return execution
}

func executeTool(ctx context.Context, execution abi.ToolExecutionContext, tool abi.Tool, call wire.ToolCall, middleware []abi.Middleware, audit *toolHookAudit) (wire.ToolResult, error) {
	if audit == nil {
		audit = newToolHookAudit(execution.RunID, call.ID)
	}
	frozen := wire.CloneToolCall(call)
	var baseMu sync.Mutex
	baseUsed := false
	var baseViolation error
	for i, layer := range middleware {
		if hook, ok := layer.(abi.BeforeToolHook); ok {
			owned := wire.CloneToolCall(frozen)
			started := time.Now()
			err := hook.BeforeTool(ctx, cloneExecution(execution), owned)
			if !reflect.DeepEqual(owned, frozen) {
				err = errors.Join(err, errors.New("BeforeTool mutated committed arguments; use PrepareTool"))
			}
			audit.before.observe(layer, i, started, err)
			if err != nil {
				if onlyToolBlocked(err) {
					result := wire.ToolResult{CallID: frozen.ID, IsError: true, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: err.Error()}}}
					return afterTool(ctx, execution, frozen, result, middleware, audit)
				}
				return wire.ToolResult{}, err
			}
		}
	}
	var next abi.ToolFunc = func(ctx context.Context, execution abi.ToolExecutionContext, owned wire.ToolCall) (wire.ToolResult, error) {
		baseMu.Lock()
		if baseUsed {
			baseViolation = errors.New("tool next may only be called once")
			err := baseViolation
			baseMu.Unlock()
			return wire.ToolResult{}, err
		}
		baseUsed = true
		baseMu.Unlock()
		if !reflect.DeepEqual(owned, frozen) {
			return wire.ToolResult{}, errors.New("tool next changed committed call; use PrepareTool")
		}
		if tool == nil {
			return wire.ToolResult{CallID: frozen.ID, IsError: true, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: fmt.Sprintf("unknown tool %q", frozen.Name)}}}, nil
		}
		return tool.Execute(ctx, cloneExecution(execution), wire.CloneToolCall(frozen))
	}
	for i := len(middleware) - 1; i >= 0; i-- {
		if wrapper, ok := middleware[i].(abi.ToolWrapper); ok {
			inner := next
			layer, index := middleware[i], i
			next = func(ctx context.Context, execution abi.ToolExecutionContext, supplied wire.ToolCall) (wire.ToolResult, error) {
				if !reflect.DeepEqual(supplied, frozen) {
					return wire.ToolResult{}, errors.New("tool wrapper changed committed call; use PrepareTool")
				}
				owned := wire.CloneToolCall(frozen)
				var leaseMu sync.Mutex
				leaseChanged := sync.NewCond(&leaseMu)
				open, used, active := true, false, 0
				var violation error
				var failures []error
				guardedNext := func(ctx context.Context, candidate abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
					leaseMu.Lock()
					if !open {
						leaseMu.Unlock()
						return wire.ToolResult{}, errors.New("tool next called after wrapper returned")
					}
					if used {
						violation = errors.Join(violation, errors.New("tool next may only be called once"))
						err := violation
						leaseMu.Unlock()
						return wire.ToolResult{}, err
					}
					used = true
					if !reflect.DeepEqual(call, frozen) {
						violation = errors.Join(violation, errors.New("tool wrapper next changed committed call; use PrepareTool"))
						err := violation
						leaseMu.Unlock()
						return wire.ToolResult{}, err
					}
					active++
					leaseMu.Unlock()
					// Keep runtime-owned authority binding. Wrappers may redirect
					// progress, but cannot replace the actual tool/session/schedule
					// that security and lock middleware are asked to authorize.
					canonical := cloneExecution(execution)
					canonical.Progress = candidate.Progress
					result, err := inner(ctx, canonical, wire.CloneToolCall(frozen))
					leaseMu.Lock()
					if err != nil {
						failures = append(failures, err)
					}
					active--
					leaseChanged.Broadcast()
					leaseMu.Unlock()
					return result, err
				}
				started := time.Now()
				result, err := wrapper.WrapTool(ctx, cloneExecution(execution), owned, guardedNext)
				leaseMu.Lock()
				open = false
				for active > 0 {
					leaseChanged.Wait()
				}
				if !reflect.DeepEqual(owned, frozen) {
					violation = errors.Join(violation, errors.New("tool wrapper mutated committed call; use PrepareTool"))
				}
				for _, failure := range failures {
					// Avoid exponentially repeating an inner error already
					// returned/wrapped by an ordinary onion middleware.
					if !errors.Is(err, failure) {
						err = errors.Join(err, failure)
					}
				}
				leaseMu.Unlock()
				err = errors.Join(err, violation)
				audit.wrap.observe(layer, index, started, err)
				return result, err
			}
		}
	}
	result, err := next(ctx, cloneExecution(execution), wire.CloneToolCall(frozen))
	baseMu.Lock()
	err = errors.Join(err, baseViolation)
	baseMu.Unlock()
	if err != nil {
		return result, err
	}
	return afterTool(ctx, execution, frozen, result, middleware, audit)
}

func afterTool(ctx context.Context, execution abi.ToolExecutionContext, frozen wire.ToolCall, result wire.ToolResult, middleware []abi.Middleware, audit *toolHookAudit) (wire.ToolResult, error) {
	for i := len(middleware) - 1; i >= 0; i-- {
		if hook, ok := middleware[i].(abi.AfterToolHook); ok {
			owned := wire.CloneToolCall(frozen)
			started := time.Now()
			err := hook.AfterTool(ctx, cloneExecution(execution), owned, &result)
			if !reflect.DeepEqual(owned, frozen) {
				err = errors.Join(err, errors.New("AfterTool mutated committed call"))
			}
			audit.after.observe(middleware[i], i, started, err)
			if err != nil {
				return result, err
			}
		}
	}
	return result, nil
}

// A mixed error tree is not an intentional model-recoverable veto. In
// particular Join(veto, SessionError/deadline) must retain the real failure.
func onlyToolBlocked(err error) bool {
	if err == nil {
		return false
	}
	if _, ok := err.(*abi.ToolBlockedError); ok {
		return true
	}
	if _, ok := err.(*abi.SessionError); ok {
		return false
	}
	if joined, ok := err.(interface{ Unwrap() []error }); ok {
		children := joined.Unwrap()
		if len(children) == 0 {
			return false
		}
		for _, child := range children {
			if !onlyToolBlocked(child) {
				return false
			}
		}
		return true
	}
	if wrapped, ok := err.(interface{ Unwrap() error }); ok {
		return onlyToolBlocked(wrapped.Unwrap())
	}
	return false
}
