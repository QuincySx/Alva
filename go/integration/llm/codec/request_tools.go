// INPUT: Original cross-protocol request JSON at native historical tool-call paths.
// OUTPUT: Private complete-argument validation before Magpie can normalize malformed calls.
// POS: [integration / protocol codec] Rejects argument coercion rather than silently replacing original invocation text with an empty object.
package codec

import (
	"encoding/json"
	"errors"
)

func validateRequestTools(protocol, target Protocol, raw []byte, limits Limits) error {
	var object map[string]json.RawMessage
	if json.Unmarshal(raw, &object) != nil {
		return errors.New("invalid native tool request")
	}
	count := 0
	check := func(raw json.RawMessage, quoted bool) error {
		count++
		if count > limits.MaxToolCalls {
			return errors.New("native request tool count exceeds bounds")
		}
		if quoted {
			var text string
			if string(raw) == "null" || json.Unmarshal(raw, &text) != nil {
				return errors.New("native request tool arguments must be a JSON string")
			}
			raw = []byte(text)
		}
		if _, err := rawObject(raw, limits.MaxArgumentBytes); err != nil {
			return errors.New("native request tool arguments must be a complete bounded object")
		}
		return argumentNumberGuard(protocol, target, raw)
	}
	var messages []map[string]json.RawMessage
	switch protocol {
	case OpenAIChat:
		_ = json.Unmarshal(object["messages"], &messages)
		for _, message := range messages {
			var calls []struct{ Function map[string]json.RawMessage }
			if value := message["tool_calls"]; presentState(value) {
				if json.Unmarshal(value, &calls) != nil {
					return errors.New("native Chat tool calls array required")
				}
				for _, call := range calls {
					if err := check(call.Function["arguments"], true); err != nil {
						return err
					}
				}
			}
		}
	case OpenAIResponses:
		_ = json.Unmarshal(object["input"], &messages)
		for _, item := range messages {
			var kind string
			_ = json.Unmarshal(item["type"], &kind)
			if kind == "function_call" {
				if err := check(item["arguments"], true); err != nil {
					return err
				}
			}
		}
	case Anthropic:
		_ = json.Unmarshal(object["messages"], &messages)
		for _, message := range messages {
			var blocks []map[string]json.RawMessage
			_ = json.Unmarshal(message["content"], &blocks)
			for _, block := range blocks {
				var kind string
				_ = json.Unmarshal(block["type"], &kind)
				if kind == "tool_use" {
					if err := check(block["input"], false); err != nil {
						return err
					}
				}
			}
		}
	case Gemini:
		_ = json.Unmarshal(object["contents"], &messages)
		for _, message := range messages {
			var parts []map[string]json.RawMessage
			_ = json.Unmarshal(message["parts"], &parts)
			for _, part := range parts {
				if call := part["functionCall"]; call != nil {
					var function map[string]json.RawMessage
					if json.Unmarshal(call, &function) != nil {
						return errors.New("invalid Gemini function call")
					}
					if err := check(function["args"], false); err != nil {
						return err
					}
				}
			}
		}
	}
	return nil
}
