// INPUT: agent/core Plugin registration, agent/security Guard, kernel Tool contracts
// OUTPUT: NewSecurityPlugin
// POS: [L4 extension] Optional security middleware wrapper with explicit known-tool ownership and fail-closed unknown effects
package securityplugin

import (
	"context"
	"errors"
	"reflect"

	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type securityPlugin struct {
	guard   *security.Guard
	trusted []abi.Tool
}

func NewSecurityPlugin(guard *security.Guard, trustedTools []abi.Tool) core.Plugin {
	return &securityPlugin{guard: guard, trusted: append([]abi.Tool(nil), trustedTools...)}
}
func (*securityPlugin) Name() string { return "security" }
func (p *securityPlugin) Register(_ context.Context, r *core.Registrar) error {
	if p.guard == nil {
		return errors.New("security guard is required")
	}
	known := make(map[string]abi.Tool, len(p.trusted))
	for _, tool := range p.trusted {
		if tool == nil || tool.Name() == "" {
			return errors.New("invalid trusted tool")
		}
		if _, exists := known[tool.Name()]; exists {
			return errors.New("duplicate trusted tool")
		}
		known[tool.Name()] = tool
	}
	return r.Middleware(securityMiddleware{guard: p.guard, known: known})
}

type securityMiddleware struct {
	guard *security.Guard
	known map[string]abi.Tool
}

func (securityMiddleware) Name() string { return "security" }
func (m securityMiddleware) WrapTool(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall, next abi.ToolFunc) (wire.ToolResult, error) {
	effect := abi.Effect{Kind: abi.EffectUnknown, Description: "Unknown or externally contributed tool; explicit host approval required"}
	if tool, known := m.known[call.Name]; known && sameToolInstance(tool, execution.ActualTool) {
		effect = tool.DescribeEffect(call)
		if effect.Kind == abi.EffectRead && !tool.IsReadOnly(call) {
			effect.Kind = abi.EffectUnknown
		}
	}
	if err := m.guard.Authorize(ctx, execution, call, effect); err != nil {
		return wire.ToolResult{}, err
	}
	return next(ctx, execution, call)
}

// Mutable AgentState may replace a name after assembly. Only the exact
// registered implementation resolved for this invocation carries trust.
// Noncomparable Go values cannot prove identity and remain unknown/HITL.
func sameToolInstance(trusted, actual abi.Tool) bool {
	if trusted == nil || actual == nil {
		return false
	}
	t := reflect.TypeOf(trusted)
	return t == reflect.TypeOf(actual) && reflect.ValueOf(trusted).Comparable() && reflect.ValueOf(actual).Comparable() && trusted == actual
}
