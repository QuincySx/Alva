// INPUT: context, encoding/json, kernel/abi, llm/wire.
// OUTPUT: Deterministic local model and harmless echo Tool used only by NewDemo.
// POS: [L5 harness] Real model-to-tool-to-model contract exercise without network or shell.
package harness

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"

	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type demoModel struct{}

func (demoModel) Stream(ctx context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if len(request.Messages) == 0 {
		return errors.New("demo requires a user message")
	}
	last := request.Messages[len(request.Messages)-1]
	if last.Role == wire.RoleTool {
		var result string
		for _, block := range last.Content {
			if block.ToolResult != nil {
				for _, content := range block.ToolResult.Content {
					if content.Kind == wire.ContentText {
						result += content.Text
					} else if content.Kind == wire.ContentReference {
						result += "[stored reference; raw result not delivered]"
					}
				}
			}
		}
		if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: "[Local demo, not a live model] Tool result: " + result}); err != nil {
			return err
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish, FinishReason: "stop"})
	}
	if last.Role != wire.RoleUser {
		return errors.New("demo expects a user message or tool result")
	}
	arguments, _ := json.Marshal(map[string]string{"text": last.Text()})
	turn := 0
	for _, message := range request.Messages {
		if message.Role == wire.RoleUser {
			turn++
		}
	}
	if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: fmt.Sprintf("demo-call-%d", turn), Name: "demo_echo", Arguments: arguments}}); err != nil {
		return err
	}
	return emit(wire.StreamChunk{Kind: wire.ChunkFinish, FinishReason: "tool_calls"})
}

type echoTool struct{}

func (echoTool) Name() string { return "demo_echo" }
func (echoTool) Description() string {
	return "Local demo fixture: return supplied text; no filesystem, network, or shell access."
}
func (echoTool) ParametersSchema() json.RawMessage {
	return json.RawMessage(`{"type":"object","properties":{"text":{"type":"string","maxLength":4096}},"required":["text"],"additionalProperties":false}`)
}
func (echoTool) Execute(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	if err := ctx.Err(); err != nil {
		return wire.ToolResult{}, err
	}
	var arguments struct {
		Text string `json:"text"`
	}
	if err := json.Unmarshal(call.Arguments, &arguments); err != nil {
		return wire.ToolResult{}, errors.New("invalid echo arguments")
	}
	if len(arguments.Text) > 4096 {
		return wire.ToolResult{}, errors.New("demo echo text exceeds 4096 bytes")
	}
	return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: arguments.Text}}}, nil
}
func (echoTool) Scheduling(wire.ToolCall) abi.Scheduling { return abi.Scheduling{ConcurrentSafe: true} }
func (echoTool) DescribeEffect(wire.ToolCall) abi.Effect {
	return abi.Effect{Kind: abi.EffectRead, Description: "Read supplied demo input only"}
}
func (echoTool) IsReadOnly(wire.ToolCall) bool { return true }
