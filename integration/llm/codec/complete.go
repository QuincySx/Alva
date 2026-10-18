// INPUT: Complete native JSON responses and explicit byte/tool/argument limits.
// OUTPUT: Private native success/finish and complete-tool validation before raw forwarding or conversion.
// POS: [integration / protocol codec] Non-stream completion is proven by native status, never HTTP success or EOF alone.
package codec

import (
	"encoding/json"
	"errors"
)

func validateComplete(protocol, target Protocol, raw []byte, limits Limits) error {
	object, err := rawObject(raw, limits.MaxResponseBytes)
	if err != nil {
		return err
	}
	if value := object["error"]; len(value) > 0 && string(value) != "null" {
		return errors.New("native response reported an error")
	}
	ids := map[string]bool{}
	tool := func(id, name string, args json.RawMessage) error {
		if id == "" || name == "" || ids[id] || len(ids) >= limits.MaxToolCalls || len(args) > limits.MaxArgumentBytes {
			return errors.New("native response contains invalid tool identity or bounds")
		}
		if _, err := rawObject(args, limits.MaxArgumentBytes); err != nil {
			return errors.New("native response contains incomplete tool arguments")
		}
		ids[id] = true
		return argumentNumberGuard(protocol, target, args)
	}
	switch protocol {
	case OpenAIChat:
		var response struct {
			Choices []struct {
				Index   int
				Finish  string `json:"finish_reason"`
				Message struct {
					Role      string
					ToolCalls []struct {
						ID, Type string
						Function struct{ Name, Arguments string }
					} `json:"tool_calls"`
				}
			}
		}
		if json.Unmarshal(raw, &response) != nil || len(response.Choices) != 1 || response.Choices[0].Index != 0 || response.Choices[0].Message.Role != "assistant" {
			return errors.New("native Chat response requires one assistant choice")
		}
		choice := response.Choices[0]
		if choice.Finish != "stop" && choice.Finish != "tool_calls" {
			return errors.New("native Chat response lacks a successful finish")
		}
		if (choice.Finish == "tool_calls") != (len(choice.Message.ToolCalls) > 0) {
			return errors.New("native Chat finish contradicts tool calls")
		}
		for _, call := range choice.Message.ToolCalls {
			if call.Type != "function" {
				return errors.New("unsupported native Chat tool type")
			}
			if err := tool(call.ID, call.Function.Name, []byte(call.Function.Arguments)); err != nil {
				return err
			}
		}
	case OpenAIResponses:
		var response struct {
			Status string
			Output []struct {
				Type, ID, CallID, Name, Arguments string `json:"-"`
			}
		}
		// Keep output opaque except actual function calls; unknown native output
		// remains intact on an identity route and Magpie diagnoses cross loss.
		if json.Unmarshal(object["status"], &response.Status) != nil || response.Status != "completed" {
			return errors.New("native Responses response is not completed")
		}
		var output []map[string]json.RawMessage
		if json.Unmarshal(object["output"], &output) != nil || output == nil {
			return errors.New("native Responses output array required")
		}
		for _, item := range output {
			var kind string
			_ = json.Unmarshal(item["type"], &kind)
			if kind == "" {
				return errors.New("native Responses item type required")
			}
			if kind == "function_call" {
				var id, name, args string
				if json.Unmarshal(item["call_id"], &id) != nil || json.Unmarshal(item["name"], &name) != nil || json.Unmarshal(item["arguments"], &args) != nil {
					return errors.New("invalid native Responses call")
				}
				if err := tool(id, name, []byte(args)); err != nil {
					return err
				}
			}
		}
	case Anthropic:
		var response struct {
			Type, Role string
			Stop       string `json:"stop_reason"`
			Content    []struct {
				Type, ID, Name string
				Input          json.RawMessage
			}
		}
		if json.Unmarshal(raw, &response) != nil || response.Type != "message" || response.Role != "assistant" || response.Content == nil {
			return errors.New("native Anthropic assistant message required")
		}
		if response.Stop != "end_turn" && response.Stop != "stop_sequence" && response.Stop != "tool_use" && response.Stop != "pause_turn" {
			return errors.New("native Anthropic response lacks a successful finish")
		}
		for _, block := range response.Content {
			if block.Type == "" {
				return errors.New("native Anthropic content type required")
			}
			if block.Type == "tool_use" {
				if err := tool(block.ID, block.Name, block.Input); err != nil {
					return err
				}
			}
		}
		if (response.Stop == "tool_use") != (len(ids) > 0) {
			return errors.New("native Anthropic finish contradicts tool calls")
		}
	case Gemini:
		var response struct {
			Candidates []struct {
				Index   int
				Finish  string `json:"finishReason"`
				Content struct {
					Role  string
					Parts []struct {
						Function *struct {
							ID, Name string
							Args     json.RawMessage
						} `json:"functionCall"`
					}
				}
			}
		}
		if json.Unmarshal(raw, &response) != nil || len(response.Candidates) != 1 || response.Candidates[0].Index != 0 || response.Candidates[0].Finish != "STOP" {
			return errors.New("native Gemini response lacks one successful STOP candidate")
		}
		for i, part := range response.Candidates[0].Content.Parts {
			if part.Function != nil {
				call := part.Function
				id := call.ID
				if id == "" {
					id = string(rune(i + 1))
				}
				if err := tool(id, call.Name, call.Args); err != nil {
					return err
				}
			}
		}
	default:
		return errors.New("unknown native completion protocol")
	}
	return nil
}
