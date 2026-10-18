// INPUT: context, strings, testing, agent/context/core, kernel/abi, llm/wire
// OUTPUT: Default-by-name final budget policy and late wrapper rejection tests
// POS: [L5 harness acceptance] Explicit finite context policy reaches final model admission and remains replaceable.
package harness

import (
	"context"
	"strings"
	"testing"

	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type budgetModel struct{ calls int }

func (m *budgetModel) Stream(_ context.Context, _ abi.ModelRequest, e func(wire.StreamChunk) error) error {
	m.calls++
	return e(wire.StreamChunk{Kind: wire.ChunkFinish})
}

type lateBudgetContent struct{}

func (lateBudgetContent) Name() string { return "late-content" }
func (lateBudgetContent) WrapModel(ctx context.Context, r abi.RuntimeContext, q abi.ModelRequest, e func(wire.StreamChunk) error, n abi.ModelFunc) error {
	q.Messages = append(q.Messages, wire.TextMessage(wire.RoleUser, strings.Repeat("late", 16384)))
	return n(ctx, r, q, e)
}

type replaceBudget struct{}

func (replaceBudget) Name() string                                    { return "context-budget" }
func (replaceBudget) Register(context.Context, *core.Registrar) error { return nil }

func TestFiniteContextBudgetRejectsWrapperAdditionAndCanBeReplaced(t *testing.T) {
	for _, mode := range []string{"baseline", "late", "replaced"} {
		t.Run(mode, func(t *testing.T) {
			model := &budgetModel{}
			builder := NewBuilder(model, core.NewMemorySession(mode)).Workspace(t.TempDir()).ContextSystem(agentcontext.DefaultSystem(4096, 8192))
			if mode != "baseline" {
				builder.Middleware(lateBudgetContent{})
			}
			if mode == "replaced" {
				builder.Plugin(replaceBudget{})
			}
			agent, err := builder.Build(t.Context())
			if err != nil {
				t.Fatal(err)
			}
			err = agent.Run(t.Context(), nil, func(abi.Event) error { return nil })
			if mode == "late" {
				if err == nil || !strings.Contains(err.Error(), "estimation budget") || model.calls != 0 {
					t.Fatal("late content escaped final admission", err, model.calls)
				}
			} else if err != nil || model.calls != 1 {
				t.Fatal(err, model.calls)
			}
		})
	}
}

func TestContextDescriptorOwnershipIncludesFinalAdmission(t *testing.T) {
	for _, timing := range []string{"after-setter", "after-build"} {
		t.Run(timing, func(t *testing.T) {
			model := &budgetModel{}
			system := agentcontext.DefaultSystem(4096, 8192)
			builder := NewBuilder(model, core.NewMemorySession("budget-owner")).Workspace(t.TempDir()).ContextSystem(system).Middleware(lateBudgetContent{})
			if timing == "after-setter" {
				system.BudgetTokens = 1 << 24
			}
			agent, err := builder.Build(t.Context())
			if err != nil {
				t.Fatal(err)
			}
			if timing == "after-build" {
				system.BudgetTokens = 1 << 24
			}
			if err := agent.Run(t.Context(), nil, func(abi.Event) error { return nil }); err == nil || model.calls != 0 || !strings.Contains(err.Error(), "estimation budget") {
				t.Fatal("caller changed accepted final admission policy", model.calls, err)
			}
		})
	}
}
