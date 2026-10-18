// INPUT: Typed per-request configuration and existing model-family recipes
// OUTPUT: Correct output/sampling/thinking keys, shallow overrides and disabled tools regressions
// POS: [integration / protocol codec] Verify settings are honored or explicitly rejected, never silently discarded
package codec

import (
	"encoding/json"
	"testing"

	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

func TestRequestConfigurationOverridesDefaultsAndFiltersTools(t *testing.T) {
	maxTokens, temperature, topP := uint32(4096), 0.5, 0.75
	effort := wire.ReasoningHigh
	for _, kind := range []Protocol{OpenAIChat, OpenAIResponses, Anthropic, Gemini} {
		t.Run(string(kind), func(t *testing.T) {
			model := "gpt-5.6"
			if kind == Anthropic {
				model = "claude-sonnet-4-5"
			}
			if kind == Gemini {
				model = "gemini-2.5-flash"
			}
			config := Request{Model: model, MaxOutputTokens: 100}
			request := Request{Config: wire.ModelConfig{MaxTokens: &maxTokens, Temperature: &temperature, TopP: &topP, ReasoningEffort: &effort, AllowedTools: []string{}}, Tools: []wire.ToolDefinition{{Name: "hidden", Parameters: json.RawMessage(`{"type":"object"}`)}}}
			var body map[string]any
			if kind == OpenAIChat {
				data, err := encodeChatFixture(config, request)
				if err != nil {
					t.Fatal(err)
				}
				_ = json.Unmarshal(data, &body)
			} else {
				var err error
				body, err = encodeNativeFixture(kind, config, request)
				if err != nil {
					t.Fatal(err)
				}
			}
			if _, ok := body["tools"]; ok {
				t.Fatal("empty allowlist emitted tool schemas")
			}
			encoded, _ := json.Marshal(body)
			var values map[string]json.RawMessage
			_ = json.Unmarshal(encoded, &values)
			key := "max_tokens"
			if kind == OpenAIChat {
				key = "max_completion_tokens"
			}
			if kind == OpenAIResponses {
				key = "max_output_tokens"
			}
			if kind == Gemini {
				var generation map[string]json.RawMessage
				_ = json.Unmarshal(values["generationConfig"], &generation)
				values = generation
				key = "maxOutputTokens"
			}
			if string(values[key]) != "4096" || string(values["temperature"]) != "0.5" {
				t.Fatalf("request config ignored: %s", encoded)
			}
			body = map[string]any{"model": "original", "thinking": map[string]any{"type": "enabled", "budget_tokens": 2000}}
			if err := applyModelConfig(kind, config, wire.ModelConfig{ExtraBody: json.RawMessage(`{"thinking":{"type":"disabled"},"custom":{"enabled":true}}`)}, body); err != nil {
				t.Fatal(err)
			}
			encoded, _ = json.Marshal(body["thinking"])
			if string(encoded) != `{"type":"disabled"}` {
				t.Fatalf("extra_body was deep merged: %s", encoded)
			}
		})
	}
}
func TestUnsupportedResponsesStopSequencesAreNotIgnored(t *testing.T) {
	if _, err := encodeNativeFixture(OpenAIResponses, Request{Model: "fixture"}, Request{Config: wire.ModelConfig{StopSequences: []string{"stop"}}}); err == nil {
		t.Fatal("unsupported stop silently ignored")
	}
}
