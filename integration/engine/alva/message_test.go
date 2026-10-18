// INPUT: Actual SDK committed assistant events, effective AfterModel changes and a harmless real demo Tool
// OUTPUT: Complete engine messages are authoritative final replies, not delta concatenations or reasoning/tool prefaces
// POS: [L7 Alva product adapter] Adapter message conformance without full-history rereads, network or product-layer SDK features
package alva

import (
	"context"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/harness"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type effectiveAnswer struct{}

func (effectiveAnswer) Name() string { return "effective-committed-answer" }
func (effectiveAnswer) AfterModel(_ context.Context, _ abi.RuntimeContext, message *wire.Message) error {
	for _, block := range message.Content {
		if block.Kind == wire.ContentToolCall {
			return nil
		}
	}
	message.Content = []wire.ContentBlock{{Kind: wire.ContentReasoning, Reasoning: &wire.Reasoning{Text: "THINKING-NOT-ANSWER"}}, {Kind: wire.ContentText, Text: "effective actual committed answer"}}
	return nil
}

func TestCompleteMessageUsesCommittedEffectiveFactNotStreamOrToolPreface(t *testing.T) {
	var log *core.MemorySession
	adapter := New(func(ctx context.Context, id string, _ engine.Request, _ bool) (harness.Runner, error) {
		log = core.NewMemorySession(id)
		turn := 0
		tool := harness.NewDemoBuilder(log).DeclaredTools()[0]
		model := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
			turn++
			if turn == 1 {
				if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: "tool-call preface is not a result"}); err != nil {
					return err
				}
				if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "read", Name: tool.Name(), Arguments: []byte(`{"text":"harmless"}`)}}); err != nil {
					return err
				}
			} else {
				if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: "original uncommitted stream answer"}); err != nil {
					return err
				}
			}
			return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
		})
		return harness.NewBuilder(model, log).Tool(tool).Middleware(effectiveAnswer{}).Build(ctx)
	})
	complete := 0
	deltaIDs := map[string]bool{}
	err := adapter.Execute(context.Background(), engine.Request{Prompt: "fixture"}, func(event engine.Event) error {
		if event.Kind == engine.MessageDelta {
			deltaIDs[event.ID] = true
		}
		if event.Kind != engine.Message {
			return nil
		}
		complete++
		if event.Text != "effective actual committed answer" || event.Reference == nil || event.Reference.SessionID != log.ID() || strings.Contains(event.Text, "THINKING") {
			t.Fatal("not actual final plain-text fact", event)
		}
		if !deltaIDs[event.ID] {
			t.Fatal("complete message did not replace this response's stable streaming row")
		}
		records, err := log.Records(context.Background(), 0)
		if err != nil {
			t.Fatal(err)
		}
		found := false
		for _, record := range records {
			if record.ID == event.Reference.RecordID && record.Message != nil && record.Message.Text() == event.Text {
				found = true
			}
		}
		if !found {
			t.Fatal("engine complete message preceded real SDK commit")
		}
		return nil
	})
	if err != nil || complete != 1 || len(deltaIDs) != 2 {
		t.Fatal("intermediate tool-preface became result or response identities merged", complete, deltaIDs, err)
	}
}
