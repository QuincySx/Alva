// INPUT: context, encoding/json, testing, agent/core, kernel/abi, llm/wire
// OUTPUT: Private public-SDK consumer fixtures for extensibility probes
// POS: [architecture / executable probes] Builds real agents without importing harness defaults or changing kernel contracts.
package extensibility_test

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type modelFunc func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error

func (fn modelFunc) Stream(ctx context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	return fn(ctx, request, emit)
}

type plugin struct {
	name     string
	register func(context.Context, *core.Registrar) error
}

func (p plugin) Name() string                                          { return p.name }
func (p plugin) Register(ctx context.Context, r *core.Registrar) error { return p.register(ctx, r) }

type tool struct {
	name     string
	readOnly bool
	execute  func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error)
}

func (t tool) Name() string        { return t.name }
func (t tool) Description() string { return "isolated extensibility fixture" }
func (t tool) ParametersSchema() json.RawMessage {
	return json.RawMessage(`{"type":"object","additionalProperties":false}`)
}
func (t tool) Execute(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	return t.execute(ctx, execution, call)
}
func (t tool) Scheduling(wire.ToolCall) abi.Scheduling {
	if t.readOnly {
		return abi.Scheduling{ConcurrentSafe: true}
	}
	return abi.Scheduling{ExecutionMode: abi.SerialGlobal}
}
func (t tool) DescribeEffect(wire.ToolCall) abi.Effect {
	if t.readOnly {
		return abi.Effect{Kind: abi.EffectRead}
	}
	return abi.Effect{Kind: abi.EffectWrite}
}
func (t tool) IsReadOnly(wire.ToolCall) bool { return t.readOnly }

func build(t *testing.T, builder *core.Builder) *core.Agent {
	t.Helper()
	agent, err := builder.Build(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	return agent
}

func finishText(emit func(wire.StreamChunk) error, text string) error {
	if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: text}); err != nil {
		return err
	}
	return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
}

func requestTool(emit func(wire.StreamChunk) error, name, id string) error {
	if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: id, Name: name, Arguments: json.RawMessage(`{}`)}}); err != nil {
		return err
	}
	return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
}

func toolResult(call wire.ToolCall, text string) wire.ToolResult {
	return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: text}}}
}

func recordList(t *testing.T, store abi.SessionStore) []abi.SessionRecord {
	t.Helper()
	records, err := store.Records(context.Background(), 0)
	if err != nil {
		t.Fatal(err)
	}
	return records
}

func discard(abi.Event) error { return nil }
