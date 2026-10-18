// INPUT: encoding/json, testing
// OUTPUT: Indexed interleaving, slot uniqueness/mode/bounds and deep ownership regressions
// POS: [L0 foundation] Canonical persisted block order is independent of parallel provider arrival/completion order.
package wire

import (
	"encoding/json"
	"testing"
)

func slot(index int) *int { return &index }
func TestAccumulatorCanonicalSlotsOverrideArrivalOrder(t *testing.T) {
	accumulator := NewStreamAccumulator(8192)
	index := 2048
	chunks := []StreamChunk{{Kind: ChunkText, ContentIndex: &index, Text: "later "}, {Kind: ChunkReasoning, Text: "transient"}, {Kind: ChunkText, ContentIndex: slot(2048), Text: "text"}, {Kind: ChunkToolCall, ContentIndex: slot(1024), ToolCall: &ToolCall{ID: "call", Name: "tool", Arguments: json.RawMessage(`{}`)}}, {Kind: ChunkContentBlock, ContentIndex: slot(0), ContentBlock: &ContentBlock{Kind: ContentReasoning, Reasoning: &Reasoning{Text: "first", Signature: "sig"}}}, {Kind: ChunkUsage, Usage: &Usage{InputTokens: 1}}, {Kind: ChunkFinish}}
	for _, chunk := range chunks {
		if err := accumulator.Push(chunk); err != nil {
			t.Fatal(err)
		}
	}
	index = 99
	message, err := accumulator.Finish()
	if err != nil || len(message.Content) != 3 || message.Content[0].Reasoning == nil || message.Content[1].ToolCall == nil || message.Content[2].Text != "later text" {
		t.Fatalf("canonical order lost %#v %v", message, err)
	}
}
func TestAccumulatorIndexedModeUniquenessAndBounds(t *testing.T) {
	for _, chunks := range [][]StreamChunk{{{Kind: ChunkText, ContentIndex: slot(0)}, {Kind: ChunkText}}, {{Kind: ChunkText}, {Kind: ChunkText, ContentIndex: slot(0)}}, {{Kind: ChunkContentBlock, ContentIndex: slot(0), ContentBlock: &ContentBlock{Kind: ContentText, Text: "complete"}}, {Kind: ChunkText, ContentIndex: slot(0), Text: "duplicate"}}, {{Kind: ChunkText, ContentIndex: slot(-1)}}, {{Kind: ChunkText, ContentIndex: slot(MaxStreamContentIndex)}}} {
		accumulator := NewStreamAccumulator(8192)
		var err error
		for _, chunk := range chunks {
			err = accumulator.Push(chunk)
			if err != nil {
				break
			}
		}
		if err == nil {
			t.Fatal("invalid index/mode accepted")
		}
	}
	accumulator := NewStreamAccumulator(1 << 20)
	for index := 0; index < MaxStreamContentBlocks; index++ {
		if err := accumulator.Push(StreamChunk{Kind: ChunkText, ContentIndex: slot(index)}); err != nil {
			t.Fatal(err)
		}
	}
	if err := accumulator.Push(StreamChunk{Kind: ChunkText, ContentIndex: slot(MaxStreamContentBlocks)}); err == nil {
		t.Fatal("empty indexed chunks amplified memory beyond hard slot limit")
	}
}
func TestCloneStreamChunkOwnsIndexAndAllPayloads(t *testing.T) {
	chunk := StreamChunk{Kind: ChunkToolCall, ContentIndex: slot(1), ToolCall: &ToolCall{ID: "c", Name: "t", Arguments: json.RawMessage(`{}`)}, Usage: &Usage{InputTokens: 3}}
	clone := CloneStreamChunk(chunk)
	*clone.ContentIndex = 9
	clone.ToolCall.Arguments[0] = 'x'
	clone.Usage.InputTokens = 8
	if *chunk.ContentIndex != 1 || chunk.ToolCall.Arguments[0] != '{' || chunk.Usage.InputTokens != 3 {
		t.Fatal("shared stream chunk ownership")
	}
}
