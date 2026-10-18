// INPUT: encoding/json, errors/strings and own llm/wire Request values.
// OUTPUT: Native Responses, Anthropic and Gemini request encoders preserving protocol-specific continuation and block order.
// POS: [integration / protocol codec] Native request encoding is independent from Magpie cross-protocol routing and never uses Chat as a hidden intermediate.
package codec

import (
	"encoding/json"
	"errors"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
	"strings"
)

func blockText(block wire.ContentBlock) (string, error) {
	switch block.Kind {
	case wire.ContentText:
		return block.Text, nil
	case wire.ContentReference:
		return referenceText(block.Reference), nil
	default:
		return "", errors.New("unsupported text content block; media requires explicit delivery adapter")
	}
}
func encodeNative(kind Protocol, request Request) (map[string]any, error) {
	var messages []any
	var systems []string
	callNames := map[string]string{}
	for _, message := range request.Messages {
		if err := message.Validate(); err != nil {
			return nil, err
		}
		for _, block := range message.Content {
			if block.ToolCall != nil {
				callNames[block.ToolCall.ID] = block.ToolCall.Name
			}
		}
	}
	for _, message := range request.Messages {
		if message.Role == wire.RoleSystem {
			var system strings.Builder
			for _, block := range message.Content {
				text, err := blockText(block)
				if err != nil {
					return nil, err
				}
				system.WriteString(text)
			}
			systems = append(systems, system.String())
			continue
		}
		role := string(message.Role)
		if role == "tool" {
			role = "user"
		}
		if kind == Gemini && role == "assistant" {
			role = "model"
		}
		var content []map[string]any
		flush := func() {
			if len(content) > 0 {
				key := "content"
				if kind == Gemini {
					key = "parts"
				}
				messages = append(messages, map[string]any{"role": role, key: content})
				content = nil
			}
		}
		for _, block := range message.Content {
			switch block.Kind {
			case wire.ContentImage, wire.ContentAudio, wire.ContentVideo:
				if message.Role != wire.RoleUser {
					return nil, errors.New("selected input media requires explicit user delivery")
				}
				parts, err := encodeMedia(kind, block)
				if err != nil {
					return nil, err
				}
				content = append(content, parts...)
			case wire.ContentText, wire.ContentReference:
				text, err := blockText(block)
				if err != nil {
					return nil, err
				}
				switch kind {
				case OpenAIResponses:
					typ := "input_text"
					if role == "assistant" {
						typ = "output_text"
					}
					content = append(content, map[string]any{"type": typ, "text": text})
				case Anthropic:
					content = append(content, map[string]any{"type": "text", "text": text})
				case Gemini:
					content = append(content, map[string]any{"text": text})
				}
			case wire.ContentToolCall:
				if message.Role != wire.RoleAssistant {
					return nil, errors.New("tool calls require assistant role")
				}
				call := block.ToolCall
				if kind != Gemini && len(call.ProviderState) > 0 {
					return nil, errors.New("foreign provider tool state requires explicit conversion")
				}
				switch kind {
				case OpenAIResponses:
					flush()
					messages = append(messages, map[string]any{"type": "function_call", "call_id": call.ID, "name": call.Name, "arguments": string(call.Arguments)})
				case Anthropic:
					content = append(content, map[string]any{"type": "tool_use", "id": call.ID, "name": call.Name, "input": call.Arguments})
				case Gemini:
					part := map[string]any{"functionCall": map[string]any{"id": call.ID, "name": call.Name, "args": call.Arguments}}
					for _, state := range call.ProviderState {
						if state.Provider != "gemini" || state.ToolCallID != call.ID {
							return nil, errors.New("Gemini thought signature association mismatch")
						}
						var value struct {
							Signature string `json:"thoughtSignature"`
						}
						if json.Unmarshal(state.Data, &value) != nil || value.Signature == "" {
							return nil, errors.New("invalid Gemini thought signature")
						}
						part["thoughtSignature"] = value.Signature
					}
					content = append(content, part)
				}
			case wire.ContentToolResult:
				if message.Role != wire.RoleTool {
					return nil, errors.New("tool results require dedicated tool role")
				}
				result := block.ToolResult
				var text strings.Builder
				if result.IsError && kind == OpenAIResponses {
					text.WriteString("[Tool reported an error]\n")
				}
				for _, child := range result.Content {
					part, err := blockText(child)
					if err != nil {
						return nil, err
					}
					text.WriteString(part)
				}
				switch kind {
				case OpenAIResponses:
					flush()
					messages = append(messages, map[string]any{"type": "function_call_output", "call_id": result.CallID, "output": text.String()})
				case Anthropic:
					content = append(content, map[string]any{"type": "tool_result", "tool_use_id": result.CallID, "content": text.String(), "is_error": result.IsError})
				case Gemini:
					name := callNames[result.CallID]
					if name == "" {
						return nil, errors.New("Gemini tool result lacks matching function name")
					}
					content = append(content, map[string]any{"functionResponse": map[string]any{"id": result.CallID, "name": name, "response": map[string]any{"output": text.String(), "is_error": result.IsError}}})
				}
			case wire.ContentReasoning:
				if message.Role != wire.RoleAssistant {
					return nil, errors.New("reasoning requires assistant role")
				}
				reason := block.Reasoning
				switch kind {
				case Anthropic:
					if reason.RedactedData != "" {
						content = append(content, map[string]any{"type": "redacted_thinking", "data": reason.RedactedData})
					} else {
						if reason.Signature == "" {
							return nil, errors.New("Anthropic thinking signature missing")
						}
						content = append(content, map[string]any{"type": "thinking", "thinking": reason.Text, "signature": reason.Signature})
					}
				case Gemini:
					if reason.RedactedData != "" {
						return nil, errors.New("foreign redacted reasoning requires conversion")
					}
					part := map[string]any{"text": reason.Text, "thought": true}
					if reason.Signature != "" {
						part["thoughtSignature"] = reason.Signature
					}
					content = append(content, part)
				default:
					return nil, errors.New("foreign reasoning requires explicit Responses conversion")
				}
			case wire.ContentProviderState:
				state := block.ProviderState
				if kind != OpenAIResponses || state.Provider != "openai_responses" || message.Role != wire.RoleAssistant {
					return nil, errors.New("provider state does not belong to this protocol")
				}
				item, err := rawObject(state.Data, responseLimit)
				var itemType string
				if err != nil || json.Unmarshal(item["type"], &itemType) != nil || itemType != "reasoning" {
					return nil, errors.New("invalid Responses reasoning state")
				}
				flush()
				messages = append(messages, append(json.RawMessage(nil), state.Data...))
			case wire.ContentRefusal:
				if message.Role != wire.RoleAssistant {
					return nil, errors.New("refusal requires assistant role")
				}
				switch kind {
				case OpenAIResponses:
					content = append(content, map[string]any{"type": "refusal", "refusal": block.Refusal.Text})
				case Anthropic:
					content = append(content, map[string]any{"type": "text", "text": "[Refusal] " + block.Refusal.Text})
				case Gemini:
					content = append(content, map[string]any{"text": "[Refusal] " + block.Refusal.Text})
				}
			default:
				return nil, errors.New("unsupported model content; explicit adapter required")
			}
		}
		flush()
	}
	var tools []map[string]any
	for _, tool := range request.Tools {
		if !request.Config.AllowsTool(tool.Name) {
			continue
		}
		var schema map[string]any
		if tool.Name == "" || json.Unmarshal(tool.Parameters, &schema) != nil || schema == nil {
			return nil, errors.New("invalid model tool schema")
		}
		switch kind {
		case OpenAIResponses:
			tools = append(tools, map[string]any{"type": "function", "name": tool.Name, "description": tool.Description, "parameters": tool.Parameters})
		case Anthropic:
			tools = append(tools, map[string]any{"name": tool.Name, "description": tool.Description, "input_schema": tool.Parameters})
		case Gemini:
			tools = append(tools, map[string]any{"name": tool.Name, "description": tool.Description, "parameters": tool.Parameters})
		}
	}
	max := request.MaxOutputTokens
	if max == 0 {
		max = 8192
	}
	var payload map[string]any
	switch kind {
	case OpenAIResponses:
		payload = map[string]any{"model": request.Model, "input": messages, "stream": request.Stream, "store": false, "max_output_tokens": max}
		if len(systems) > 0 {
			payload["instructions"] = strings.Join(systems, "\n\n")
		}
		if len(tools) > 0 {
			payload["tools"] = tools
		}
	case Anthropic:
		payload = map[string]any{"model": request.Model, "messages": messages, "stream": request.Stream, "max_tokens": max}
		if len(systems) > 0 {
			var system []map[string]string
			for _, text := range systems {
				system = append(system, map[string]string{"type": "text", "text": text})
			}
			payload["system"] = system
		}
		if len(tools) > 0 {
			payload["tools"] = tools
		}
	case Gemini:
		payload = map[string]any{"contents": messages, "generationConfig": map[string]int{"maxOutputTokens": max}}
		if len(systems) > 0 {
			payload["systemInstruction"] = map[string]any{"parts": []map[string]string{{"text": strings.Join(systems, "\n\n")}}}
		}
		if len(tools) > 0 {
			payload["tools"] = []map[string]any{{"functionDeclarations": tools}}
		}
	default:
		return nil, errors.New("unsupported protocol serializer")
	}
	if err := applyModelConfig(kind, request, request.Config, payload); err != nil {
		return nil, err
	}
	return payload, nil
}
