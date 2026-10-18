// INPUT: context, encoding/json, strings, testing, kernel/abi, llm/wire
// OUTPUT: Actual per-run model settings, tool admission and prompt isolation regressions
// POS: [L2.5 SDK assembly] Verifies typed request configuration flows to the provider and restricts execution, not merely schemas.
package core_test

import (
	"context"
	"encoding/json"
	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
	"strings"
	"testing"
)

func TestPerRunConfigFlowsAndDoesNotPolluteDefaults(t *testing.T) {
	temperature := 0.25
	settings := wire.ModelConfig{Temperature: &temperature, TurnSystemAdditions: []string{"single-turn skill"}, AllowedTools: []string{}}
	turn := 0
	model := modelFunc(func(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		turn++
		var prompt string
		for _, message := range request.Messages {
			if message.Role == wire.RoleSystem {
				prompt += message.Text()
			}
		}
		if turn == 1 {
			if request.Config.Temperature == nil || *request.Config.Temperature != 0.25 || len(request.Tools) != 0 || !strings.Contains(prompt, "single-turn skill") {
				t.Fatal("missing real per-run settings")
			}
			*request.Config.Temperature = 1
			request.Config.TurnSystemAdditions[0] = "mutated"
		} else {
			if request.Config.Temperature != nil || len(request.Tools) != 1 || strings.Contains(prompt, "single-turn skill") || strings.Contains(prompt, "mutated") {
				t.Fatal("per-run config leaked to defaults")
			}
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	agent := build(t, core.NewBuilder(model, core.NewMemorySession("settings")).Tool(&testTool{}).SystemPrompt("base"))
	if err := agent.RunWithConfig(context.Background(), nil, settings, discard); err != nil {
		t.Fatal(err)
	}
	if temperature != 0.25 || settings.TurnSystemAdditions[0] != "single-turn skill" {
		t.Fatal("provider mutated caller config")
	}
	if err := agent.Run(context.Background(), nil, discard); err != nil {
		t.Fatal(err)
	}
}
func TestDisabledToolCannotExecuteHallucinatedCall(t *testing.T) {
	turn, effects := 0, 0
	model := modelFunc(func(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		turn++
		if len(request.Tools) != 0 {
			t.Fatal("disabled schemas exposed")
		}
		if turn == 1 {
			if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "call", Name: "test", Arguments: json.RawMessage(`{}`)}}); err != nil {
				return err
			}
		} else {
			last := request.Messages[len(request.Messages)-1]
			if last.Content[0].ToolResult == nil || !last.Content[0].ToolResult.IsError {
				t.Fatal("hallucinated call did not recover as business feedback")
			}
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	tool := &testTool{execute: func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
		effects++
		return wire.ToolResult{}, nil
	}}
	agent := build(t, core.NewBuilder(model, core.NewMemorySession("disabled")).Tool(tool).ModelConfig(wire.ModelConfig{DisableTools: true}))
	if err := agent.Run(context.Background(), nil, discard); err != nil {
		t.Fatal(err)
	}
	if effects != 0 || turn != 2 {
		t.Fatal(effects, turn)
	}
}

func TestMalformedUTF8ArgumentsNeverAuthorizeSideEffect(t *testing.T) {
	effects := 0
	tool := &testTool{execute: func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
		effects++
		return wire.ToolResult{}, nil
	}}
	model := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		return emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "call", Name: "test", Arguments: json.RawMessage{'"', 0xff, '"'}}})
	})
	session := core.NewMemorySession("invalid-utf8")
	agent := build(t, core.NewBuilder(model, session).Tool(tool))
	if err := agent.Run(context.Background(), nil, discard); err == nil || effects != 0 {
		t.Fatal("malformed raw bytes reached execution", err, effects)
	}
	for _, record := range records(t, session) {
		if record.ToolIntent != nil {
			t.Fatal("malformed UTF-8 authorized a tool intent")
		}
	}
}
