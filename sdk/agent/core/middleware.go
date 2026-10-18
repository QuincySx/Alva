// INPUT: context, errors, unicode/utf8, kernel/abi, llm/wire
// OUTPUT: RunStart, RunEnd, InputCommitted, BeforeModel, AfterModel, WrapModel, AdmitModel, PrepareTool, BeforeTool, WrapTool, AfterTool
// POS: [L2.5 SDK assembly] Single-function adapters contribute exactly one typed hook without a parallel phase execution model.
package core

import (
	"context"
	"errors"
	"unicode/utf8"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type middlewareRegistration struct {
	middleware abi.Middleware
	orders     []int
}

type middlewareContribution struct {
	middleware abi.Middleware
	order      int
}

type hookIdentity struct {
	name  string
	valid bool
}

func (h hookIdentity) Name() string { return h.name }
func (h hookIdentity) validateHook() error {
	if h.name == "" || !utf8.ValidString(h.name) {
		return errors.New("middleware hook identity missing or invalid")
	}
	if !h.valid {
		return errors.New("middleware hook function is nil")
	}
	return nil
}

// RunStart adapts one function. No other optional hook is implemented.
// A nil function is rejected during registration; direct invocation returns an error.
func RunStart(name string, fn func(context.Context, abi.RuntimeContext) error) abi.Middleware {
	return runStartHook{hookIdentity: hookIdentity{name: name, valid: fn != nil}, fn: fn}
}

type runStartHook struct {
	hookIdentity
	fn func(context.Context, abi.RuntimeContext) error
}

func (h runStartHook) OnRunStart(ctx context.Context, runtime abi.RuntimeContext) error {
	if err := h.validateHook(); err != nil {
		return err
	}
	return h.fn(ctx, runtime)
}

// RunEnd adapts one function. No other optional hook is implemented.
// A nil function is rejected during registration; direct invocation returns an error.
func RunEnd(name string, fn func(context.Context, abi.RuntimeContext, error) error) abi.Middleware {
	return runEndHook{hookIdentity: hookIdentity{name: name, valid: fn != nil}, fn: fn}
}

type runEndHook struct {
	hookIdentity
	fn func(context.Context, abi.RuntimeContext, error) error
}

func (h runEndHook) OnRunEnd(ctx context.Context, runtime abi.RuntimeContext, cause error) error {
	if err := h.validateHook(); err != nil {
		return err
	}
	return h.fn(ctx, runtime, cause)
}

// InputCommitted adapts one function. No other optional hook is implemented.
// A nil function is rejected during registration; direct invocation returns an error.
func InputCommitted(name string, fn func(context.Context, abi.RuntimeContext, wire.Message) error) abi.Middleware {
	return inputCommittedHook{hookIdentity: hookIdentity{name: name, valid: fn != nil}, fn: fn}
}

type inputCommittedHook struct {
	hookIdentity
	fn func(context.Context, abi.RuntimeContext, wire.Message) error
}

func (h inputCommittedHook) InputCommitted(ctx context.Context, runtime abi.RuntimeContext, message wire.Message) error {
	if err := h.validateHook(); err != nil {
		return err
	}
	return h.fn(ctx, runtime, message)
}

// BeforeModel adapts one function. No other optional hook is implemented.
// A nil function is rejected during registration; direct invocation returns an error.
func BeforeModel(name string, fn func(context.Context, abi.RuntimeContext, *abi.ModelRequest) error) abi.Middleware {
	return beforeModelHook{hookIdentity: hookIdentity{name: name, valid: fn != nil}, fn: fn}
}

type beforeModelHook struct {
	hookIdentity
	fn func(context.Context, abi.RuntimeContext, *abi.ModelRequest) error
}

func (h beforeModelHook) BeforeModel(ctx context.Context, runtime abi.RuntimeContext, request *abi.ModelRequest) error {
	if err := h.validateHook(); err != nil {
		return err
	}
	return h.fn(ctx, runtime, request)
}

// AfterModel adapts one function. No other optional hook is implemented.
// A nil function is rejected during registration; direct invocation returns an error.
func AfterModel(name string, fn func(context.Context, abi.RuntimeContext, *wire.Message) error) abi.Middleware {
	return afterModelHook{hookIdentity: hookIdentity{name: name, valid: fn != nil}, fn: fn}
}

type afterModelHook struct {
	hookIdentity
	fn func(context.Context, abi.RuntimeContext, *wire.Message) error
}

func (h afterModelHook) AfterModel(ctx context.Context, runtime abi.RuntimeContext, response *wire.Message) error {
	if err := h.validateHook(); err != nil {
		return err
	}
	return h.fn(ctx, runtime, response)
}

// WrapModel adapts one function. No other optional hook is implemented.
// A nil function is rejected during registration; direct invocation returns an error.
func WrapModel(name string, fn func(context.Context, abi.RuntimeContext, abi.ModelRequest, func(wire.StreamChunk) error, abi.ModelFunc) error) abi.Middleware {
	return wrapModelHook{hookIdentity: hookIdentity{name: name, valid: fn != nil}, fn: fn}
}

type wrapModelHook struct {
	hookIdentity
	fn func(context.Context, abi.RuntimeContext, abi.ModelRequest, func(wire.StreamChunk) error, abi.ModelFunc) error
}

func (h wrapModelHook) WrapModel(ctx context.Context, runtime abi.RuntimeContext, request abi.ModelRequest, emit func(wire.StreamChunk) error, next abi.ModelFunc) error {
	if err := h.validateHook(); err != nil {
		return err
	}
	return h.fn(ctx, runtime, request, emit, next)
}

// AdmitModel adapts one function. No other optional hook is implemented.
// A nil function is rejected during registration; direct invocation returns an error.
func AdmitModel(name string, fn func(context.Context, abi.RuntimeContext, abi.ModelRequest) error) abi.Middleware {
	return admitModelHook{hookIdentity: hookIdentity{name: name, valid: fn != nil}, fn: fn}
}

type admitModelHook struct {
	hookIdentity
	fn func(context.Context, abi.RuntimeContext, abi.ModelRequest) error
}

func (h admitModelHook) AdmitModel(ctx context.Context, runtime abi.RuntimeContext, request abi.ModelRequest) error {
	if err := h.validateHook(); err != nil {
		return err
	}
	return h.fn(ctx, runtime, request)
}

// PrepareTool adapts one function. No other optional hook is implemented.
// A nil function is rejected during registration; direct invocation returns an error.
func PrepareTool(name string, fn func(context.Context, abi.ToolPreparationContext, wire.ToolCall) (abi.ToolPreparation, error)) abi.Middleware {
	return prepareToolHook{hookIdentity: hookIdentity{name: name, valid: fn != nil}, fn: fn}
}

type prepareToolHook struct {
	hookIdentity
	fn func(context.Context, abi.ToolPreparationContext, wire.ToolCall) (abi.ToolPreparation, error)
}

func (h prepareToolHook) PrepareTool(ctx context.Context, preparation abi.ToolPreparationContext, call wire.ToolCall) (abi.ToolPreparation, error) {
	if err := h.validateHook(); err != nil {
		return abi.ToolPreparation{}, err
	}
	return h.fn(ctx, preparation, call)
}

// BeforeTool adapts one function. No other optional hook is implemented.
// A nil function is rejected during registration; direct invocation returns an error.
func BeforeTool(name string, fn func(context.Context, abi.ToolExecutionContext, wire.ToolCall) error) abi.Middleware {
	return beforeToolHook{hookIdentity: hookIdentity{name: name, valid: fn != nil}, fn: fn}
}

type beforeToolHook struct {
	hookIdentity
	fn func(context.Context, abi.ToolExecutionContext, wire.ToolCall) error
}

func (h beforeToolHook) BeforeTool(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall) error {
	if err := h.validateHook(); err != nil {
		return err
	}
	return h.fn(ctx, execution, call)
}

// WrapTool adapts one function. No other optional hook is implemented.
// A nil function is rejected during registration; direct invocation returns an error.
func WrapTool(name string, fn func(context.Context, abi.ToolExecutionContext, wire.ToolCall, abi.ToolFunc) (wire.ToolResult, error)) abi.Middleware {
	return wrapToolHook{hookIdentity: hookIdentity{name: name, valid: fn != nil}, fn: fn}
}

type wrapToolHook struct {
	hookIdentity
	fn func(context.Context, abi.ToolExecutionContext, wire.ToolCall, abi.ToolFunc) (wire.ToolResult, error)
}

func (h wrapToolHook) WrapTool(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall, next abi.ToolFunc) (wire.ToolResult, error) {
	if err := h.validateHook(); err != nil {
		return wire.ToolResult{}, err
	}
	return h.fn(ctx, execution, call, next)
}

// AfterTool adapts one function. No other optional hook is implemented.
// A nil function is rejected during registration; direct invocation returns an error.
func AfterTool(name string, fn func(context.Context, abi.ToolExecutionContext, wire.ToolCall, *wire.ToolResult) error) abi.Middleware {
	return afterToolHook{hookIdentity: hookIdentity{name: name, valid: fn != nil}, fn: fn}
}

type afterToolHook struct {
	hookIdentity
	fn func(context.Context, abi.ToolExecutionContext, wire.ToolCall, *wire.ToolResult) error
}

func (h afterToolHook) AfterTool(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall, result *wire.ToolResult) error {
	if err := h.validateHook(); err != nil {
		return err
	}
	return h.fn(ctx, execution, call, result)
}
