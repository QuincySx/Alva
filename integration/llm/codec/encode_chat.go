// INPUT: encoding/json, errors/fmt and own llm/wire.
// OUTPUT: Native OpenAI Chat request encoding from the own-wire Request boundary.
// POS: [integration / protocol codec] Same-protocol chat serialization never detours through a cross-protocol canonical representation.
package codec

import (
	"encoding/json"
	"errors"
	"fmt"

	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type chatMessage struct {
	Role       string         `json:"role"`
	Content    any            `json:"content"`
	Refusal    string         `json:"refusal,omitempty"`
	Reasoning  string         `json:"reasoning_content,omitempty"`
	ToolCallID string         `json:"tool_call_id,omitempty"`
	ToolCalls  []chatToolCall `json:"tool_calls,omitempty"`
}
type chatToolCall struct {
	ID       string `json:"id"`
	Type     string `json:"type"`
	Function struct {
		Name      string `json:"name"`
		Arguments string `json:"arguments"`
	} `json:"function"`
}

func encodeChat(request Request) ([]byte, error) {
	messages := make([]chatMessage, 0, len(request.Messages))
	for _, message := range request.Messages {
		if err := message.Validate(); err != nil {
			return nil, fmt.Errorf("invalid model message: %w", err)
		}
		translated := chatMessage{Role: string(message.Role)}
		var parts []map[string]any
		for _, block := range message.Content {
			switch block.Kind {
			case wire.ContentText:
				parts = append(parts, textPart(OpenAIChat, block.Text))
			case wire.ContentReference:
				parts = append(parts, textPart(OpenAIChat, referenceText(block.Reference)))
			case wire.ContentImage, wire.ContentAudio, wire.ContentVideo:
				if message.Role != wire.RoleUser {
					return nil, errors.New("selected input media requires explicit user delivery")
				}
				media, err := encodeMedia(OpenAIChat, block)
				if err != nil {
					return nil, err
				}
				parts = append(parts, media...)
			case wire.ContentToolCall:
				if len(block.ToolCall.ProviderState) > 0 {
					return nil, errors.New("foreign provider tool state requires explicit conversion")
				}
				if message.Role != wire.RoleAssistant {
					return nil, errors.New("tool calls require assistant role")
				}
				call := chatToolCall{ID: block.ToolCall.ID, Type: "function"}
				call.Function.Name = block.ToolCall.Name
				call.Function.Arguments = string(block.ToolCall.Arguments)
				translated.ToolCalls = append(translated.ToolCalls, call)
			case wire.ContentToolResult:
				if message.Role != wire.RoleTool || len(message.Content) != 1 {
					return nil, errors.New("tool result requires a dedicated tool message")
				}
				translated.ToolCallID = block.ToolResult.CallID
				if block.ToolResult.IsError {
					parts = append(parts, textPart(OpenAIChat, "[Tool reported an error]\n"))
				}
				for _, result := range block.ToolResult.Content {
					if result.Kind == wire.ContentText {
						parts = append(parts, textPart(OpenAIChat, result.Text))
					} else if result.Kind == wire.ContentReference {
						parts = append(parts, textPart(OpenAIChat, referenceText(result.Reference)))
					} else {
						return nil, errors.New("unsupported chat tool-result content requires explicit conversion")
					}
				}
			case wire.ContentRefusal:
				if message.Role != wire.RoleAssistant {
					return nil, errors.New("refusal requires assistant role")
				}
				translated.Refusal += block.Refusal.Text
			case wire.ContentReasoning:
				if message.Role != wire.RoleAssistant || block.Reasoning.Signature != "" || block.Reasoning.RedactedData != "" {
					return nil, errors.New("foreign signed reasoning requires explicit conversion")
				}
				translated.Reasoning += block.Reasoning.Text
			default:
				return nil, errors.New("unsupported chat content requires explicit provider conversion")
			}
		}
		translated.Content = collapseChatText(parts)
		if message.Role == wire.RoleTool && translated.ToolCallID == "" {
			return nil, errors.New("tool message is missing its call identity")
		}
		messages = append(messages, translated)
	}
	tools := make([]map[string]any, 0, len(request.Tools))
	for _, definition := range request.Tools {
		if !request.Config.AllowsTool(definition.Name) {
			continue
		}
		if definition.Name == "" || !json.Valid(definition.Parameters) {
			return nil, errors.New("invalid tool definition")
		}
		tools = append(tools, map[string]any{"type": "function", "function": map[string]any{"name": definition.Name, "description": definition.Description, "parameters": definition.Parameters}})
	}
	payload := map[string]any{"model": request.Model, "messages": messages, "stream": request.Stream, "stream_options": map[string]bool{"include_usage": true}}
	if !request.Stream {
		delete(payload, "stream_options")
	}
	if len(tools) > 0 {
		payload["tools"] = tools
	}
	if err := applyModelConfig(OpenAIChat, request, request.Config, payload); err != nil {
		return nil, err
	}
	body, err := json.Marshal(payload)
	if err != nil {
		return nil, errors.New("could not encode model request")
	}
	if len(body) > responseLimit {
		return nil, errors.New("model request exceeds 8 MiB transport limit")
	}
	return body, nil
}

func referenceText(reference *wire.Reference) string {
	// Metadata is a notice, never a claim that bytes were sent or understood.
	encoded, _ := json.Marshal(reference)
	return "\n[Stored reference; raw bytes not delivered: " + string(encoded) + "]\n"
}
