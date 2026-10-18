// INPUT: Bounded complete native JSON events, protocol-local identities and synchronous wire consumer
// OUTPUT: Responses native item/text/function terminal validation with continuation-state preservation
// POS: [integration / protocol codec] Portable codec preserves native semantics without HTTP, credentials or cross-protocol fallback.
package codec

import (
	"context"
	"encoding/json"
	"errors"
	"strings"

	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type responseItem struct {
	Type      string `json:"type"`
	ID        string `json:"id"`
	CallID    string `json:"call_id"`
	Name      string `json:"name"`
	Arguments string `json:"arguments"`
	Status    string `json:"status"`
	Content   []struct {
		Type    string `json:"type"`
		Text    string `json:"text"`
		Refusal string `json:"refusal"`
	} `json:"content"`
}
type responseEvent struct {
	Type         string       `json:"type"`
	OutputIndex  int          `json:"output_index"`
	ContentIndex int          `json:"content_index"`
	ItemID       string       `json:"item_id"`
	Delta        string       `json:"delta"`
	Arguments    string       `json:"arguments"`
	Refusal      string       `json:"refusal"`
	Text         string       `json:"text"`
	Item         responseItem `json:"item"`
	Response     struct {
		ID     string          `json:"id"`
		Status string          `json:"status"`
		Error  json.RawMessage `json:"error"`
		Usage  struct {
			Input        int `json:"input_tokens"`
			Output       int `json:"output_tokens"`
			Total        int `json:"total_tokens"`
			InputDetails struct {
				Cached     *uint64 `json:"cached_tokens"`
				CacheWrite *uint64 `json:"cache_write_tokens"`
			} `json:"input_tokens_details"`
			OutputDetails struct {
				Reasoning int `json:"reasoning_tokens"`
			} `json:"output_tokens_details"`
		} `json:"usage"`
	} `json:"response"`
}
type responseCall struct {
	item      responseItem
	arguments strings.Builder
	done      bool
	argsDone  bool
}
type responseText struct {
	kind           string
	text           strings.Builder
	hasDelta, done bool
}

func newResponsesDecoder(limits Limits) eventConsumer {
	calls := map[int]*responseCall{}
	complete := false
	ids := map[string]bool{}
	texts := map[int]*responseText{}
	finalizeText := func(slot int, kind, value string, emit func(wire.StreamChunk) error) error {
		state := texts[slot]
		if state == nil {
			if len(texts) >= wire.MaxStreamContentBlocks {
				return errors.New("Responses content block count exceeds limit")
			}
			state = &responseText{kind: kind}
			texts[slot] = state
		}
		if state.kind != kind || state.hasDelta && state.text.String() != value {
			return errors.New("Responses final text disagrees with streamed content")
		}
		if state.done {
			if state.text.String() != value {
				return errors.New("Responses duplicate final text disagrees")
			}
			return nil
		}
		if len(value) > limits.MaxResponseBytes {
			return errors.New("Responses text exceeds limit")
		}
		if !state.hasDelta {
			state.text.WriteString(value)
		}
		state.done = true
		if kind == "refusal" {
			block := wire.ContentBlock{Kind: wire.ContentRefusal, Refusal: &wire.Refusal{Text: value}}
			return emit(wire.StreamChunk{Kind: wire.ChunkContentBlock, ContentBlock: &block, ContentIndex: contentIndex(slot)})
		}
		if !state.hasDelta {
			return emit(wire.StreamChunk{Kind: wire.ChunkText, Text: value, ContentIndex: contentIndex(slot)})
		}
		return nil
	}
	return func(_ context.Context, incoming Event, emit func(wire.StreamChunk) error) (bool, error) {
		name, data := incoming.Name, incoming.Data
		var event responseEvent
		if json.Unmarshal(data, &event) != nil || event.Type == "" || (name != "" && name != event.Type) {
			return false, errors.New("invalid Responses SSE envelope")
		}
		if event.OutputIndex < 0 || event.OutputIndex >= 1024 || event.ContentIndex < 0 || event.ContentIndex >= 1024 {
			return false, errors.New("Responses content index exceeds limits")
		}
		slot := event.OutputIndex*1024 + event.ContentIndex
		switch event.Type {
		case "response.output_text.delta", "response.refusal.delta":
			kind := "output_text"
			if event.Type == "response.refusal.delta" {
				kind = "refusal"
			}
			state := texts[slot]
			if state == nil {
				state = &responseText{kind: kind}
				texts[slot] = state
			}
			if state.done || state.kind != kind || state.text.Len()+len(event.Delta) > limits.MaxResponseBytes || len(texts) > 4096 {
				return false, errors.New("invalid, finalized or oversized Responses text delta")
			}
			state.hasDelta = true
			state.text.WriteString(event.Delta)
			if kind == "output_text" {
				return false, emit(wire.StreamChunk{Kind: wire.ChunkText, Text: event.Delta, ContentIndex: contentIndex(slot)})
			}
		case "response.output_text.done":
			return false, finalizeText(slot, "output_text", event.Text, emit)
		case "response.refusal.done":
			return false, finalizeText(slot, "refusal", event.Refusal, emit)
		case "response.reasoning_summary_text.delta", "response.reasoning_text.delta":
			return false, emit(wire.StreamChunk{Kind: wire.ChunkReasoning, Text: event.Delta})
		case "response.output_item.added":
			if event.Item.Type != "function_call" {
				if event.Item.Type != "message" && event.Item.Type != "reasoning" {
					return false, errors.New("unsupported Responses output item")
				}
				return false, nil
			}
			if len(calls) >= limits.MaxToolCalls || calls[event.OutputIndex] != nil || event.Item.ID == "" || event.Item.CallID == "" || event.Item.Name == "" || ids[event.Item.CallID] {
				return false, errors.New("invalid Responses function identity")
			}
			ids[event.Item.CallID] = true
			call := &responseCall{item: event.Item}
			if len(event.Item.Arguments) > limits.MaxArgumentBytes {
				return false, errors.New("Responses function arguments exceed limit")
			}
			call.arguments.WriteString(event.Item.Arguments)
			calls[event.OutputIndex] = call
		case "response.function_call_arguments.delta":
			call := calls[event.OutputIndex]
			if call == nil || call.item.ID != event.ItemID || call.done || call.argsDone {
				return false, errors.New("unexpected Responses argument delta")
			}
			if call.arguments.Len()+len(event.Delta) > limits.MaxArgumentBytes {
				return false, errors.New("Responses arguments exceed limit")
			}
			call.arguments.WriteString(event.Delta)
		case "response.function_call_arguments.done":
			call := calls[event.OutputIndex]
			if call == nil || call.item.ID != event.ItemID || call.argsDone || call.done || len(event.Arguments) > limits.MaxArgumentBytes {
				return false, errors.New("unexpected Responses argument finalization")
			}
			if call.arguments.Len() != 0 && call.arguments.String() != event.Arguments {
				return false, errors.New("Responses final arguments disagree with deltas")
			}
			if call.arguments.Len() == 0 {
				call.arguments.WriteString(event.Arguments)
			}
			if !json.Valid([]byte(event.Arguments)) {
				return false, errors.New("invalid Responses final JSON arguments")
			}
			call.argsDone = true
		case "response.output_item.done":
			if event.Item.Type == "reasoning" {
				var envelope struct {
					Item json.RawMessage `json:"item"`
				}
				if json.Unmarshal(data, &envelope) != nil {
					return false, errors.New("invalid Responses reasoning item")
				}
				value := wire.ContentBlock{Kind: wire.ContentProviderState, ProviderState: &wire.ProviderState{Provider: "openai_responses", Data: append(json.RawMessage(nil), envelope.Item...)}}
				return false, emit(wire.StreamChunk{Kind: wire.ChunkContentBlock, ContentBlock: &value, ContentIndex: contentIndex(event.OutputIndex * 1024)})
			}
			if event.Item.Type != "function_call" {
				if event.Item.Type != "message" {
					return false, errors.New("unsupported completed Responses output item")
				}
				if len(event.Item.Content) > 1024 {
					return false, errors.New("Responses message content count exceeds limit")
				}
				for index, part := range event.Item.Content {
					if part.Type != "output_text" && part.Type != "refusal" {
						return false, errors.New("unsupported Responses output message content")
					}
					text := part.Text
					if part.Type == "refusal" {
						text = part.Refusal
					}
					if err := finalizeText(event.OutputIndex*1024+index, part.Type, text, emit); err != nil {
						return false, err
					}
				}
				return false, nil
			}
			call := calls[event.OutputIndex]
			if call == nil || call.item.ID != event.Item.ID || call.item.CallID != event.Item.CallID || call.item.Name != event.Item.Name || call.done || event.Item.Status != "completed" || len(event.Item.Arguments) > limits.MaxArgumentBytes {
				return false, errors.New("invalid Responses finalized function item")
			}
			if call.arguments.Len() != 0 && call.arguments.String() != event.Item.Arguments {
				return false, errors.New("Responses final item arguments disagree")
			}
			if call.arguments.Len() == 0 {
				call.arguments.WriteString(event.Item.Arguments)
			}
			var object map[string]json.RawMessage
			if json.Unmarshal([]byte(call.arguments.String()), &object) != nil || object == nil {
				return false, errors.New("invalid Responses function arguments")
			}
			call.done = true
			value := wire.ToolCall{ID: call.item.CallID, Name: call.item.Name, Arguments: json.RawMessage(call.arguments.String())}
			if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &value, ContentIndex: contentIndex(event.OutputIndex * 1024)}); err != nil {
				return false, err
			}
		case "response.failed", "response.incomplete", "error":
			return false, errors.New("Responses model failed or ended incomplete")
		case "response.completed":
			if complete || event.Response.Status != "completed" || (len(event.Response.Error) > 0 && string(event.Response.Error) != "null") {
				return false, errors.New("invalid Responses completed outcome")
			}
			for _, call := range calls {
				if !call.done {
					return false, errors.New("Responses completed before a function item finished")
				}
			}
			for _, text := range texts {
				if !text.done {
					return false, errors.New("Responses completed before streamed text was finalized")
				}
			}
			usage := event.Response.Usage
			if usage.Input < 0 || usage.Output < 0 || usage.Total < 0 || usage.OutputDetails.Reasoning < 0 {
				return false, errors.New("invalid Responses usage")
			}
			counts := wire.Usage{InputTokens: uint64(max(0, usage.Input)), OutputTokens: uint64(max(0, usage.Output)), TotalTokens: uint64(max(0, usage.Total)), CachedInputTokens: usage.InputDetails.Cached, CacheWriteInputTokens: usage.InputDetails.CacheWrite, ReasoningTokens: uint64(max(0, usage.OutputDetails.Reasoning))}
			if err := emit(wire.StreamChunk{Kind: wire.ChunkUsage, Usage: &counts}); err != nil {
				return false, err
			}
			reason := "stop"
			if len(calls) > 0 {
				reason = "tool_calls"
			}
			if err := emit(wire.StreamChunk{Kind: wire.ChunkFinish, FinishReason: reason, ResponseID: event.Response.ID}); err != nil {
				return false, err
			}
			complete = true
			return true, nil
		default:
			if strings.HasPrefix(event.Type, "response.output_") || strings.HasPrefix(event.Type, "response.audio") {
				if event.Type != "response.output_text.annotation.added" {
					return false, errors.New("unsupported Responses output event; refusing to silently discard it")
				}
			}
		}
		return false, nil
	}
}
