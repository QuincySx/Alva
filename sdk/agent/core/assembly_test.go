// INPUT: context, errors, reflect, strings, testing, agent/abi, agent/context, kernel/abi, kernel/bus, kernel/core, llm/wire
// OUTPUT: Assembly snapshot ownership, cancellation boundaries, panic sealing and identity regressions
// POS: [L2.5 SDK assembly] Exercises plugin callbacks against real builds and runs without adding lifecycle mechanisms.
package core_test

import (
	"context"
	"errors"
	"reflect"
	"strings"
	"testing"

	agentabi "github.com/QuincySx/Alva/sdk/agent/abi"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/kernel/bus"
	kernel "github.com/QuincySx/Alva/sdk/kernel/core"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type assemblyLoopFunc func(context.Context, kernel.RunConfig, []wire.Message, func(abi.Event) error) error

func (fn assemblyLoopFunc) Run(ctx context.Context, config kernel.RunConfig, input []wire.Message, emit func(abi.Event) error) error {
	return fn(ctx, config, input, emit)
}

type namedLayer string

func (name namedLayer) Name() string { return string(name) }

func TestBuildCapturesPluginSetBeforeCallbacks(t *testing.T) {
	registered, finalized := 0, 0
	late := plugin{name: "next-build", register: func(context.Context, *core.Registrar) error { registered++; return nil }, lateTools: func(context.Context, core.LateContext) ([]abi.Tool, error) {
		finalized++
		return []abi.Tool{&testTool{}}, nil
	}}
	modelCalls := 0
	model := modelFunc(func(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		modelCalls++
		var prompt string
		for _, message := range request.Messages {
			if message.Role == wire.RoleSystem {
				prompt += message.Text()
			}
		}
		if modelCalls == 1 && strings.Contains(prompt, "next prompt") {
			return errors.New("callback changed current build configuration")
		}
		if modelCalls == 2 && !strings.Contains(prompt, "next prompt") {
			return errors.New("builder cannot be reused with new configuration")
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	builder := core.NewBuilder(model, core.NewMemorySession("assembly-capture")).SystemPrompt("original")
	changed := false
	builder.Plugin(plugin{name: "mutator", register: func(context.Context, *core.Registrar) error {
		if !changed {
			changed = true
			builder.Plugin(late).SystemPrompt("next prompt")
		}
		return nil
	}})
	var first *core.Agent
	func() {
		defer func() {
			if value := recover(); value != nil {
				t.Errorf("callback corrupted this build: %v", value)
			}
		}()
		first = build(t, builder)
	}()
	if first == nil {
		return
	}
	if registered != 0 || finalized != 0 || len(first.Snapshot().Plugins) != 1 {
		t.Fatal("unregistered plugin entered this build", registered, finalized, first.Snapshot())
	}
	if err := first.Run(t.Context(), nil, discard); err != nil {
		t.Fatal(err)
	}
	second := build(t, builder)
	if registered != 1 || finalized != 1 || len(second.Snapshot().Tools) != 1 {
		t.Fatal("new builder contributions did not apply to next build", registered, finalized)
	}
	if err := second.Run(t.Context(), nil, discard); err != nil {
		t.Fatal(err)
	}
}

func TestFinalizeCannotChangeCurrentBuilderRecipe(t *testing.T) {
	modelCalled := false
	model := modelFunc(func(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		modelCalled = true
		if len(request.Tools) != 0 {
			return errors.New("late builder mutation changed current tool recipe")
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	builder := core.NewBuilder(model, core.NewMemorySession("finalize-recipe"))
	builder.Plugin(plugin{name: "finalizer", finalize: func(context.Context, core.LateContext) error {
		builder.Loop(assemblyLoopFunc(func(context.Context, kernel.RunConfig, []wire.Message, func(abi.Event) error) error {
			return errors.New("changed loop ran")
		})).Tool(&testTool{})
		return nil
	}})
	if err := build(t, builder).Run(t.Context(), nil, discard); err != nil || !modelCalled {
		t.Fatal("finalizer changed active recipe", modelCalled, err)
	}
}

func TestBuildCancellationStopsFollowingCallbacksAndPublication(t *testing.T) {
	for _, stage := range []string{"register", "finalize"} {
		for _, cancelAt := range []int{0, 1} {
			t.Run(stage+string(rune('0'+cancelAt)), func(t *testing.T) {
				ctx, cancel := context.WithCancel(t.Context())
				defer cancel()
				builder := core.NewBuilder(textModel("unused"), core.NewMemorySession("cancel-build"))
				for index := 0; index < 2; index++ {
					builder.Plugin(plugin{name: string(rune('a' + index)), register: func(ctx context.Context, _ *core.Registrar) error {
						if ctx.Err() != nil {
							t.Error("Register invoked after cancellation")
						}
						if stage == "register" && index == cancelAt {
							cancel()
						}
						return nil
					}, finalize: func(ctx context.Context, _ core.LateContext) error {
						if ctx.Err() != nil {
							t.Error("Finalize invoked after cancellation")
						}
						if stage == "finalize" && index == cancelAt {
							cancel()
						}
						return nil
					}})
				}
				agent, err := builder.Build(ctx)
				if agent != nil || !errors.Is(err, context.Canceled) {
					t.Fatal("cancelled assembly was published", agent, err)
				}
			})
		}
	}
}

func TestRegistrationPanicStillSealsEscapedContributions(t *testing.T) {
	var retained *core.Registrar
	panicked := false
	func() {
		defer func() { panicked = recover() != nil }()
		_, _ = core.NewBuilder(textModel("unused"), core.NewMemorySession("panic-build")).Plugin(plugin{name: "panic", register: func(_ context.Context, r *core.Registrar) error { retained = r; panic("plugin fixture") }}).Build(t.Context())
	}()
	if !panicked || retained == nil {
		t.Fatal("expected callback panic")
	}
	if err := retained.SystemPrompt("after aborted assembly"); err == nil {
		t.Error("escaped Registrar remained writable")
	}
	if err := bus.Provide[string](retained.BusWriter(), "late capability"); err == nil {
		t.Error("escaped bus writer remained writable")
	}
}

func TestContextSystemDescriptorIsOwnedAtRegistration(t *testing.T) {
	for _, route := range []string{"builder", "plugin"} {
		for _, mutation := range []string{"after-registration", "after-build"} {
			t.Run(route+"/"+mutation, func(t *testing.T) {
				system := agentcontext.DefaultSystem(1000, 2000)
				called := false
				builder := core.NewBuilder(modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
					called = true
					return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
				}), core.NewMemorySession("owned-context"))
				if route == "builder" {
					builder.ContextSystem(system)
					if mutation == "after-registration" {
						system.Hooks = nil
					}
				} else {
					builder.Plugin(plugin{name: "context", register: func(_ context.Context, r *core.Registrar) error {
						if err := r.ContextSystem(system); err != nil {
							return err
						}
						if mutation == "after-registration" {
							system.Hooks = nil
						}
						return nil
					}})
				}
				agent, err := builder.Build(t.Context())
				if err != nil {
					t.Fatal("caller changed an accepted descriptor", err)
				}
				if mutation == "after-build" {
					system.Hooks = nil
				}
				if err := agent.Run(t.Context(), []wire.Message{wire.TextMessage(wire.RoleUser, "check")}, discard); err != nil || !called {
					t.Fatal("caller changed agent context configuration", called, err)
				}
			})
		}
	}
}

func TestReplacementLoopCannotMutateLaterRunDescriptors(t *testing.T) {
	calls := 0
	loop := assemblyLoopFunc(func(_ context.Context, config kernel.RunConfig, _ []wire.Message, _ func(abi.Event) error) error {
		calls++
		if config.ContextSystem.BudgetTokens != 1000 || config.Middleware[0].Name() != "original" {
			return errors.New("previous invocation modified assembly defaults")
		}
		config.ContextSystem.BudgetTokens = 1
		config.Middleware[0] = namedLayer("replaced")
		return nil
	})
	agent := build(t, core.NewBuilder(textModel("unused"), core.NewMemorySession("loop-config")).ContextSystem(agentcontext.DefaultSystem(1000, 2000)).Middleware(namedLayer("original")).Loop(loop))
	for range 2 {
		if err := agent.Run(t.Context(), nil, discard); err != nil {
			t.Fatal(err)
		}
	}
	if calls != 2 || !reflect.DeepEqual(agent.Snapshot().Middleware, []string{"original"}) {
		t.Fatal(calls, agent.Snapshot())
	}
}

func TestAssemblyRejectsInvalidUTF8IdentityBeforeConsumerIO(t *testing.T) {
	invalid := string([]byte{0xff})
	for _, kind := range []string{"plugin", "middleware", "command-name", "command-description"} {
		t.Run(kind, func(t *testing.T) {
			called := false
			builder := core.NewBuilder(textModel("unused"), core.NewMemorySession("invalid-identity"))
			switch kind {
			case "plugin":
				builder.Plugin(plugin{name: invalid})
			case "middleware":
				builder.Middleware(namedLayer(invalid))
			case "command-name", "command-description":
				builder.Plugin(plugin{name: "command", register: func(_ context.Context, r *core.Registrar) error {
					command := agentabi.Command{Name: "valid", Description: "valid"}
					if kind == "command-name" {
						command.Name = invalid
					} else {
						command.Description = invalid
					}
					return r.Command(command)
				}})
			}
			builder.Plugin(plugin{name: "consumer", register: func(context.Context, *core.Registrar) error { called = true; return nil }})
			if _, err := builder.Build(t.Context()); err == nil || called {
				t.Fatal("invalid identity reached consumer", called, err)
			}
		})
	}
}
