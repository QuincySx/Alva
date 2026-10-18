// INPUT: Bounded complete native JSON events, protocol-local identities and synchronous wire consumer
// OUTPUT: Anthropic native block/thinking lifecycle, cumulative usage presence and terminal validation
// POS: [integration / protocol codec] Portable codec preserves native semantics without HTTP, credentials or cross-protocol fallback.
package codec

import (
	"context"
	"encoding/json"
	"errors"
	"sort"
	"strings"

	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type anthropicEvent struct {
	Type    string `json:"type"`
	Index   *int   `json:"index"`
	Message struct {
		ID    string `json:"id"`
		Role  string `json:"role"`
		Usage struct {
			Input      int     `json:"input_tokens"`
			Output     int     `json:"output_tokens"`
			Cached     *uint64 `json:"cache_read_input_tokens"`
			CacheWrite *uint64 `json:"cache_creation_input_tokens"`
		} `json:"usage"`
	} `json:"message"`
	Block struct {
		Type      string          `json:"type"`
		ID        string          `json:"id"`
		Name      string          `json:"name"`
		Text      string          `json:"text"`
		Input     json.RawMessage `json:"input"`
		Thinking  string          `json:"thinking"`
		Signature string          `json:"signature"`
		Data      string          `json:"data"`
	} `json:"content_block"`
	Delta struct {
		Type       string `json:"type"`
		Text       string `json:"text"`
		JSON       string `json:"partial_json"`
		StopReason string `json:"stop_reason"`
		Thinking   string `json:"thinking"`
		Signature  string `json:"signature"`
	} `json:"delta"`
	Usage struct {
		Input      *uint64 `json:"input_tokens"`
		Output     int     `json:"output_tokens"`
		Cached     *uint64 `json:"cache_read_input_tokens"`
		CacheWrite *uint64 `json:"cache_creation_input_tokens"`
	} `json:"usage"`
}
type anthropicBlock struct {
	kind, id, name string
	initial        json.RawMessage
	arguments      strings.Builder
	done           bool
	reason         wire.Reasoning
}

func newAnthropicDecoder(limits Limits) eventConsumer {
	blocks := map[int]*anthropicBlock{}
	ids := map[string]bool{}
	started, complete := false, false
	input := uint64(0)
	output := 0
	var cache, cacheWrite *uint64
	responseID := ""
	stopReason := ""
	return func(_ context.Context, incoming Event, emit func(wire.StreamChunk) error) (bool, error) {
		name, data := incoming.Name, incoming.Data
		var event anthropicEvent
		if json.Unmarshal(data, &event) != nil || event.Type == "" || (name != "" && name != event.Type) {
			return false, errors.New("invalid Anthropic SSE envelope")
		}
		switch event.Type {
		case "ping":
			return false, nil
		case "error":
			return false, errors.New("Anthropic stream reported an error")
		case "message_start":
			if started || event.Message.Role != "assistant" {
				return false, errors.New("invalid Anthropic message_start")
			}
			started = true
			responseID = event.Message.ID
			initialInput := event.Message.Usage.Input
			output = event.Message.Usage.Output
			if initialInput < 0 || output < 0 {
				return false, errors.New("invalid Anthropic initial usage")
			}
			input = uint64(initialInput)
			cache, cacheWrite = event.Message.Usage.Cached, event.Message.Usage.CacheWrite
		case "content_block_start":
			if !started || event.Index == nil || *event.Index < 0 || *event.Index >= wire.MaxStreamContentIndex || len(blocks) >= wire.MaxStreamContentBlocks || blocks[*event.Index] != nil {
				return false, errors.New("invalid Anthropic content block identity")
			}
			block := &anthropicBlock{kind: event.Block.Type, id: event.Block.ID, name: event.Block.Name, initial: append(json.RawMessage(nil), event.Block.Input...)}
			switch block.kind {
			case "text":
				if event.Block.Text != "" {
					if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: event.Block.Text, ContentIndex: contentIndex(*event.Index)}); err != nil {
						return false, err
					}
				}
			case "tool_use":
				if block.id == "" || block.name == "" || ids[block.id] {
					return false, errors.New("invalid Anthropic tool identity")
				}
				ids[block.id] = true
			case "thinking":
				block.reason = wire.Reasoning{Text: event.Block.Thinking, Signature: event.Block.Signature}
			case "redacted_thinking":
				if event.Block.Data == "" {
					return false, errors.New("empty redacted Anthropic thinking")
				}
				block.reason = wire.Reasoning{RedactedData: event.Block.Data}
			default:
				return false, errors.New("unsupported Anthropic content block; refusing to silently discard it")
			}
			blocks[*event.Index] = block
		case "content_block_delta":
			if event.Index == nil {
				return false, errors.New("Anthropic delta lacks block index")
			}
			block := blocks[*event.Index]
			if block == nil || block.done {
				return false, errors.New("unexpected Anthropic block delta")
			}
			switch event.Delta.Type {
			case "text_delta":
				if block.kind != "text" {
					return false, errors.New("Anthropic text/tool block mismatch")
				}
				return false, emit(wire.StreamChunk{Kind: wire.ChunkText, Text: event.Delta.Text, ContentIndex: contentIndex(*event.Index)})
			case "input_json_delta":
				if block.kind != "tool_use" || block.arguments.Len()+len(event.Delta.JSON) > limits.MaxArgumentBytes {
					return false, errors.New("invalid or oversized Anthropic JSON delta")
				}
				block.arguments.WriteString(event.Delta.JSON)
			case "thinking_delta":
				if block.kind != "thinking" || len(block.reason.Text)+len(event.Delta.Thinking) > limits.MaxResponseBytes {
					return false, errors.New("invalid or oversized thinking delta")
				}
				block.reason.Text += event.Delta.Thinking
				return false, emit(wire.StreamChunk{Kind: wire.ChunkReasoning, Text: event.Delta.Thinking})
			case "signature_delta":
				if block.kind != "thinking" || len(block.reason.Signature)+len(event.Delta.Signature) > limits.MaxEventBytes {
					return false, errors.New("invalid thinking signature")
				}
				block.reason.Signature += event.Delta.Signature
			default:
				return false, errors.New("unsupported Anthropic block delta")
			}
		case "content_block_stop":
			if event.Index == nil {
				return false, errors.New("Anthropic stop lacks block index")
			}
			block := blocks[*event.Index]
			if block == nil || block.done {
				return false, errors.New("unexpected Anthropic block stop")
			}
			if block.kind == "tool_use" {
				arguments := block.arguments.String()
				if arguments == "" {
					arguments = string(block.initial)
					block.arguments.WriteString(arguments)
				}
				var object map[string]json.RawMessage
				if len(arguments) > limits.MaxArgumentBytes || json.Unmarshal([]byte(arguments), &object) != nil || object == nil {
					return false, errors.New("invalid Anthropic function input")
				}
				call := wire.ToolCall{ID: block.id, Name: block.name, Arguments: json.RawMessage(arguments)}
				if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &call, ContentIndex: contentIndex(*event.Index)}); err != nil {
					return false, err
				}
			}
			block.done = true
			if block.kind == "thinking" || block.kind == "redacted_thinking" {
				if block.kind == "thinking" && block.reason.Signature == "" {
					return false, errors.New("Anthropic thinking completed without required signature")
				}
				value := wire.ContentBlock{Kind: wire.ContentReasoning, Reasoning: &block.reason}
				if err := emit(wire.StreamChunk{Kind: wire.ChunkContentBlock, ContentBlock: &value, ContentIndex: contentIndex(*event.Index)}); err != nil {
					return false, err
				}
			}
		case "message_delta":
			if !started {
				return false, errors.New("Anthropic message delta before start")
			}
			if event.Delta.StopReason != "" {
				if stopReason != "" {
					return false, errors.New("duplicate Anthropic stop reason")
				}
				stopReason = event.Delta.StopReason
			}
			output = event.Usage.Output
			if output < 0 {
				return false, errors.New("invalid Anthropic output usage")
			}
			if event.Usage.Input != nil {
				input = *event.Usage.Input
			}
			if event.Usage.Cached != nil {
				cache = event.Usage.Cached
			}
			if event.Usage.CacheWrite != nil {
				cacheWrite = event.Usage.CacheWrite
			}
		case "message_stop":
			if !started || complete || stopReason == "" {
				return false, errors.New("incomplete Anthropic message")
			}
			if stopReason != "end_turn" && stopReason != "tool_use" && stopReason != "stop_sequence" && stopReason != "refusal" {
				return false, errors.New("Anthropic message stopped incomplete or unsupported")
			}
			var indices []int
			for index, block := range blocks {
				if !block.done {
					return false, errors.New("Anthropic message ended with open block")
				}
				if block.kind == "tool_use" {
					indices = append(indices, index)
				}
			}
			if (stopReason == "tool_use") != (len(indices) > 0) {
				return false, errors.New("Anthropic stop reason disagrees with tool calls")
			}
			sort.Ints(indices)
			usage := wire.Usage{InputTokens: input, OutputTokens: uint64(max(0, output)), CachedInputTokens: cache, CacheWriteInputTokens: cacheWrite}
			for _, count := range []*uint64{cache, cacheWrite} {
				if count != nil {
					if ^uint64(0)-usage.InputTokens < *count {
						return false, errors.New("Anthropic usage overflow")
					}
					usage.InputTokens += *count
				}
			}
			if ^uint64(0)-usage.InputTokens < usage.OutputTokens {
				return false, errors.New("Anthropic usage total overflow")
			}
			usage.TotalTokens = usage.InputTokens + usage.OutputTokens
			if err := emit(wire.StreamChunk{Kind: wire.ChunkUsage, Usage: &usage}); err != nil {
				return false, err
			}
			reason := "stop"
			if len(indices) > 0 {
				reason = "tool_calls"
			}
			if err := emit(wire.StreamChunk{Kind: wire.ChunkFinish, FinishReason: reason, ResponseID: responseID}); err != nil {
				return false, err
			}
			complete = true
			return true, nil
		}
		return false, nil
	}
}
