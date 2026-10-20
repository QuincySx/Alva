// INPUT: context, encoding/json, errors, reflect, strings, testing, agent/core, kernel/abi, llm/wire
// OUTPUT: Single-hook dispatch/audit, actual invocation binding, registration order, nil rejection and cleanup regressions
// POS: [L2.5 SDK assembly] Function contributions use the ordinary runtime and never expose unrelated optional hooks.
package core_test

import (
	"context"
	"encoding/json"
	"errors"
	"reflect"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func implementedFunctionHooks(layer abi.Middleware) []string {
	var names []string
	if _, ok := layer.(abi.RunStartHook); ok {
		names = append(names, "start")
	}
	if _, ok := layer.(abi.InputCommittedHook); ok {
		names = append(names, "input")
	}
	if _, ok := layer.(abi.BeforeModelHook); ok {
		names = append(names, "before-model")
	}
	if _, ok := layer.(abi.ModelWrapper); ok {
		names = append(names, "wrap-model")
	}
	if _, ok := layer.(abi.ModelAdmissionHook); ok {
		names = append(names, "admit-model")
	}
	if _, ok := layer.(abi.AfterModelHook); ok {
		names = append(names, "after-model")
	}
	if _, ok := layer.(abi.PrepareToolHook); ok {
		names = append(names, "prepare-tool")
	}
	if _, ok := layer.(abi.BeforeToolHook); ok {
		names = append(names, "before-tool")
	}
	if _, ok := layer.(abi.ToolWrapper); ok {
		names = append(names, "wrap-tool")
	}
	if _, ok := layer.(abi.AfterToolHook); ok {
		names = append(names, "after-tool")
	}
	if _, ok := layer.(abi.RunEndHook); ok {
		names = append(names, "end")
	}
	return names
}

func TestFunctionHooksKeepSelectedDispatchAndActualAudit(t *testing.T) {
	calls := map[string]int{}
	observe := func(name string) { calls[name]++ }
	log := core.NewMemorySession("function-hooks")
	tool := &testTool{}
	layers := []abi.Middleware{
		core.RunStart("start", func(context.Context, abi.RuntimeContext) error { observe("start"); return nil }),
		core.InputCommitted("input", func(context.Context, abi.RuntimeContext, wire.Message) error { observe("input"); return nil }),
		core.BeforeModel("before-model", func(context.Context, abi.RuntimeContext, *abi.ModelRequest) error {
			observe("before-model")
			return nil
		}),
		core.WrapModel("wrap-model", func(ctx context.Context, r abi.RuntimeContext, request abi.ModelRequest, emit func(wire.StreamChunk) error, next abi.ModelFunc) error {
			observe("wrap-model")
			return next(ctx, r, request, emit)
		}),
		core.AdmitModel("admit-model", func(_ context.Context, r abi.RuntimeContext, _ abi.ModelRequest) error {
			observe("admit-model")
			if r.InvocationSessionID != log.ID() || r.InvocationModel == nil {
				t.Error("lost actual model/log binding")
			}
			return nil
		}),
		core.AfterModel("after-model", func(context.Context, abi.RuntimeContext, *wire.Message) error { observe("after-model"); return nil }),
		core.PrepareTool("prepare-tool", func(_ context.Context, p abi.ToolPreparationContext, _ wire.ToolCall) (abi.ToolPreparation, error) {
			observe("prepare-tool")
			if p.ActualTool != tool || p.Runtime.State.Session() != log {
				t.Error("lost actual preparation binding")
			}
			for _, record := range records(t, log) {
				if record.ToolIntent != nil {
					t.Error("preparation ran after intent")
				}
			}
			return abi.ToolPreparation{}, nil
		}),
		core.BeforeTool("before-tool", func(context.Context, abi.ToolExecutionContext, wire.ToolCall) error {
			observe("before-tool")
			return nil
		}),
		core.WrapTool("wrap-tool", func(ctx context.Context, e abi.ToolExecutionContext, call wire.ToolCall, next abi.ToolFunc) (wire.ToolResult, error) {
			observe("wrap-tool")
			if e.ActualTool != tool || e.Session != log {
				t.Error("lost actual execution binding")
			}
			return next(ctx, e, call)
		}),
		core.AfterTool("after-tool", func(context.Context, abi.ToolExecutionContext, wire.ToolCall, *wire.ToolResult) error {
			observe("after-tool")
			return nil
		}),
		core.RunEnd("end", func(context.Context, abi.RuntimeContext, error) error { observe("end"); return nil }),
	}
	p := plugin{name: "bundle", register: func(_ context.Context, r *core.Registrar) error {
		for _, layer := range layers {
			if got := implementedFunctionHooks(layer); !reflect.DeepEqual(got, []string{layer.Name()}) {
				t.Fatal("adapter exposes unrelated hook", layer.Name(), got)
			}
			if err := r.Middleware(layer); err != nil {
				return err
			}
		}
		return nil
	}}
	a := build(t, auditedBuilder(batchModel([]string{"call"}), log).Tool(tool).Plugin(p))
	if err := a.Run(t.Context(), []wire.Message{wire.TextMessage(wire.RoleUser, "one real model/tool/model")}, discard); err != nil {
		t.Fatal(err)
	}
	want := map[string]int{"start": 1, "input": 1, "before-model": 2, "wrap-model": 2, "admit-model": 2, "after-model": 2, "prepare-tool": 1, "before-tool": 1, "wrap-tool": 1, "after-tool": 1, "end": 1}
	if !reflect.DeepEqual(calls, want) {
		t.Fatal(calls, want)
	}
	stages := map[string]string{"start": "on_run_start", "input": "input_committed", "before-model": "before_model", "wrap-model": "wrap_model", "admit-model": "admit_model", "after-model": "after_model", "prepare-tool": "prepare_tool", "before-tool": "before_tool", "wrap-tool": "wrap_tool", "after-tool": "after_tool", "end": "on_run_end"}
	audited := map[string]int{}
	for _, record := range records(t, log) {
		if record.Namespace != "alva.kernel.audit" || record.Type != "middleware_hook_batch" {
			continue
		}
		var batch struct {
			Hook  string `json:"hook"`
			Calls []struct {
				Name string `json:"middleware_name"`
			} `json:"calls"`
		}
		if err := json.Unmarshal(record.Payload, &batch); err != nil {
			t.Fatal(err)
		}
		for _, call := range batch.Calls {
			if stages[call.Name] != batch.Hook {
				t.Fatal("invented unrelated audit stage", call.Name, batch.Hook)
			}
			audited[call.Name]++
		}
	}
	if !reflect.DeepEqual(audited, want) {
		t.Fatal("audit differs from actual calls", audited, want)
	}
	if len(a.Snapshot().Middleware) != len(layers) {
		t.Fatal(a.Snapshot())
	}
}

func nilFunctionHookCall(layer abi.Middleware) error {
	ctx := context.Background()
	switch h := layer.(type) {
	case abi.RunStartHook:
		return h.OnRunStart(ctx, abi.RuntimeContext{})
	case abi.RunEndHook:
		return h.OnRunEnd(ctx, abi.RuntimeContext{}, nil)
	case abi.InputCommittedHook:
		return h.InputCommitted(ctx, abi.RuntimeContext{}, wire.Message{})
	case abi.BeforeModelHook:
		return h.BeforeModel(ctx, abi.RuntimeContext{}, nil)
	case abi.AfterModelHook:
		return h.AfterModel(ctx, abi.RuntimeContext{}, nil)
	case abi.ModelWrapper:
		return h.WrapModel(ctx, abi.RuntimeContext{}, abi.ModelRequest{}, nil, nil)
	case abi.ModelAdmissionHook:
		return h.AdmitModel(ctx, abi.RuntimeContext{}, abi.ModelRequest{})
	case abi.PrepareToolHook:
		_, err := h.PrepareTool(ctx, abi.ToolPreparationContext{}, wire.ToolCall{})
		return err
	case abi.BeforeToolHook:
		return h.BeforeTool(ctx, abi.ToolExecutionContext{}, wire.ToolCall{})
	case abi.AfterToolHook:
		return h.AfterTool(ctx, abi.ToolExecutionContext{}, wire.ToolCall{}, nil)
	case abi.ToolWrapper:
		_, err := h.WrapTool(ctx, abi.ToolExecutionContext{}, wire.ToolCall{}, nil)
		return err
	}
	panic("fixture has no hook")
}

func TestFunctionHookNilRejectedBeforePluginsAndSafeForDirectInvocation(t *testing.T) {
	layers := []abi.Middleware{
		core.RunStart("start", nil), core.RunEnd("end", nil), core.InputCommitted("input", nil),
		core.BeforeModel("before", nil), core.AfterModel("after", nil), core.WrapModel("wrap-model", nil),
		core.AdmitModel("admit", nil), core.PrepareTool("prepare", nil), core.BeforeTool("before-tool", nil),
		core.AfterTool("after-tool", nil), core.WrapTool("wrap-tool", nil),
	}
	for _, layer := range layers {
		t.Run(layer.Name(), func(t *testing.T) {
			registered := 0
			p := plugin{name: "must-not-register", register: func(context.Context, *core.Registrar) error { registered++; return nil }}
			_, err := core.NewBuilder(textModel("unused"), core.NewMemorySession("nil-hook")).Middleware(layer).Plugin(p).Build(t.Context())
			if err == nil || registered != 0 {
				t.Fatal("nil function reached plugin registration", err, registered)
			}
			if err := nilFunctionHookCall(layer); err == nil || !strings.Contains(err.Error(), "nil") {
				t.Fatal("direct nil invocation did not return an error", err)
			}
		})
	}
}

func TestMiddlewareOrderIsAssemblyLocalAndStable(t *testing.T) {
	var trace []string
	before := func(name string) abi.Middleware {
		return core.BeforeModel(name, func(context.Context, abi.RuntimeContext, *abi.ModelRequest) error {
			trace = append(trace, name)
			return nil
		})
	}
	after := func(name string) abi.Middleware {
		return core.AfterModel(name, func(context.Context, abi.RuntimeContext, *wire.Message) error {
			trace = append(trace, name)
			return nil
		})
	}
	wrap := func(name string) abi.Middleware {
		return core.WrapModel(name, func(ctx context.Context, r abi.RuntimeContext, q abi.ModelRequest, emit func(wire.StreamChunk) error, next abi.ModelFunc) error {
			trace = append(trace, name+"-enter")
			err := next(ctx, r, q, emit)
			trace = append(trace, name+"-exit")
			return err
		})
	}
	p := plugin{name: "inner", register: func(_ context.Context, r *core.Registrar) error {
		for _, layer := range []abi.Middleware{before("inner-before"), wrap("inner-wrap"), after("inner-after")} {
			if err := r.Middleware(layer, 5000); err != nil {
				return err
			}
		}
		return nil
	}}
	order := []int{1000}
	b := core.NewBuilder(textModel("answer"), core.NewMemorySession("ordered")).
		Middleware(before("default-first")).
		Middleware(before("outer-before"), order...).
		Middleware(wrap("outer-wrap"), 1000).
		Middleware(after("outer-after"), 1000).
		Middleware(before("default-second")).
		Plugin(p)
	order[0] = 6000
	a := build(t, b)
	if err := a.Run(t.Context(), nil, discard); err != nil {
		t.Fatal(err)
	}
	want := []string{"outer-before", "default-first", "default-second", "inner-before", "outer-wrap-enter", "inner-wrap-enter", "inner-wrap-exit", "outer-wrap-exit", "inner-after", "outer-after"}
	if !reflect.DeepEqual(trace, want) {
		t.Fatal(trace, want)
	}
	if got := a.Snapshot().Middleware; !reflect.DeepEqual(got, []string{"outer-before", "outer-wrap", "outer-after", "default-first", "default-second", "inner-before", "inner-wrap", "inner-after"}) {
		t.Fatal(got)
	}
}

func TestMiddlewareRegistrationRejectsInvalidOrderAndDuplicateWithoutPartialContribution(t *testing.T) {
	layer := core.BeforeModel("same", func(context.Context, abi.RuntimeContext, *abi.ModelRequest) error { return nil })
	registered := 0
	p := plugin{name: "late", register: func(context.Context, *core.Registrar) error { registered++; return nil }}
	if _, err := core.NewBuilder(textModel("unused"), core.NewMemorySession("bad-order")).Middleware(layer, 1, 2).Plugin(p).Build(t.Context()); err == nil || registered != 0 {
		t.Fatal(err, registered)
	}
	var registrar *core.Registrar
	p = plugin{name: "registration", register: func(_ context.Context, r *core.Registrar) error {
		registrar = r
		if err := r.Middleware(layer, 1, 2); err == nil {
			t.Error("invalid order accepted")
		}
		if err := r.Middleware(core.BeforeModel("", func(context.Context, abi.RuntimeContext, *abi.ModelRequest) error { return nil })); err == nil {
			t.Error("empty adapter name accepted")
		}
		if err := r.Middleware(core.BeforeModel("bad-utf8-\xff", func(context.Context, abi.RuntimeContext, *abi.ModelRequest) error { return nil })); err == nil {
			t.Error("invalid adapter name accepted")
		}
		if err := r.Middleware(layer); err != nil {
			return err
		}
		if err := r.Middleware(core.AfterModel("same", func(context.Context, abi.RuntimeContext, *wire.Message) error { return nil })); err == nil {
			t.Error("duplicate accepted")
		}
		return nil
	}}
	a := build(t, core.NewBuilder(textModel("answer"), core.NewMemorySession("registration")).Plugin(p))
	if !reflect.DeepEqual(a.Snapshot().Middleware, []string{"same"}) {
		t.Fatal(a.Snapshot())
	}
	if err := registrar.Middleware(layer); err == nil {
		t.Fatal("sealed contribution accepted")
	}
}

func TestFunctionHookFailureStillRunsCleanupWithActualCause(t *testing.T) {
	failure := errors.New("startup rejected")
	cleanup := 0
	a := build(t, core.NewBuilder(textModel("unused"), core.NewMemorySession("cleanup")).
		Middleware(core.RunStart("start", func(context.Context, abi.RuntimeContext) error { return failure })).
		Middleware(core.RunEnd("end", func(ctx context.Context, _ abi.RuntimeContext, cause error) error {
			cleanup++
			if !errors.Is(cause, failure) || ctx.Err() != nil {
				t.Error("cleanup lost actual failure or live context", cause, ctx.Err())
			}
			return nil
		})))
	if err := a.Run(t.Context(), nil, discard); !errors.Is(err, failure) || cleanup != 1 {
		t.Fatal(err, cleanup)
	}
}
