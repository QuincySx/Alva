// INPUT: Bounded OpenAI Chat native delta events and explicit [DONE] delimiter
// OUTPUT: Complete indexed calls/refusal/reasoning and held finish after actual normal terminal
// POS: [integration / protocol codec] Native pure codec preserves original block order without converting through another protocol.
package codec

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
	"strings"
)

type responseChunk struct {
	ID      string `json:"id"`
	Choices []struct {
		Index int `json:"index"`
		Delta struct {
			Content        string          `json:"content"`
			Refusal        string          `json:"refusal"`
			Reasoning      string          `json:"reasoning_content"`
			Audio          json.RawMessage `json:"audio"`
			LegacyFunction json.RawMessage `json:"function_call"`
			ToolCalls      []struct {
				Index    int    `json:"index"`
				ID       string `json:"id"`
				Type     string `json:"type"`
				Function struct {
					Name      string `json:"name"`
					Arguments string `json:"arguments"`
				} `json:"function"`
			} `json:"tool_calls"`
		} `json:"delta"`
		FinishReason *string `json:"finish_reason"`
	} `json:"choices"`
	Usage *struct {
		PromptTokens     int `json:"prompt_tokens"`
		CompletionTokens int `json:"completion_tokens"`
		TotalTokens      int `json:"total_tokens"`
		PromptDetails    struct {
			Cached *uint64 `json:"cached_tokens"`
		} `json:"prompt_tokens_details"`
		CompletionDetails struct {
			Reasoning int `json:"reasoning_tokens"`
		} `json:"completion_tokens_details"`
	} `json:"usage"`
	Error json.RawMessage `json:"error"`
}

func newChatDecoder(limits Limits) eventConsumer {
	calls := make(map[int]*wire.ToolCall)
	callSlots := map[int]int{}
	nextSlot := 0
	textSlot, refusalSlot, reasoningSlot := -1, -1, -1
	reserve := func(slot *int) int {
		if *slot < 0 {
			*slot = nextSlot
			nextSlot++
		}
		return *slot
	}
	finished := false
	finishReason := ""
	responseID := ""
	var refusal, reasoning strings.Builder
	finish := func(emit func(wire.StreamChunk) error) error {
		if !finished {
			return errors.New("model stream ended without a finish reason")
		}
		if finishReason == "tool_calls" && len(calls) == 0 {
			return errors.New("model finished with tool_calls but supplied no tool call")
		}
		if finishReason == "stop" && len(calls) > 0 {
			return errors.New("model finished with stop despite supplying tool calls")
		}
		for index := 0; index < len(calls); index++ {
			call, ok := calls[index]
			if !ok || call.ID == "" || call.Name == "" || !json.Valid(call.Arguments) {
				return errors.New("model stream contains an incomplete tool call")
			}
			copyCall := *call
			if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &copyCall, ContentIndex: contentIndex(callSlots[index])}); err != nil {
				return err
			}
		}
		if refusal.Len() > 0 {
			block := wire.ContentBlock{Kind: wire.ContentRefusal, Refusal: &wire.Refusal{Text: refusal.String()}}
			if err := emit(wire.StreamChunk{Kind: wire.ChunkContentBlock, ContentBlock: &block, ContentIndex: contentIndex(refusalSlot)}); err != nil {
				return err
			}
		}
		if reasoning.Len() > 0 {
			block := wire.ContentBlock{Kind: wire.ContentReasoning, Reasoning: &wire.Reasoning{Text: reasoning.String()}}
			if err := emit(wire.StreamChunk{Kind: wire.ChunkContentBlock, ContentBlock: &block, ContentIndex: contentIndex(reasoningSlot)}); err != nil {
				return err
			}
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish, FinishReason: finishReason, ResponseID: responseID})
	}
	return func(_ context.Context, incoming Event, emit func(wire.StreamChunk) error) (bool, error) {
		data := string(incoming.Data)
		if data == "[DONE]" {
			return true, finish(emit)
		}
		if !wire.ValidJSON([]byte(data)) {
			return false, errors.New("invalid model JSON/Unicode")
		}
		var chunk responseChunk
		if err := json.Unmarshal([]byte(data), &chunk); err != nil {
			return false, errors.New("invalid model SSE JSON")
		}
		if len(chunk.Error) > 0 && string(chunk.Error) != "null" {
			return false, errors.New("model stream reported an error")
		}
		if len(chunk.Choices) > 1 {
			return false, errors.New("multiple model choices are unsupported")
		}
		if chunk.ID != "" {
			if responseID != "" && responseID != chunk.ID {
				return false, errors.New("model response identity changed mid-stream")
			}
			responseID = chunk.ID
		}
		for _, choice := range chunk.Choices {
			if choice.Index != 0 {
				return false, errors.New("multiple model choices are unsupported")
			}
			if len(choice.Delta.Audio) > 0 || len(choice.Delta.LegacyFunction) > 0 {
				return false, errors.New("unsupported native Chat output payload; refusing to silently discard it")
			}
			if choice.Delta.Refusal != "" || choice.Delta.Reasoning != "" {
				if finished {
					return false, errors.New("model content followed its finish reason")
				}
				refusal.WriteString(choice.Delta.Refusal)
				reasoning.WriteString(choice.Delta.Reasoning)
				if choice.Delta.Refusal != "" {
					reserve(&refusalSlot)
				}
				if choice.Delta.Reasoning != "" {
					reserve(&reasoningSlot)
					if err := emit(wire.StreamChunk{Kind: wire.ChunkReasoning, Text: choice.Delta.Reasoning}); err != nil {
						return false, err
					}
				}
			}
			if choice.Delta.Content != "" {
				if finished {
					return false, errors.New("model content followed its finish reason")
				}
				if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: choice.Delta.Content, ContentIndex: contentIndex(reserve(&textSlot))}); err != nil {
					return false, err
				}
			}
			for _, delta := range choice.Delta.ToolCalls {
				if finished || delta.Index < 0 || delta.Index >= limits.MaxToolCalls {
					return false, errors.New("invalid model tool-call delta")
				}
				call := calls[delta.Index]
				if call == nil {
					call = &wire.ToolCall{}
					calls[delta.Index] = call
					callSlots[delta.Index] = nextSlot
					nextSlot++
				}
				if delta.Type != "" && delta.Type != "function" {
					return false, errors.New("unsupported model tool-call type")
				}
				if delta.ID != "" {
					if len(delta.ID) > 256 {
						return false, errors.New("model tool-call ID limit exceeded")
					}
					if call.ID != "" && call.ID != delta.ID {
						return false, errors.New("conflicting model tool-call IDs")
					}
					call.ID = delta.ID
				}
				if len(call.Name)+len(delta.Function.Name) > 256 || len(call.Arguments)+len(delta.Function.Arguments) > limits.MaxArgumentBytes {
					return false, errors.New("model tool-call size limit exceeded")
				}
				call.Name += delta.Function.Name
				call.Arguments = append(call.Arguments, delta.Function.Arguments...)
			}
			if choice.FinishReason != nil {
				if finished {
					return false, errors.New("model stream supplied multiple finish reasons")
				}
				switch *choice.FinishReason {
				case "stop", "tool_calls":
					finished = true
					finishReason = *choice.FinishReason
				default:
					return false, errors.New("model response did not complete normally")
				}
			}
		}
		if chunk.Usage != nil {
			if chunk.Usage.PromptTokens < 0 || chunk.Usage.CompletionTokens < 0 || chunk.Usage.TotalTokens < 0 || chunk.Usage.CompletionDetails.Reasoning < 0 {
				return false, errors.New("invalid model usage")
			}
			u := chunk.Usage
			usage := wire.Usage{InputTokens: uint64(u.PromptTokens), OutputTokens: uint64(u.CompletionTokens), TotalTokens: uint64(max(0, u.TotalTokens)), CachedInputTokens: u.PromptDetails.Cached, ReasoningTokens: uint64(max(0, u.CompletionDetails.Reasoning))}
			if err := emit(wire.StreamChunk{Kind: wire.ChunkUsage, Usage: &usage}); err != nil {
				return false, err
			}
		}
		return false, nil
	}
}
