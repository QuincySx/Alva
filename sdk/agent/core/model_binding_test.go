// INPUT: context, testing, agent/core, kernel/abi, llm/wire
// OUTPUT: Actual per-model-invocation snapshot and concurrent mutation regressions
// POS: [L2.5 SDK assembly] Prevents wrapper/runtime identity checks from racing a second mutable State.Model lookup.
package core_test

import (
	"context"
	"testing"

	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type bindingModel struct {
	calls int
	text  string
}

func (model *bindingModel) Stream(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	model.calls++
	if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: model.text}); err != nil {
		return err
	}
	return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
}

func TestBeforeModelMutationBindsCurrentInvocationButNotOtherLifecycle(t *testing.T) {
	old, selected := &bindingModel{text: "old"}, &bindingModel{text: "selected"}
	hook := &runtimeHooks{before: func(r abi.RuntimeContext, _ *abi.ModelRequest) error {
		if r.InvocationModel != nil {
			t.Error("invocation evidence leaked outside wrappers")
		}
		return r.State.SetModel(selected)
	}, wrapper: func(ctx context.Context, r abi.RuntimeContext, request abi.ModelRequest, emit func(wire.StreamChunk) error, next abi.ModelFunc) error {
		if r.InvocationModel != selected {
			t.Error("BeforeModel mutation not bound")
		}
		return next(ctx, r, request, emit)
	}, end: func(r abi.RuntimeContext, _ error) error {
		if r.InvocationModel != nil {
			t.Error("run-end carried stale invocation model")
		}
		return nil
	}}
	if err := build(t, core.NewBuilder(old, core.NewMemorySession("binding")).Middleware(hook)).Run(context.Background(), nil, discard); err != nil || old.calls != 0 || selected.calls != 1 {
		t.Fatal("wrong current model", err, old.calls, selected.calls)
	}
}
func TestWrapperModelMutationDoesNotRedirectCurrentMultiNextInvocation(t *testing.T) {
	actual, replacement := &bindingModel{text: "actual"}, &bindingModel{text: "replacement"}
	round := 0
	hook := &runtimeHooks{wrapper: func(ctx context.Context, r abi.RuntimeContext, request abi.ModelRequest, emit func(wire.StreamChunk) error, next abi.ModelFunc) error {
		round++
		if round == 1 {
			if r.InvocationModel != actual {
				t.Fatal("actual model absent")
			}
			if err := r.State.SetModel(replacement); err != nil {
				return err
			}
			if err := next(ctx, r, request, func(wire.StreamChunk) error { return nil }); err != nil {
				return err
			}
			return next(ctx, r, request, emit)
		}
		if r.InvocationModel != replacement {
			t.Fatal("replacement not effective next invocation")
		}
		return next(ctx, r, request, emit)
	}}
	agent := build(t, core.NewBuilder(actual, core.NewMemorySession("multiple")).Middleware(hook))
	if err := agent.Run(context.Background(), nil, discard); err != nil {
		t.Fatal(err)
	}
	if actual.calls != 2 || replacement.calls != 0 {
		t.Fatal("multi-next contract redirected or disabled", actual.calls, replacement.calls)
	}
	if err := agent.Run(context.Background(), nil, discard); err != nil || replacement.calls != 1 {
		t.Fatal("next invocation did not consume live state", err, replacement.calls)
	}
}
func TestConcurrentSetModelCannotRaceActualInvocationTarget(t *testing.T) {
	actual, replacement := &bindingModel{text: "actual"}, &bindingModel{text: "replacement"}
	entered, release := make(chan struct{}), make(chan struct{})
	hook := &runtimeHooks{wrapper: func(ctx context.Context, r abi.RuntimeContext, request abi.ModelRequest, emit func(wire.StreamChunk) error, next abi.ModelFunc) error {
		if r.InvocationModel != actual {
			t.Error("missing actual snapshot")
		}
		close(entered)
		<-release
		return next(ctx, r, request, emit)
	}}
	agent := build(t, core.NewBuilder(actual, core.NewMemorySession("concurrent")).Middleware(hook))
	done := make(chan error, 1)
	go func() { done <- agent.Run(context.Background(), nil, discard) }()
	<-entered
	if err := agent.State().SetModel(replacement); err != nil {
		t.Fatal(err)
	}
	close(release)
	if err := <-done; err != nil || actual.calls != 1 || replacement.calls != 0 {
		t.Fatal("TOCTOU redirected actual call", err, actual.calls, replacement.calls)
	}
}
