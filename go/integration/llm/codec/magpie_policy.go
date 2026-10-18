// INPUT: Original and converted native requests plus Magpie's parsed request-local intent.
// OUTPUT: Private explicit loss checks for ignored controls, tools, schemas, reasoning budgets and message rewrites.
// POS: Alva admission keeps compatibility transformations reviewable without enabling silent fallback policies.
package codec

import (
	"bytes"
	"encoding/json"
	"reflect"
	"strings"

	"github.com/QuincySx/Alva/go/integration/llm/codec/internal/magpie"
)

func magpieLoss(from, to Protocol, path string) error {
	return &LossError{Diagnostics: []Diagnostic{{Code: "request_semantics_lost", Path: path, Message: "target protocol cannot preserve the requested semantics", Severity: Error, From: from, To: to}}}
}
func validateMagpieRequest(from, to Protocol, raw, converted []byte, source *magpie.Request) error {
	// Unknown top-level controls are never silently treated as portable. Native
	// identity routes preserve them and do not enter this cross-protocol gate.
	var object map[string]json.RawMessage
	if err := json.Unmarshal(raw, &object); err != nil {
		return err
	}
	typ := reflect.TypeOf(magpie.RequestShape(string(from))).Elem()
	fields := map[string]bool{}
	for i := 0; i < typ.NumField(); i++ {
		tag := strings.Split(typ.Field(i).Tag.Get("json"), ",")[0]
		if tag != "" && tag != "-" {
			fields[tag] = true
		}
	}
	for field := range object {
		if !fields[field] {
			return magpieLoss(from, to, field)
		}
	}
	if err := validateMagpieVariants(from, to, object); err != nil {
		return err
	}

	target, err := magpie.ParseRequest(string(to), converted)
	if err != nil {
		return conversionError(err)
	}
	seen := map[string]bool{}
	for _, tool := range source.Tools {
		if tool.Name == "" || seen[tool.Name] {
			return magpieLoss(from, to, "tools.name")
		}
		seen[tool.Name] = true
	}
	// Compare the normalized intent after real Magpie parse/build/parse. Tool
	// namespace/custom transport metadata stays on Exchange for inverse mapping.
	a, b := reflect.ValueOf(*source), reflect.ValueOf(*target)
	for i := 0; i < a.NumField(); i++ {
		field := a.Type().Field(i)
		if field.PkgPath != "" || a.Field(i).IsZero() {
			continue
		}
		switch field.Name {
		case "Model", "Stream", "Namespaced", "LastIsTurn", "GeminiCompat", "OffLevel", "Grok":
			continue
		}
		left, right := a.Field(i).Interface(), b.Field(i).Interface()
		if field.Name == "Messages" {
			left = messageIntent(source.Messages)
			right = messageIntent(target.Messages)
		}
		if !sameIntentJSON(left, right) {
			return magpieLoss(from, to, field.Name)
		}
	}
	// Budget -> effort -> budget is not exact. Preserve the original number or
	// reject it, even if the shared IR put it in the same approximate effort band.
	if value, ok := thinkingBudget(from, object); ok {
		var out map[string]json.RawMessage
		_ = json.Unmarshal(converted, &out)
		other, present := thinkingBudget(to, out)
		if !present || !bytes.Equal(bytes.TrimSpace(value), bytes.TrimSpace(other)) {
			return magpieLoss(from, to, "thinking_budget")
		}
	}
	return nil
}
func messageIntent(messages []magpie.Message) []magpie.Message {
	out := make([]magpie.Message, 0, len(messages))
	for _, message := range messages {
		message.Parts = append([]magpie.Part(nil), message.Parts...)
		for i := range message.Parts {
			if message.Parts[i].Kind == magpie.ToolResult {
				message.Parts[i].Name = ""
			}
		}
		if len(out) > 0 && out[len(out)-1].Role == message.Role {
			out[len(out)-1].Parts = append(out[len(out)-1].Parts, message.Parts...)
		} else {
			out = append(out, message)
		}
	}
	return out
}
func sameIntentJSON(left, right any) bool {
	normalize := func(value any) any {
		data, err := json.Marshal(value)
		if err != nil {
			return nil
		}
		decoder := json.NewDecoder(bytes.NewReader(data))
		decoder.UseNumber()
		var result any
		if decoder.Decode(&result) != nil {
			return nil
		}
		return result
	}
	return reflect.DeepEqual(normalize(left), normalize(right))
}
func thinkingBudget(protocol Protocol, object map[string]json.RawMessage) (json.RawMessage, bool) {
	var holder map[string]json.RawMessage
	switch protocol {
	case Anthropic:
		_ = json.Unmarshal(object["thinking"], &holder)
		value, present := holder["budget_tokens"]
		return value, present
	case Gemini:
		var config map[string]json.RawMessage
		_ = json.Unmarshal(object["generationConfig"], &config)
		_ = json.Unmarshal(config["thinkingConfig"], &holder)
		value, present := holder["thinkingBudget"]
		return value, present
	}
	return nil, false
}

func validateMagpieVariants(from, to Protocol, object map[string]json.RawMessage) error {
	var tools []map[string]json.RawMessage
	_ = json.Unmarshal(object["tools"], &tools)
	var visitTools func([]map[string]json.RawMessage) error
	visitTools = func(tools []map[string]json.RawMessage) error {
		for _, tool := range tools {
			var schema json.RawMessage
			switch from {
			case OpenAIResponses:
				schema = tool["parameters"]
			case Anthropic:
				schema = tool["input_schema"]
			case OpenAIChat:
				var function map[string]json.RawMessage
				_ = json.Unmarshal(tool["function"], &function)
				schema = function["parameters"]
			}
			if encryptedSchema(schema) {
				return magpieLoss(from, to, "tools.parameters.encrypted")
			}
			var kind string
			_ = json.Unmarshal(tool["type"], &kind)
			switch from {
			case OpenAIResponses:
				switch {
				case kind == "namespace":
					var nested []map[string]json.RawMessage
					_ = json.Unmarshal(tool["tools"], &nested)
					if err := visitTools(nested); err != nil {
						return err
					}
				case kind == "function" || kind == "custom" || strings.HasPrefix(kind, "web_search"):
					if kind == "custom" {
						var format map[string]json.RawMessage
						_ = json.Unmarshal(tool["format"], &format)
						var mode string
						_ = json.Unmarshal(format["type"], &mode)
						if mode != "" && mode != "text" {
							return magpieLoss(from, to, "tools.format")
						}
					}
				case kind == "tool_search":
					var execution string
					_ = json.Unmarshal(tool["execution"], &execution)
					if execution != "client" {
						return magpieLoss(from, to, "tools.execution")
					}
				default:
					return magpieLoss(from, to, "tools.type")
				}
			case OpenAIChat:
				if kind != "function" {
					return magpieLoss(from, to, "tools.type")
				}
			case Anthropic:
				if kind != "" && kind != "custom" && !strings.HasPrefix(kind, "web_search") {
					return magpieLoss(from, to, "tools.type")
				}
			case Gemini:
				for key := range tool {
					switch key {
					case "functionDeclarations", "googleSearch", "google_search":
					default:
						return magpieLoss(from, to, "tools."+key)
					}
				}
			}
		}
		return nil
	}
	if err := visitTools(tools); err != nil {
		return err
	}
	var content func(json.RawMessage, map[string]bool) error
	content = func(raw json.RawMessage, allowed map[string]bool) error {
		var blocks []map[string]json.RawMessage
		if json.Unmarshal(raw, &blocks) != nil {
			return nil
		}
		for _, block := range blocks {
			var kind string
			_ = json.Unmarshal(block["type"], &kind)
			if !allowed[kind] {
				return magpieLoss(from, to, "content.type")
			}
		}
		return nil
	}
	var messages []map[string]json.RawMessage
	switch from {
	case OpenAIChat, Anthropic:
		_ = json.Unmarshal(object["messages"], &messages)
		kinds := map[string]bool{"text": true, "image_url": true, "file": true}
		if from == Anthropic {
			kinds = map[string]bool{"text": true, "image": true, "tool_use": true, "tool_result": true, "thinking": true, "redacted_thinking": true}
		}
		seenConversation := false
		for _, message := range messages {
			var role string
			_ = json.Unmarshal(message["role"], &role)
			if role == "system" || role == "developer" {
				if seenConversation {
					return magpieLoss(from, to, "messages.role")
				}
			} else {
				seenConversation = true
			}
			if err := content(message["content"], kinds); err != nil {
				return err
			}
		}
	case OpenAIResponses:
		_ = json.Unmarshal(object["input"], &messages)
		for _, item := range messages {
			var kind string
			_ = json.Unmarshal(item["type"], &kind)
			if value := item["encrypted_function_args"]; presentState(value) && string(value) != "false" {
				return magpieLoss(from, to, "input.encrypted_function_args")
			}
			switch kind {
			case "message", "":
				if err := content(item["content"], map[string]bool{"input_text": true, "output_text": true, "text": true, "input_image": true}); err != nil {
					return err
				}
			case "function_call", "function_call_output", "custom_tool_call", "custom_tool_call_output", "tool_search_call", "tool_search_output", "additional_tools", "reasoning":
			default:
				return magpieLoss(from, to, "input.type")
			}
			if kind == "tool_search_output" || kind == "additional_tools" {
				var nested []map[string]json.RawMessage
				_ = json.Unmarshal(item["tools"], &nested)
				if err := visitTools(nested); err != nil {
					return err
				}
			}
		}
	}
	return nil
}

// Visit schema nodes, not literal const/enum/default business values.
func encryptedSchema(raw json.RawMessage) bool {
	var object map[string]json.RawMessage
	if json.Unmarshal(raw, &object) != nil {
		return false
	}
	if string(bytes.TrimSpace(object["encrypted"])) == "true" {
		return true
	}
	for _, key := range []string{"properties", "$defs", "definitions", "patternProperties"} {
		var children map[string]json.RawMessage
		_ = json.Unmarshal(object[key], &children)
		for _, child := range children {
			if encryptedSchema(child) {
				return true
			}
		}
	}
	for _, key := range []string{"items", "additionalProperties", "contains", "not", "if", "then", "else"} {
		if encryptedSchema(object[key]) {
			return true
		}
	}
	for _, key := range []string{"allOf", "anyOf", "oneOf", "prefixItems"} {
		var children []json.RawMessage
		_ = json.Unmarshal(object[key], &children)
		for _, child := range children {
			if encryptedSchema(child) {
				return true
			}
		}
	}
	return false
}
