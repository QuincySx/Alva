// INPUT: encoding/json, math, reflect, testing
// OUTPUT: Model config optional/allowlist ownership and reasoning validation regressions
// POS: [L0 foundation] Pins per-request typed settings without transport defaults or ambiguous empty tool permissions.
package wire

import (
	"encoding/json"
	"math"
	"reflect"
	"testing"
)

func TestModelConfigCloneRoundTripAndOptionalToolList(t *testing.T) {
	temperature, topP := 0.25, 0.9
	maxTokens := uint32(4000)
	effort := ReasoningHigh
	config := ModelConfig{Temperature: &temperature, TopP: &topP, MaxTokens: &maxTokens, ReasoningEffort: &effort, ExtraBody: json.RawMessage(`{"option":true}`), AllowedTools: []string{}, StopSequences: []string{"stop"}, TurnSystemAdditions: []string{"skill"}}
	if err := config.Validate(); err != nil {
		t.Fatal(err)
	}
	clone := CloneModelConfig(config)
	*clone.Temperature = 1
	clone.ExtraBody[0] = 'x'
	clone.StopSequences[0] = "mutated"
	if *config.Temperature != 0.25 || config.ExtraBody[0] != '{' || config.StopSequences[0] != "stop" || clone.AllowedTools == nil {
		t.Fatal("config clone lost ownership/empty allowlist")
	}
	encoded, _ := json.Marshal(config)
	var decoded ModelConfig
	if err := json.Unmarshal(encoded, &decoded); err != nil || !reflect.DeepEqual(config, decoded) {
		t.Fatal(decoded, err)
	}
	if config.AllowsTool("x") || !(ModelConfig{}).AllowsTool("x") {
		t.Fatal("nil/empty allowlist conflated")
	}
}
func TestModelConfigRejectsInvalidNumbersAndOverrides(t *testing.T) {
	bad := math.NaN()
	zero := uint32(0)
	for _, config := range []ModelConfig{{Temperature: &bad}, {MaxTokens: &zero}, {ExtraBody: json.RawMessage(`[]`)}, {AllowedTools: []string{""}}} {
		if config.Validate() == nil {
			t.Fatal("invalid config accepted")
		}
	}
	effort, err := ParseReasoningEffort(" High ")
	if err != nil || effort != ReasoningHigh {
		t.Fatal(effort, err)
	}
	if budget, ok := effort.SuggestedTokenBudget(); !ok || budget != 16384 {
		t.Fatal(budget, ok)
	}
}
