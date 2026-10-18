// INPUT: Realistic mixed provider output order and malformed terminal streams
// OUTPUT: Ordered authoritative blocks and no side effects on failed stream regressions
// POS: [integration / model transport] Provider-to-kernel acceptance for complete blocks before final response validation
package provider

import (
	"context"
	"encoding/json"
	"io"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestCompletedCallsKeepTheirOriginalPosition(t *testing.T) {
	for _, test := range []struct {
		name, stream string
		parse        func(context.Context, io.Reader, int, func(wire.StreamChunk) error) error
	}{
		{"anthropic", fixtureSSE(`{"type":"message_start","message":{"role":"assistant"}}`, `{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"call","name":"fixture","input":{}}}`, `{"type":"content_block_stop","index":0}`, `{"type":"content_block_start","index":1,"content_block":{"type":"text","text":"after call"}}`, `{"type":"content_block_stop","index":1}`, `{"type":"message_delta","delta":{"stop_reason":"tool_use"}}`, `{"type":"message_stop"}`), parseAnthropicFixture},
		{"responses", fixtureSSE(`{"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","id":"item","call_id":"call","name":"fixture","arguments":""}}`, `{"type":"response.output_item.done","output_index":0,"item":{"type":"function_call","id":"item","call_id":"call","name":"fixture","arguments":"{}","status":"completed"}}`, `{"type":"response.output_text.delta","output_index":1,"delta":"after call"}`, `{"type":"response.output_text.done","output_index":1,"text":"after call"}`, `{"type":"response.completed","response":{"status":"completed"}}`), parseResponsesFixture},
		{"gemini", fixtureSSE(`{"candidates":[{"content":{"parts":[{"functionCall":{"id":"call","name":"fixture","args":{}}},{"text":"after call"}]},"finishReason":"STOP"}]}`), parseGeminiFixture},
	} {
		t.Run(test.name, func(t *testing.T) {
			acc := wire.NewStreamAccumulator(1 << 20)
			if err := test.parse(context.Background(), strings.NewReader(test.stream), 1<<20, acc.Push); err != nil {
				t.Fatal(err)
			}
			message, err := acc.Finish()
			if err != nil {
				t.Fatal(err)
			}
			if len(message.Content) != 2 || message.Content[0].Kind != wire.ContentToolCall || message.Content[1].Text != "after call" {
				t.Fatalf("provider reordered facts: %#v", message.Content)
			}
		})
	}
}
func TestUnsupportedSuccessfulOutputAndUnsignedThinkingFailExplicitly(t *testing.T) {
	for _, test := range []struct {
		name, stream string
		parse        func(context.Context, io.Reader, int, func(wire.StreamChunk) error) error
	}{
		{"gemini audio", fixtureSSE(`{"candidates":[{"content":{"parts":[{"inlineData":{"mimeType":"audio/wav","data":"YXVkaW8="}}]},"finishReason":"STOP"}]}`), parseGeminiFixture},
		{"Responses image", fixtureSSE(`{"type":"response.output_item.done","item":{"type":"image_generation_call","result":"image"}}`, `{"type":"response.completed","response":{"status":"completed"}}`), parseResponsesFixture},
		{"unsigned thinking", strings.ReplaceAll(anthropicFixture, `{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"signed"}}`, `{"type":"ping"}`), parseAnthropicFixture},
	} {
		t.Run(test.name, func(t *testing.T) {
			finished := false
			err := test.parse(context.Background(), strings.NewReader(test.stream), 1<<20, func(c wire.StreamChunk) error { finished = finished || c.Kind == wire.ChunkFinish; return nil })
			if err == nil || finished {
				t.Fatalf("lost unsupported output accepted: %v", err)
			}
		})
	}
}

type failingCallModel struct{}

func (failingCallModel) Stream(ctx context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	return parseGeminiFixture(ctx, strings.NewReader(fixtureSSE(`{"candidates":[{"content":{"parts":[{"functionCall":{"id":"call","name":"fixture","args":{}}}]}}]}`, `{"error":{"message":"private upstream failure"}}`)), 1<<20, emit)
}

type countedTool struct{ executed int }

func (*countedTool) Name() string                      { return "fixture" }
func (*countedTool) Description() string               { return "local fixture" }
func (*countedTool) ParametersSchema() json.RawMessage { return json.RawMessage(`{"type":"object"}`) }
func (t *countedTool) Execute(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
	t.executed++
	return wire.ToolResult{CallID: "call"}, nil
}
func (*countedTool) Scheduling(wire.ToolCall) abi.Scheduling { return abi.Scheduling{} }
func (*countedTool) DescribeEffect(wire.ToolCall) abi.Effect {
	return abi.Effect{Kind: abi.EffectUnknown}
}
func (*countedTool) IsReadOnly(wire.ToolCall) bool { return false }
func TestCompletedCallCannotExecuteBeforeProviderStreamSucceeds(t *testing.T) {
	tool := &countedTool{}
	session := core.NewMemorySession("fixture")
	agent, err := core.NewBuilder(failingCallModel{}, session).Tool(tool).Build(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	err = agent.Run(context.Background(), []wire.Message{wire.TextMessage(wire.RoleUser, "fixture")}, func(abi.Event) error { return nil })
	if err == nil || tool.executed != 0 {
		t.Fatalf("failed response caused side effect: %d %v", tool.executed, err)
	}
}

func TestParallelResponsesSlotsAndFinalOnlyText(t *testing.T) {
	stream := fixtureSSE(
		`{"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","id":"item","call_id":"call","name":"fixture","arguments":""}}`,
		`{"type":"response.output_text.delta","output_index":1,"content_index":0,"delta":"arrived early"}`,
		`{"type":"response.output_item.done","output_index":0,"item":{"type":"function_call","id":"item","call_id":"call","name":"fixture","arguments":"{}","status":"completed"}}`,
		`{"type":"response.output_item.done","output_index":1,"item":{"type":"message","content":[{"type":"output_text","text":"arrived early"},{"type":"output_text","text":"final-only text"}]}}`,
		`{"type":"response.completed","response":{"status":"completed"}}`,
	)
	acc := wire.NewStreamAccumulator(1 << 20)
	if err := parseResponsesFixture(context.Background(), strings.NewReader(stream), 1<<20, acc.Push); err != nil {
		t.Fatal(err)
	}
	message, err := acc.Finish()
	if err != nil {
		t.Fatal(err)
	}
	if len(message.Content) != 3 || message.Content[0].Kind != wire.ContentToolCall || message.Content[1].Text != "arrived early" || message.Content[2].Text != "final-only text" {
		t.Fatalf("parallel final order/content lost: %#v", message.Content)
	}
	bad := strings.ReplaceAll(stream, `"text":"arrived early"`, `"text":"disagrees with streamed value"`)
	if err := parseResponsesFixture(context.Background(), strings.NewReader(bad), 1<<20, func(wire.StreamChunk) error { return nil }); err == nil {
		t.Fatal("conflicting final response accepted")
	}
}
func TestInvalidUTF8IsRejectedBeforeToolArgumentsCanBeRepaired(t *testing.T) {
	bad := fixtureSSE(`{"type":"response.output_text.delta","delta":"`+string([]byte{255})+`"}`, `{"type":"response.completed","response":{"status":"completed"}}`)
	if err := parseResponsesFixture(context.Background(), strings.NewReader(bad), 1<<20, func(wire.StreamChunk) error { return nil }); err == nil {
		t.Fatal("invalid UTF-8 silently repaired")
	}
	bad = fixtureSSE(`{"choices":[{"index":0,"delta":{"content":"`+string([]byte{255})+`"},"finish_reason":"stop"}]}`, `[DONE]`)
	if err := parseChatFixture(context.Background(), strings.NewReader(bad), 1<<20, func(wire.StreamChunk) error { return nil }); err == nil {
		t.Fatal("invalid Chat UTF-8 silently repaired")
	}
}
