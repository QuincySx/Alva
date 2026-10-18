// INPUT: Native request/response JSON at explicit protocol-owned continuation paths.
// OUTPUT: Private conservative cross-protocol continuation-state loss rejection.
// POS: [integration / protocol codec] Covers native signed/encrypted state that cross-protocol conversion cannot preserve.
package codec

import "encoding/json"

func continuationGuard(from, to Protocol, raw []byte) error {
	if from == to {
		return nil
	}
	var object map[string]json.RawMessage
	if json.Unmarshal(raw, &object) != nil {
		return nil
	} // strict shape validation is a separate gate.
	path := ""
	refusalPath := ""
	set := func(candidate string, value json.RawMessage) {
		if path == "" && presentState(value) {
			path = candidate
		}
	}
	var visitBlocks func(json.RawMessage, string)
	visitBlocks = func(raw json.RawMessage, prefix string) {
		var blocks []map[string]json.RawMessage
		if json.Unmarshal(raw, &blocks) != nil {
			return
		}
		for _, block := range blocks {
			var kind string
			_ = json.Unmarshal(block["type"], &kind)
			switch kind {
			case "thinking":
				set(prefix+".signature", block["signature"])
			case "redacted_thinking":
				set(prefix+".data", block["data"])
			case "reasoning", "compaction":
				set(prefix+".encrypted_content", block["encrypted_content"])
			case "refusal":
				refusalPath = prefix
			}
			if kind == "message" {
				visitBlocks(block["content"], prefix+".content")
			}
		}
	}
	switch from {
	case Anthropic:
		visitBlocks(object["content"], "content")
		var messages []map[string]json.RawMessage
		_ = json.Unmarshal(object["messages"], &messages)
		for _, message := range messages {
			visitBlocks(message["content"], "messages.content")
		}
		if block := object["content_block"]; block != nil {
			visitBlocks(append(append([]byte{'['}, block...), ']'), "content_block")
		}
		var delta map[string]json.RawMessage
		_ = json.Unmarshal(object["delta"], &delta)
		var kind string
		_ = json.Unmarshal(delta["type"], &kind)
		if kind == "signature_delta" {
			set("delta.signature", delta["signature"])
		}
		var stop string
		_ = json.Unmarshal(object["stop_reason"], &stop)
		if stop == "pause_turn" {
			path = "stop_reason"
		}
		_ = json.Unmarshal(delta["stop_reason"], &stop)
		if stop == "pause_turn" {
			path = "delta.stop_reason"
		}
	case OpenAIResponses:
		var eventType string
		_ = json.Unmarshal(object["type"], &eventType)
		if eventType == "response.refusal.delta" || eventType == "response.refusal.done" {
			refusalPath = eventType
		}
		set("previous_response_id", object["previous_response_id"])
		set("conversation", object["conversation"])
		visitBlocks(object["input"], "input")
		visitBlocks(object["output"], "output")
		if item := object["item"]; item != nil {
			visitBlocks(append(append([]byte{'['}, item...), ']'), "item")
		}
		var response map[string]json.RawMessage
		_ = json.Unmarshal(object["response"], &response)
		visitBlocks(response["output"], "response.output")
	case Gemini:
		var contents []map[string]json.RawMessage
		_ = json.Unmarshal(object["contents"], &contents)
		var candidates []struct{ Content map[string]json.RawMessage }
		_ = json.Unmarshal(object["candidates"], &candidates)
		for _, candidate := range candidates {
			contents = append(contents, candidate.Content)
		}
		for _, content := range contents {
			var parts []map[string]json.RawMessage
			_ = json.Unmarshal(content["parts"], &parts)
			for _, part := range parts {
				set("content.parts.thoughtSignature", part["thoughtSignature"])
			}
		}
	case OpenAIChat:
		var messages []map[string]json.RawMessage
		_ = json.Unmarshal(object["messages"], &messages)
		var choices []struct{ Message, Delta map[string]json.RawMessage }
		_ = json.Unmarshal(object["choices"], &choices)
		for _, choice := range choices {
			messages = append(messages, choice.Message, choice.Delta)
		}
		for _, message := range messages {
			set("messages.reasoning_details", message["reasoning_details"])
			if presentState(message["refusal"]) {
				refusalPath = "choices.message.refusal"
			}
		}
	}
	if path != "" {
		return &LossError{Diagnostics: []Diagnostic{{Code: "continuation_state_lost", Path: path, Message: "native signed or encrypted continuation cannot be preserved across these protocols", Severity: Error, From: from, To: to}}}
	}
	if refusalPath != "" {
		return &LossError{Diagnostics: []Diagnostic{{Code: "refusal_state_lost", Path: refusalPath, Message: "structured refusal cannot be preserved by this converter", Severity: Error, From: from, To: to}}}
	}
	return nil
}
func presentState(raw json.RawMessage) bool {
	if len(raw) == 0 {
		return false
	}
	var value any
	if json.Unmarshal(raw, &value) != nil {
		return true
	}
	switch value := value.(type) {
	case nil:
		return false
	case string:
		return value != ""
	case []any:
		return len(value) > 0
	case map[string]any:
		return len(value) > 0
	default:
		return true
	}
}
