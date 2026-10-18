// INPUT: context, encoding/json, errors, testing, agent/core/security, kernel/abi, llm/wire
// OUTPUT: Dynamic value comparability and real Agent security fallback regressions
// POS: [L4 extension] Unprovable tool identity requires approval without crashing the execution goroutine.
package builtin

import (
	"context"
	"encoding/json"
	"errors"
	"testing"

	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/agent/security"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type identityValueTool struct {
	configuration any
	executions    *int
}

func (identityValueTool) Name() string                      { return "identity-value" }
func (identityValueTool) Description() string               { return "Value tool with interface configuration" }
func (identityValueTool) ParametersSchema() json.RawMessage { return json.RawMessage(`{}`) }
func (identityValueTool) Scheduling(wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{ConcurrentSafe: true}
}
func (identityValueTool) IsReadOnly(wire.ToolCall) bool { return true }
func (identityValueTool) DescribeEffect(wire.ToolCall) abi.Effect {
	return abi.Effect{Kind: abi.EffectRead}
}
func (tool identityValueTool) Execute(_ context.Context, _ abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	*tool.executions++
	return wire.ToolResult{CallID: call.ID}, nil
}

type identityModel struct{ called bool }

func (model *identityModel) Stream(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	if !model.called {
		model.called = true
		if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "call", Name: "identity-value", Arguments: json.RawMessage(`{}`)}}); err != nil {
			return err
		}
	}
	return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
}

func TestSecurityIdentityUsesDynamicValues(t *testing.T) {
	for _, values := range [][2]any{{map[string]string{}, "comparable"}, {"comparable", []string{}}, {map[string]string{}, map[string]string{}}} {
		func() {
			defer func() {
				if value := recover(); value != nil {
					t.Errorf("identity comparison panicked: %v", value)
				}
			}()
			if sameToolInstance(identityValueTool{configuration: values[0]}, identityValueTool{configuration: values[1]}) {
				t.Error("noncomparable configuration proved tool identity")
			}
		}()
	}
	if !sameToolInstance(identityValueTool{configuration: "same"}, identityValueTool{configuration: "same"}) {
		t.Fatal("comparable value identity lost")
	}
}

func TestValueToolIdentityFallsBackToApprovalInRealAgent(t *testing.T) {
	for _, approved := range []bool{false, true} {
		t.Run(map[bool]string{false: "approval_unavailable", true: "explicit_approval"}[approved], func(t *testing.T) {
			executions, approvals := 0, 0
			tool := identityValueTool{configuration: map[string]string{"mode": "read"}, executions: &executions}
			var handler security.ApprovalHandler
			if approved {
				handler = security.ApprovalFunc(func(_ context.Context, request security.ApprovalRequest) (security.ApprovalDecision, error) {
					approvals++
					if request.Effect.Kind != abi.EffectUnknown {
						t.Error("unprovable identity borrowed declared read trust")
					}
					return security.ApprovalDecision{Allowed: true, Scope: security.Once}, nil
				})
			}
			guard, err := security.NewGuard(t.TempDir(), nil, nil, security.NewPermissionManager(security.Ask, handler))
			if err != nil {
				t.Fatal(err)
			}
			agent, err := core.NewBuilder(&identityModel{}, core.NewMemorySession("identity")).Tool(tool).Plugin(NewSecurityPlugin(guard, []abi.Tool{tool})).Build(t.Context())
			if err != nil {
				t.Fatal(err)
			}
			err = agent.Run(t.Context(), nil, func(abi.Event) error { return nil })
			if approved {
				if err != nil || executions != 1 || approvals != 1 {
					t.Fatal("approved unknown tool did not execute exactly once", err, executions, approvals)
				}
			} else if !errors.Is(err, security.ErrApprovalRequired) || executions != 0 {
				t.Fatal("unapproved value tool executed", err, executions)
			}
		})
	}
}
