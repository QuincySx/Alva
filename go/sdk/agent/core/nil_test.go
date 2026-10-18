// INPUT: Typed-nil executable SDK contributions and a real registration side-effect counter
// OUTPUT: Model/session/loop/plugin/tool/middleware/context fail-fast regressions before runtime invocation
// POS: [L2.5 SDK assembly] A nonnil Go interface must not admit a missing implementation and defer its panic until execution.
package core_test

import (
	"context"
	"testing"

	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	kernel "github.com/QuincySx/Alva/go/sdk/kernel/core"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type missingPlugin struct{ core.Plugin }
type missingSession struct{ abi.ConversationLog }
type missingTool struct{ abi.Tool }
type missingMiddleware struct{ abi.Middleware }
type missingLoop struct{ kernel.AgentLoop }
type missingHooks struct{ abi.ContextHooks }
type missingHandle struct{ abi.ContextHandle }
type missingCounter struct{ abi.TokenCounter }
type missingApply struct{ abi.ContextApply }
type contextName struct{}

func (contextName) Name() string { return "test-context" }

func TestSDKTypedNilModelRejectedBeforePluginRegistration(t *testing.T) {
	registered := 0
	var missing modelFunc
	_, err := core.NewBuilder(missing, core.NewMemorySession("typed-nil-model")).Plugin(plugin{name: "must-not-register", register: func(context.Context, *core.Registrar) error { registered++; return nil }}).Build(t.Context())
	if err == nil || registered != 0 {
		t.Fatal("missing executable reached plugin registration", registered, err)
	}
	valid := modelFunc(func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error { return nil })
	_, err = core.NewBuilder(valid, core.NewMemorySession("")).Plugin(plugin{name: "no-register", register: func(context.Context, *core.Registrar) error { registered++; return nil }}).Build(t.Context())
	if err == nil || registered != 0 {
		t.Fatal("unidentified log reached plugin registration", registered, err)
	}
}

func TestSDKTypedNilContributionsReturnErrorsWithoutCallingMethods(t *testing.T) {
	model := modelFunc(func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error { return nil })
	base := func() *core.Builder { return core.NewBuilder(model, core.NewMemorySession("typed-nil")) }
	builders := []*core.Builder{
		core.NewBuilder(model, (*missingSession)(nil)),
		base().Loop((*missingLoop)(nil)),
		base().Plugin((*missingPlugin)(nil)),
		base().Tool((*missingTool)(nil)),
		base().Middleware((*missingMiddleware)(nil)),
		base().ContextSystem(&abi.ContextSystem{Hooks: (*missingHooks)(nil), Handle: (*missingHandle)(nil)}),
		base().ContextSystem(&abi.ContextSystem{Hooks: contextName{}, Handle: missingHandle{}, Counter: (*missingCounter)(nil)}),
		base().ContextSystem(&abi.ContextSystem{Hooks: contextName{}, Handle: missingHandle{}, Apply: (*missingApply)(nil)}),
		base().Middleware(core.BeforeModel("nil-hook", nil)),
	}
	for index, builder := range builders {
		if _, err := builder.Build(t.Context()); err == nil {
			t.Fatal("typed-nil contribution admitted", index)
		}
	}
}
