// INPUT: encoding/json, strings, testing
// OUTPUT: Ordered stream, reasoning continuation and bounded ownership regressions
// POS: [L0 foundation] Pins all authoritative content and provider state across accumulation and clone boundaries.
package wire

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestAccumulatorPreservesReasoningOrderAndContinuation(t *testing.T) {
	a := NewStreamAccumulator(4096)
	state := ProviderState{Provider: "gemini", ToolCallID: "c", Data: json.RawMessage(`{"thoughtSignature":"opaque"}`)}
	reasoning := ContentBlock{Kind: ContentReasoning, Reasoning: &Reasoning{Text: "private", Signature: "sig"}}
	call := ToolCall{ID: "c", Name: "tool", Arguments: json.RawMessage(`{}`), ProviderState: []ProviderState{state}}
	cached, written := uint64(2), uint64(1)
	for _, chunk := range []StreamChunk{{Kind: ChunkReasoning, Text: "private"}, {Kind: ChunkContentBlock, ContentBlock: &reasoning}, {Kind: ChunkText, Text: "one"}, {Kind: ChunkToolCall, ToolCall: &call}, {Kind: ChunkText, Text: "two"}, {Kind: ChunkUsage, Usage: &Usage{InputTokens: 3, CachedInputTokens: &cached, CacheWriteInputTokens: &written, ReasoningTokens: 4}}, {Kind: ChunkFinish, ResponseID: "response", FinishReason: "tool_use"}} {
		if err := a.Push(chunk); err != nil {
			t.Fatal(err)
		}
	}
	call.ProviderState[0].Data[0] = 'x'
	reasoning.Reasoning.Signature = "mutated"
	message, err := a.Finish()
	if err != nil {
		t.Fatal(err)
	}
	if len(message.Content) != 4 || message.Content[0].Reasoning.Signature != "sig" || message.Text() != "onetwo" || message.Content[2].ToolCall.ProviderState[0].Data[0] != '{' || message.ResponseID != "response" || message.Usage.ReasoningTokens != 4 {
		t.Fatalf("lost continuation: %#v", message)
	}
	clone := CloneMessages([]Message{message})
	clone[0].Usage.ReasoningTokens = 0
	clone[0].Content[2].ToolCall.ProviderState[0].Data[0] = 'x'
	if a.Snapshot().Usage.ReasoningTokens != 4 || a.Snapshot().Content[2].ToolCall.ProviderState[0].Data[0] != '{' {
		t.Fatal("snapshot shared bytes")
	}
}
func TestAccumulatorRejectsMalformedIncompleteDuplicateAndOverflow(t *testing.T) {
	if _, err := NewStreamAccumulator(100).Finish(); err == nil {
		t.Fatal("missing finish accepted")
	}
	a := NewStreamAccumulator(10)
	if err := a.Push(StreamChunk{Kind: ChunkText, Text: strings.Repeat("x", 20)}); err == nil {
		t.Fatal("unbounded text")
	}
	if a.Snapshot().Text() != "" {
		t.Fatal("rejected oversized content retained in partial transcript")
	}
	if err := a.Push(StreamChunk{Kind: ChunkFinish}); err == nil {
		t.Fatal("failure not sticky")
	}
	a = NewStreamAccumulator(1000)
	chunk := StreamChunk{Kind: ChunkToolCall, ToolCall: &ToolCall{ID: "c", Name: "t", Arguments: json.RawMessage(`{}`)}}
	if err := a.Push(chunk); err != nil {
		t.Fatal(err)
	}
	if err := a.Push(chunk); err == nil {
		t.Fatal("duplicate call")
	}
	for _, block := range []ContentBlock{{Kind: ContentReasoning, Reasoning: &Reasoning{Text: "text", RedactedData: "opaque"}}, {Kind: ContentProviderState, ProviderState: &ProviderState{Provider: "p", Data: json.RawMessage(`bad`)}}} {
		if err := block.Validate(); err == nil {
			t.Fatal("malformed reasoning/state accepted")
		}
	}
}

func TestAccumulatorRejectsPayloadOutsideDeclaredChunkKind(t *testing.T) {
	call := &ToolCall{ID: "hidden", Name: "tool", Arguments: json.RawMessage(`{}`)}
	block := &ContentBlock{Kind: ContentText, Text: "hidden"}
	for _, chunk := range []StreamChunk{
		{Kind: ChunkText, Text: "visible", ToolCall: call},
		{Kind: ChunkToolCall, ToolCall: call, ContentBlock: block},
		{Kind: ChunkContentBlock, ContentBlock: block, Usage: &Usage{}},
		{Kind: ChunkReasoning, Text: "progress", ToolCall: call},
		{Kind: ChunkUsage, Usage: &Usage{}, Text: "hidden"},
		{Kind: ChunkFinish, Usage: &Usage{InputTokens: 3}},
		{Kind: ChunkText, Text: "visible", ResponseID: "silently-lost"},
	} {
		a := NewStreamAccumulator(4096)
		if err := a.Push(chunk); err == nil {
			t.Errorf("ambiguous %s chunk silently discarded a payload", chunk.Kind)
		}
		if _, err := a.Finish(); err == nil {
			t.Error("invalid chunk became a complete response")
		}
	}
}
