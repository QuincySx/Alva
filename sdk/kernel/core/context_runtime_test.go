// INPUT: Direct kernel context systems with typed-nil lifecycle and optional executor/counter parts
// OUTPUT: Invalid executable context and direct-loop rejection before lifecycle or cleanup invocation
// POS: [L2 kernel runtime] Direct kernel entry retains the same required-interface semantics without depending on Agent assembly.
package core

import (
	"context"
	"errors"
	"testing"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type namedContext struct{}

func (namedContext) Name() string { return "direct-kernel-context" }

type absentContextHooks struct{ abi.ContextHooks }
type absentContextHandle struct{ abi.ContextHandle }
type absentContextApply struct{ abi.ContextApply }
type absentContextCounter struct{ abi.TokenCounter }

func TestDirectKernelRejectsTypedNilContextParts(t *testing.T) {
	for _, system := range []*abi.ContextSystem{
		{Hooks: (*absentContextHooks)(nil), Handle: absentContextHandle{}},
		{Hooks: namedContext{}, Handle: (*absentContextHandle)(nil)},
		{Hooks: namedContext{}, Handle: absentContextHandle{}, Apply: (*absentContextApply)(nil)},
		{Hooks: namedContext{}, Handle: absentContextHandle{}, Counter: (*absentContextCounter)(nil)},
	} {
		if err := NewContextRuntime("root", system).Bootstrap(t.Context()); err == nil {
			t.Fatal("direct kernel admitted a missing executable context part")
		}
	}
}

type absentDisposeHooks struct{ calls int }

func (*absentDisposeHooks) Name() string { return "missing-lifecycle" }
func (hooks *absentDisposeHooks) Dispose(context.Context) error {
	hooks.calls++
	return nil
}

func TestInvalidContextNeverInvokesLifecycleAfterBootstrapFailure(t *testing.T) {
	system := &abi.ContextSystem{Hooks: (*absentDisposeHooks)(nil), Handle: absentContextHandle{}}
	runtime := NewContextRuntime("invalid-lifecycle", system)
	defer func() {
		if recovered := recover(); recovered != nil {
			t.Fatal("invalid executable invoked after failed bootstrap", recovered)
		}
	}()
	if err := runtime.Bootstrap(t.Context()); err == nil {
		t.Fatal("invalid bootstrap admitted")
	}
	if err := runtime.OnMessage(t.Context(), wire.TextMessage(wire.RoleUser, "must not ingest")); err == nil {
		t.Fatal("invalid ingest admitted")
	}
	if _, _, err := runtime.Prepare(t.Context(), nil, nil); err == nil {
		t.Fatal("invalid prepare admitted")
	}
	if err := runtime.AfterTurn(t.Context()); err == nil {
		t.Fatal("invalid after-turn admitted")
	}
	if err := runtime.Dispose(t.Context()); err == nil {
		t.Fatal("invalid dispose admitted")
	}
}

type preflightHooks struct{ calls *int }

func (preflightHooks) Name() string { return "preflight-side-effect-probe" }
func (hook preflightHooks) OnRunStart(context.Context, abi.RuntimeContext) error {
	*hook.calls++
	return nil
}
func (hook preflightHooks) OnRunEnd(context.Context, abi.RuntimeContext, error) error {
	*hook.calls++
	return nil
}

type absentRunMiddleware struct {
	abi.Middleware
	abi.RunStartHook
	abi.RunEndHook
}

func TestDirectLoopRejectsMissingContributionsBeforeLifecycle(t *testing.T) {
	for _, name := range []string{"nil-middleware", "typed-nil-middleware", "invalid-context", "zero-state"} {
		t.Run(name, func(t *testing.T) {
			defer func() {
				if recovered := recover(); recovered != nil {
					t.Fatal("invalid direct-loop config invoked a missing implementation", recovered)
				}
			}()
			calls, events := 0, 0
			config := RunConfig{State: testState(t, "preflight"), Middleware: []abi.Middleware{preflightHooks{&calls}}}
			switch name {
			case "nil-middleware":
				config.Middleware = append(config.Middleware, nil)
			case "typed-nil-middleware":
				config.Middleware = append(config.Middleware, (*absentRunMiddleware)(nil))
			case "invalid-context":
				config.ContextSystem = &abi.ContextSystem{Hooks: (*absentDisposeHooks)(nil), Handle: absentContextHandle{}}
			case "zero-state":
				config.State = &abi.AgentState{}
			}
			err := (DefaultAgentLoop{}).Run(t.Context(), config, nil, func(abi.Event) error { events++; return nil })
			if err == nil || calls != 0 || events != 0 {
				t.Fatal("invalid config reached lifecycle side effects or emitted execution", calls, events, err)
			}
		})
	}
}

type contextSourceFunc func(context.Context, abi.ConversationReader) ([]wire.Message, error)

func (fn contextSourceFunc) Load(ctx context.Context, reader abi.ConversationReader) ([]wire.Message, error) {
	return fn(ctx, reader)
}

type sourceReader struct{ messages int }

func (*sourceReader) ID() string { return "source-session" }
func (reader *sourceReader) Messages(context.Context) ([]wire.Message, error) {
	reader.messages++
	return []wire.Message{wire.TextMessage(wire.RoleUser, "full history")}, nil
}
func (*sourceReader) Flush(context.Context) error { return nil }
func (*sourceReader) AppendRecords(context.Context, []abi.SessionRecord) ([]abi.SessionRecord, error) {
	panic("source obtained append authority")
}

type missingContextSource struct{ abi.ContextSource }

func TestContextSourceHasBoundReaderAndOwnedWorkingView(t *testing.T) {
	reader := &sourceReader{}
	original := []wire.Message{wire.TextMessage(wire.RoleUser, "bounded view")}
	source := contextSourceFunc(func(_ context.Context, bound abi.ConversationReader) ([]wire.Message, error) {
		if bound.ID() != reader.ID() {
			t.Fatal("session identity lost")
		}
		if _, mutable := bound.(abi.ConversationLog); mutable {
			t.Fatal("source received write authority")
		}
		return original, nil
	})
	messages, err := NewContextRuntime(reader.ID(), nil).Load(t.Context(), reader, source)
	if err != nil || reader.messages != 0 || len(messages) != 1 {
		t.Fatal(messages, err, reader.messages)
	}
	messages[0].Content[0].Text = "mutated"
	if original[0].Text() != "bounded view" {
		t.Fatal("source view aliased runtime")
	}
}
func TestContextSourceErrorsNeverFallBackToFullHistory(t *testing.T) {
	reader := &sourceReader{}
	failure := errors.New("source unavailable")
	for _, source := range []abi.ContextSource{contextSourceFunc(func(context.Context, abi.ConversationReader) ([]wire.Message, error) { return nil, failure }), (*missingContextSource)(nil), contextSourceFunc(func(context.Context, abi.ConversationReader) ([]wire.Message, error) {
		return []wire.Message{{Role: "invalid"}}, nil
	})} {
		if _, err := NewContextRuntime(reader.ID(), nil).Load(t.Context(), reader, source); err == nil {
			t.Fatal("invalid source accepted")
		}
	}
	if reader.messages != 0 {
		t.Fatal("source failure read full history")
	}
	if _, err := NewContextRuntime("foreign", nil).Load(t.Context(), reader, nil); err == nil {
		t.Fatal("foreign reader accepted")
	}
	messages, err := NewContextRuntime(reader.ID(), nil).Load(t.Context(), reader, nil)
	if err != nil || len(messages) != 1 || reader.messages != 1 {
		t.Fatal("default reader path changed", messages, err)
	}
}
