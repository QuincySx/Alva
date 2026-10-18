// INPUT: encoding/json, errors, math, strings
// OUTPUT: ModelConfig, ReasoningEffort, ParseReasoningEffort, CloneModelConfig
// POS: [L0 foundation] Typed per-request model settings with explicit optional values and provider-specific JSON object overrides.
package wire

import (
	"encoding/json"
	"errors"
	"math"
	"strings"
)

type ReasoningEffort string

const (
	ReasoningNone    ReasoningEffort = "none"
	ReasoningMinimal ReasoningEffort = "minimal"
	ReasoningLow     ReasoningEffort = "low"
	ReasoningMedium  ReasoningEffort = "medium"
	ReasoningHigh    ReasoningEffort = "high"
	ReasoningXHigh   ReasoningEffort = "xhigh"
	ReasoningMax     ReasoningEffort = "max"
)

func ParseReasoningEffort(value string) (ReasoningEffort, error) {
	effort := ReasoningEffort(strings.ToLower(strings.TrimSpace(value)))
	switch effort {
	case ReasoningNone, ReasoningMinimal, ReasoningLow, ReasoningMedium, ReasoningHigh, ReasoningXHigh, ReasoningMax:
		return effort, nil
	}
	return "", errors.New("unknown reasoning effort")
}
func (effort ReasoningEffort) SuggestedTokenBudget() (uint32, bool) {
	switch effort {
	case ReasoningNone:
		return 0, false
	case ReasoningMinimal:
		return 1024, true
	case ReasoningLow:
		return 2048, true
	case ReasoningMedium:
		return 8192, true
	case ReasoningHigh:
		return 16384, true
	case ReasoningXHigh:
		return 24576, true
	case ReasoningMax:
		return 32768, true
	}
	return 0, false
}

type ModelConfig struct {
	Temperature     *float64         `json:"temperature,omitempty"`
	MaxTokens       *uint32          `json:"max_tokens,omitempty"`
	StopSequences   []string         `json:"stop_sequences,omitempty"`
	TopP            *float64         `json:"top_p,omitempty"`
	ReasoningEffort *ReasoningEffort `json:"reasoning_effort,omitempty"`
	ExtraBody       json.RawMessage  `json:"extra_body,omitempty"`
	DisableTools    bool             `json:"disable_tools,omitempty"`
	// nil means unrestricted, a nonnil empty slice permits no tools.
	AllowedTools        []string `json:"allowed_tools"`
	TurnSystemAdditions []string `json:"turn_system_additions,omitempty"`
}

func (config ModelConfig) Validate() error {
	if config.Temperature != nil && (math.IsNaN(*config.Temperature) || math.IsInf(*config.Temperature, 0) || *config.Temperature < 0) {
		return errors.New("temperature must be finite and nonnegative")
	}
	if config.TopP != nil && (math.IsNaN(*config.TopP) || math.IsInf(*config.TopP, 0) || *config.TopP < 0 || *config.TopP > 1) {
		return errors.New("top_p must be finite in [0,1]")
	}
	if config.MaxTokens != nil && *config.MaxTokens == 0 {
		return errors.New("max_tokens must be positive")
	}
	if config.ReasoningEffort != nil {
		if _, err := ParseReasoningEffort(string(*config.ReasoningEffort)); err != nil {
			return err
		}
		if string(*config.ReasoningEffort) != strings.ToLower(strings.TrimSpace(string(*config.ReasoningEffort))) {
			return errors.New("reasoning effort must be canonical")
		}
	}
	if len(config.ExtraBody) > 0 {
		if !ValidJSON(config.ExtraBody) {
			return errors.New("extra_body must be valid UTF-8 JSON")
		}
		var object map[string]json.RawMessage
		if err := json.Unmarshal(config.ExtraBody, &object); err != nil || object == nil {
			return errors.New("extra_body must be a JSON object")
		}
	}
	for _, name := range config.AllowedTools {
		if name == "" || !validStrings(name) {
			return errors.New("empty allowed tool name")
		}
	}
	for _, text := range append(append([]string(nil), config.StopSequences...), config.TurnSystemAdditions...) {
		if !validStrings(text) {
			return errors.New("model configuration text must be valid UTF-8")
		}
	}
	return nil
}
func CloneModelConfig(config ModelConfig) ModelConfig {
	if config.Temperature != nil {
		value := *config.Temperature
		config.Temperature = &value
	}
	if config.MaxTokens != nil {
		value := *config.MaxTokens
		config.MaxTokens = &value
	}
	if config.TopP != nil {
		value := *config.TopP
		config.TopP = &value
	}
	if config.ReasoningEffort != nil {
		value := *config.ReasoningEffort
		config.ReasoningEffort = &value
	}
	config.StopSequences = append([]string(nil), config.StopSequences...)
	config.TurnSystemAdditions = append([]string(nil), config.TurnSystemAdditions...)
	config.ExtraBody = append(json.RawMessage(nil), config.ExtraBody...)
	if config.AllowedTools != nil {
		config.AllowedTools = append([]string{}, config.AllowedTools...)
	}
	return config
}
func (config ModelConfig) AllowsTool(name string) bool {
	if config.DisableTools {
		return false
	}
	if config.AllowedTools == nil {
		return true
	}
	for _, allowed := range config.AllowedTools {
		if allowed == name {
			return true
		}
	}
	return false
}
