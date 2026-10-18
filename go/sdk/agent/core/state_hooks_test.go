// INPUT: context, errors, reflect, testing, kernel/abi, llm/wire
// OUTPUT: Mutable runtime state and complete hook regressions
// POS: [L2.5 SDK assembly] Proves hooks mutate real per-Agent model/tool/log state rather than detached snapshots.
package core_test

import (
	"context"
	"errors"
	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
	"reflect"
	"testing"
)

type runtimeHooks struct {
	start   func(abi.RuntimeContext) error
	input   func(abi.RuntimeContext, wire.Message) error
	before  func(abi.RuntimeContext, *abi.ModelRequest) error
	end     func(abi.RuntimeContext, error) error
	wrapper func(context.Context, abi.RuntimeContext, abi.ModelRequest, func(wire.StreamChunk) error, abi.ModelFunc) error
}

func (*runtimeHooks) Name() string { return "runtime-hooks" }
func (hook *runtimeHooks) OnRunStart(_ context.Context, runtime abi.RuntimeContext) error {
	if hook.start != nil {
		return hook.start(runtime)
	}
	return nil
}
func (hook *runtimeHooks) InputCommitted(_ context.Context, runtime abi.RuntimeContext, message wire.Message) error {
	if hook.input != nil {
		return hook.input(runtime, message)
	}
	return nil
}
func (hook *runtimeHooks) BeforeModel(_ context.Context, runtime abi.RuntimeContext, request *abi.ModelRequest) error {
	if hook.before != nil {
		return hook.before(runtime, request)
	}
	return nil
}
func (hook *runtimeHooks) OnRunEnd(_ context.Context, runtime abi.RuntimeContext, err error) error {
	if hook.end != nil {
		return hook.end(runtime, err)
	}
	return nil
}
func (hook *runtimeHooks) WrapModel(ctx context.Context, runtime abi.RuntimeContext, request abi.ModelRequest, emit func(wire.StreamChunk) error, next abi.ModelFunc) error {
	if hook.wrapper != nil {
		return hook.wrapper(ctx, runtime, request, emit, next)
	}
	return next(ctx, runtime, request, emit)
}

func TestRuntimeStateMutatesActualModelToolsAndLog(t *testing.T) {
	oldLog, newLog := core.NewMemorySession("old"), core.NewMemorySession("new")
	var order []string
	var replacement abi.LanguageModel = modelFunc(func(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		order = append(order, "model")
		if len(request.Tools) != 1 || request.Tools[0].Name != "test" {
			t.Fatal("live tool registration not reflected")
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	hook := &runtimeHooks{start: func(runtime abi.RuntimeContext) error {
		order = append(order, "start")
		if runtime.RunID == "" {
			t.Fatal("missing run identity")
		}
		if err := runtime.State.SetModel(replacement); err != nil {
			return err
		}
		if err := runtime.State.SetSession(newLog); err != nil {
			return err
		}
		return runtime.State.Tools().Register(&testTool{})
	}, input: func(runtime abi.RuntimeContext, message wire.Message) error {
		order = append(order, "input")
		stored := records(t, newLog)
		if len(stored) != 1 || message.ID == "" || stored[0].Message.ID != message.ID {
			t.Fatal("input hook before commit or missing stable identity")
		}
		abi.PutExtension(runtime.State.Extensions(), message.ID)
		return nil
	}, before: func(runtime abi.RuntimeContext, _ *abi.ModelRequest) error {
		order = append(order, "before")
		if _, ok := abi.GetExtension[string](runtime.State.Extensions()); !ok {
			t.Fatal("typed runtime extension absent")
		}
		return nil
	}, wrapper: func(ctx context.Context, runtime abi.RuntimeContext, request abi.ModelRequest, emit func(wire.StreamChunk) error, next abi.ModelFunc) error {
		order = append(order, "wrap-in")
		err := next(ctx, runtime, request, emit)
		order = append(order, "wrap-out")
		return err
	}, end: func(runtime abi.RuntimeContext, err error) error {
		order = append(order, "end")
		if runtime.State.Session() != newLog || err != nil {
			t.Fatal("end did not see live state")
		}
		return nil
	}}
	agent := build(t, core.NewBuilder(textModel("wrong"), oldLog).Middleware(hook))
	if err := agent.Run(context.Background(), []wire.Message{wire.TextMessage(wire.RoleUser, "input")}, discard); err != nil {
		t.Fatal(err)
	}
	if len(withoutHookAudits(records(t, oldLog))) != 0 || len(withoutHookAudits(records(t, newLog))) != 4 {
		t.Fatal("replacement did not own input/model facts")
	}
	if !reflect.DeepEqual(order, []string{"start", "input", "before", "wrap-in", "model", "wrap-out", "end"}) {
		t.Fatal(order)
	}
}
func TestToolRegistryExplicitReplaceAndDetachedSnapshot(t *testing.T) {
	first, second := &testTool{}, &testTool{}
	registry, err := abi.NewToolRegistry([]abi.Tool{first})
	if err != nil {
		t.Fatal(err)
	}
	if err := registry.Register(second); err == nil {
		t.Fatal("implicit same-name replacement")
	}
	snapshot := registry.Snapshot()
	snapshot.Definitions[0].Parameters[0] = 'x'
	snapshot.Tools[0] = nil
	if snapshot2 := registry.Snapshot(); snapshot2.Definitions[0].Parameters[0] != '{' || snapshot2.Tools[0] != first {
		t.Fatal("snapshot shares registry backing")
	}
	old, err := registry.Replace(second)
	if err != nil || old != first {
		t.Fatal(old, err)
	}
	if !registry.Remove("test") || registry.Remove("test") {
		t.Fatal("remove semantics")
	}
	if _, err := registry.Replace(first); err == nil {
		t.Fatal("replace silently registered missing tool")
	}
}
func TestSharedRuntimeHookKeepsAgentStateSeparate(t *testing.T) {
	type seen struct{ calls int }
	hook := &runtimeHooks{start: func(runtime abi.RuntimeContext) error {
		if _, ok := abi.GetExtension[seen](runtime.State.Extensions()); ok {
			return errors.New("another Agent leaked state")
		}
		abi.PutExtension(runtime.State.Extensions(), seen{1})
		return nil
	}}
	for _, id := range []string{"one", "two"} {
		agent := build(t, core.NewBuilder(textModel("done"), core.NewMemorySession(id)).Middleware(hook))
		if err := agent.Run(context.Background(), nil, discard); err != nil {
			t.Fatal(err)
		}
	}
}

func TestSameIDDistinctSessionAuthoritiesBothFlush(t *testing.T) {
	fixture := errors.New("first authority flush failed")
	first := &failingSession{MemorySession: core.NewMemorySession("same-id"), flushErr: fixture}
	second := &failingSession{MemorySession: core.NewMemorySession("same-id")}
	hook := &runtimeHooks{wrapper: func(ctx context.Context, runtime abi.RuntimeContext, request abi.ModelRequest, emit func(wire.StreamChunk) error, next abi.ModelFunc) error {
		if err := runtime.State.SetSession(second); err != nil {
			return err
		}
		return next(ctx, runtime, request, emit)
	}}
	agent := build(t, core.NewBuilder(textModel("done"), first).Middleware(hook))
	err := agent.Run(context.Background(), []wire.Message{wire.TextMessage(wire.RoleUser, "input")}, discard)
	if !errors.Is(err, fixture) || !first.flushed || !second.flushed {
		t.Fatal("actual log authority was lost behind duplicate ID", err, first.flushed, second.flushed)
	}
	if len(withoutHookAudits(records(t, first))) != 4 || len(withoutHookAudits(records(t, second))) != 0 {
		t.Fatal("in-flight facts redirected to replacement authority")
	}
}

func withoutHookAudits(records []abi.SessionRecord) []abi.SessionRecord {
	var out []abi.SessionRecord
	for _, r := range records {
		if r.Namespace != "alva.kernel.audit" {
			out = append(out, r)
		}
	}
	return out
}

func TestActualToolBindingIsResolvedImplementation(t *testing.T) {
	var original, replacement *testTool
	executed := false
	original = &testTool{execute: func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
		t.Fatal("old implementation executed")
		return wire.ToolResult{}, nil
	}}
	replacement = &testTool{execute: func(_ context.Context, execution abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
		if execution.ActualTool != replacement || execution.ActualTool == original {
			t.Fatal("tool identity was only a stale name")
		}
		executed = true
		return wire.ToolResult{CallID: call.ID}, nil
	}}
	turn := 0
	model := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		turn++
		if turn == 1 {
			if err := emit(callChunk("actual")); err != nil {
				return err
			}
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	hook := &runtimeHooks{start: func(runtime abi.RuntimeContext) error {
		_, err := runtime.State.Tools().Replace(replacement)
		return err
	}}
	agent := build(t, core.NewBuilder(model, core.NewMemorySession("actual")).Tool(original).Middleware(hook))
	if err := agent.Run(context.Background(), nil, discard); err != nil || !executed {
		t.Fatal(err, executed)
	}
}
