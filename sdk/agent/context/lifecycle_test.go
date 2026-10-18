// INPUT: context, encoding/json, errors, reflect, testing, agent/core, kernel/abi, llm/wire
// OUTPUT: Run-scoped context cleanup, repeated startup and session-switch lifecycle regressions
// POS: [L3 context acceptance] Defines Bootstrap/Dispose ownership without adding per-session disposal hooks.
package agentcontext

import (
	"context"
	"encoding/json"
	"errors"
	"reflect"
	"testing"

	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type runLifecycle struct {
	ids       []string
	disposals int
	fail      error
}

func (*runLifecycle) Name() string { return "run-lifecycle" }
func (h *runLifecycle) Bootstrap(_ context.Context, _ abi.ContextHandle, id string) error {
	h.ids = append(h.ids, id)
	return h.fail
}
func (h *runLifecycle) Dispose(ctx context.Context) error { h.disposals++; return ctx.Err() }

func TestContextDisposeIsOncePerRunAcrossFailureAndReuse(t *testing.T) {
	failure := errors.New("partial bootstrap")
	hooks := &runLifecycle{fail: failure}
	model := modelFunc(func(_ context.Context, _ abi.ModelRequest, e func(wire.StreamChunk) error) error {
		return e(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	agent, err := core.NewBuilder(model, core.NewMemorySession("reuse")).ContextSystem(&abi.ContextSystem{Hooks: hooks, Handle: NewStore(0, 0, nil, nil)}).Build(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	if err = agent.Run(t.Context(), nil, func(abi.Event) error { return nil }); !errors.Is(err, failure) || hooks.disposals != 1 {
		t.Fatal("failed bootstrap cleanup lost", err, hooks.disposals)
	}
	hooks.fail = nil
	if err = agent.Run(t.Context(), nil, func(abi.Event) error { return nil }); err != nil || hooks.disposals != 2 || !reflect.DeepEqual(hooks.ids, []string{"reuse", "reuse"}) {
		t.Fatal("reused lifecycle mismatch", err, hooks)
	}
}

type switchContextSession struct {
	next    abi.ConversationLog
	changed bool
}

func (*switchContextSession) Name() string { return "switch-context-session" }
func (h *switchContextSession) WrapModel(ctx context.Context, r abi.RuntimeContext, q abi.ModelRequest, e func(wire.StreamChunk) error, n abi.ModelFunc) error {
	if !h.changed {
		h.changed = true
		if err := r.State.SetSession(h.next); err != nil {
			return err
		}
	}
	return n(ctx, r, q, e)
}

type lifecycleTool struct{}

func (lifecycleTool) Name() string                      { return "continue" }
func (lifecycleTool) Description() string               { return "continue once" }
func (lifecycleTool) ParametersSchema() json.RawMessage { return json.RawMessage(`{}`) }
func (lifecycleTool) Execute(_ context.Context, _ abi.ToolExecutionContext, c wire.ToolCall) (wire.ToolResult, error) {
	return wire.ToolResult{CallID: c.ID}, nil
}
func (lifecycleTool) Scheduling(wire.ToolCall) abi.Scheduling { return abi.Scheduling{} }
func (lifecycleTool) DescribeEffect(wire.ToolCall) abi.Effect {
	return abi.Effect{Kind: abi.EffectRead}
}
func (lifecycleTool) IsReadOnly(wire.ToolCall) bool { return true }

func TestContextSessionSwitchBootstrapsBothAndDisposesRunOnce(t *testing.T) {
	hooks := &runLifecycle{}
	round := 0
	model := modelFunc(func(_ context.Context, _ abi.ModelRequest, e func(wire.StreamChunk) error) error {
		round++
		if round == 1 {
			c := wire.ToolCall{ID: "one", Name: "continue", Arguments: []byte(`{}`)}
			if err := e(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &c}); err != nil {
				return err
			}
		}
		return e(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	agent, err := core.NewBuilder(model, core.NewMemorySession("first")).Tool(lifecycleTool{}).Middleware(&switchContextSession{next: core.NewMemorySession("second")}).ContextSystem(&abi.ContextSystem{Hooks: hooks, Handle: NewStore(0, 0, nil, nil)}).Build(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	if err = agent.Run(t.Context(), nil, func(abi.Event) error { return nil }); err != nil || round != 2 || hooks.disposals != 1 || !reflect.DeepEqual(hooks.ids, []string{"first", "second"}) {
		t.Fatal("session switch cleanup mismatched", err, hooks, round)
	}
}
