// INPUT: Actual SDK committed assistant events, effective AfterModel changes and a harmless real demo Tool
// OUTPUT: Complete engine messages are authoritative final replies, not delta concatenations or reasoning/tool prefaces
// POS: [L7 Alva product adapter] Adapter message conformance without full-history rereads, network or product-layer SDK features
package alva

import (
	"context"
	"strings"
	"testing"
	"unicode/utf8"

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

// oversizedStream streams a reasoning block and an answer in rune-safe chunks.
func oversizedStream(reasoning, text string) func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error {
	return func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		if err := emit(wire.StreamChunk{Kind: wire.ChunkContentBlock, ContentBlock: &wire.ContentBlock{Kind: wire.ContentReasoning, Reasoning: &wire.Reasoning{Text: reasoning}}}); err != nil {
			return err
		}
		for rest := text; rest != ""; {
			end := min(len(rest), 16000)
			for end < len(rest) && !utf8.RuneStart(rest[end]) {
				end--
			}
			if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: rest[:end]}); err != nil {
				return err
			}
			rest = rest[end:]
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	}
}

func finalEvents(t *testing.T, reasoning, text string) (message, block *engine.Event) {
	t.Helper()
	adapter := New(func(ctx context.Context, id string, _ engine.Request, _ bool) (harness.Runner, error) {
		return harness.NewBuilder(modelFunc(oversizedStream(reasoning, text)), core.NewMemorySession(id)).Build(ctx)
	})
	err := adapter.Execute(context.Background(), engine.Request{Prompt: "fixture"}, func(event engine.Event) error {
		switch event.Kind {
		case engine.Message:
			message = &event
		case engine.ContentBlock:
			block = &event
		}
		return nil
	})
	if err != nil || message == nil || block == nil {
		t.Fatal("final message or content block missing", err, message != nil, block != nil)
	}
	return message, block
}

func TestOversizedFinalMessageAndBlockReportTruncatedPrefix(t *testing.T) {
	// A multi-byte rune straddles each limit so the prefix must back off.
	reasoning := strings.Repeat("r", (16<<10)-1) + "思" + strings.Repeat("r", 64)
	text := strings.Repeat("a", (60<<10)-2) + "答" + strings.Repeat("a", 10<<10)
	message, block := finalEvents(t, reasoning, text)
	if !message.Truncated || message.Reference == nil || len(message.Text) > 60<<10 || !utf8.ValidString(message.Text) || !strings.HasPrefix(text, message.Text) || strings.Contains(message.Text, "[bounded]") {
		t.Fatal("oversized final message is not a marked UTF-8 prefix with its reference", message.Truncated, message.Reference, len(message.Text))
	}
	if !block.Truncated || len(block.Text) > 16<<10 || !utf8.ValidString(block.Text) || !strings.HasPrefix(reasoning, block.Text) {
		t.Fatal("oversized content block is not a marked UTF-8 prefix", block.Truncated, len(block.Text))
	}
	message, block = finalEvents(t, "short reasoning", "short answer")
	if message.Truncated || block.Truncated || message.Text != "short answer" || block.Text != "short reasoning" {
		t.Fatal("bounded content was marked truncated", message.Truncated, block.Truncated)
	}
}
