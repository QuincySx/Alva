// INPUT: context, json, testing, agent/core, kernel/abi, wire
// OUTPUT: Default validation policy and same-name Plugin replacement regressions
// POS: [L5 harness testing] Verifies real default assembly rejects bad arguments while explicit replacement stays available.
package harness

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type integerEcho struct {
	echoTool
	calls int
}

func (*integerEcho) ParametersSchema() json.RawMessage {
	return json.RawMessage(`{"type":"object","properties":{"text":{"type":"integer"}},"required":["text"]}`)
}
func (tool *integerEcho) Execute(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	tool.calls++
	return tool.echoTool.Execute(ctx, execution, call)
}

type replaceValidation struct{}

func (replaceValidation) Name() string                                    { return "tool-validation" }
func (replaceValidation) Register(context.Context, *core.Registrar) error { return nil }

func TestDefaultValidationAndExplicitSameNameReplacement(t *testing.T) {
	for _, replace := range []bool{false, true} {
		t.Run(map[bool]string{false: "default", true: "replacement"}[replace], func(t *testing.T) {
			tool := &integerEcho{}
			log := core.NewMemorySession("harness-validation")
			builder := NewBuilder(demoModel{}, log).Workspace(t.TempDir()).Tool(tool)
			if replace {
				builder.Plugin(replaceValidation{})
			}
			agent, err := builder.Build(t.Context())
			if err != nil {
				t.Fatal(err)
			}
			if err = agent.Run(t.Context(), []wire.Message{wire.TextMessage(wire.RoleUser, "string-argument")}, func(abi.Event) error { return nil }); err != nil {
				t.Fatal(err)
			}
			want := 0
			if replace {
				want = 1
			}
			if tool.calls != want {
				t.Fatalf("wrong validation policy: executions=%d want=%d", tool.calls, want)
			}
			count := 0
			for _, plugin := range agent.Snapshot().Plugins {
				if plugin.Name == "tool-validation" {
					count++
				}
			}
			if count != 1 {
				t.Fatal("default replacement duplicated or lost")
			}
		})
	}
}
