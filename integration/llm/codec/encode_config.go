// INPUT: validated wire.ModelConfig and own explicit Request defaults
// OUTPUT: Native per-request sampling/output/thinking and explicit shallow extra-body encoding
// POS: [integration / protocol codec] Preserve existing model-family recipes without adding policy into the SDK
package codec

import (
	"encoding/json"
	"errors"
	"strconv"
	"strings"

	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func outputLimit(config Request, request wire.ModelConfig) int {
	if request.MaxTokens != nil {
		return int(*request.MaxTokens)
	}
	if config.MaxOutputTokens > 0 {
		return config.MaxOutputTokens
	}
	return 8192
}
func reasoningModel(model string) bool {
	return strings.HasPrefix(model, "gpt-5") || model == "o1" || strings.HasPrefix(model, "o1-") || strings.HasPrefix(model, "o3") || strings.HasPrefix(model, "o4")
}
func openAIEffort(model string, effort wire.ReasoningEffort) string {
	if !reasoningModel(model) || model == "o1-mini" {
		return ""
	}
	plus := strings.HasPrefix(model, "gpt-5.")
	xhigh := strings.HasPrefix(model, "gpt-5.6") || strings.Contains(model, "codex-max") || strings.HasPrefix(model, "gpt-5.2-codex") || strings.HasPrefix(model, "gpt-5.3-codex")
	switch effort {
	case wire.ReasoningNone:
		if plus {
			return "none"
		}
		return ""
	case wire.ReasoningMinimal:
		if model == "gpt-5" || strings.HasPrefix(model, "gpt-5-") {
			return "minimal"
		}
		return "low"
	case wire.ReasoningXHigh:
		if xhigh {
			return "xhigh"
		}
		return "high"
	case wire.ReasoningMax:
		if strings.HasPrefix(model, "gpt-5.6") {
			return "max"
		}
		if xhigh {
			return "xhigh"
		}
		return "high"
	default:
		return string(effort)
	}
}
func anthropicVersion(model, family string, major, minor int) bool {
	model = strings.ReplaceAll(strings.ToLower(model), ".", "-")
	_, suffix, found := strings.Cut(model, family+"-")
	if !found {
		return false
	}
	parts := strings.Split(suffix, "-")
	a, err := strconv.Atoi(parts[0])
	if err != nil {
		return false
	}
	b := 0
	if len(parts) > 1 {
		b, _ = strconv.Atoi(parts[1])
	}
	return a > major || a == major && b >= minor
}
func applyModelConfig(kind Protocol, config Request, request wire.ModelConfig, body map[string]any) error {
	if err := request.Validate(); err != nil {
		return err
	}
	maxTokens := outputLimit(config, request)
	rejectSampling := false
	if kind == Anthropic {
		lower := strings.ToLower(config.Model)
		rejectSampling = anthropicVersion(config.Model, "opus", 4, 7) || anthropicVersion(config.Model, "sonnet", 5, 0) || strings.Contains(lower, "fable") || strings.Contains(lower, "mythos")
	}
	switch kind {
	case OpenAIChat:
		field := "max_tokens"
		if reasoningModel(config.Model) {
			field = "max_completion_tokens"
		}
		body[field] = maxTokens
	case OpenAIResponses:
		body["max_output_tokens"] = maxTokens
	case Anthropic:
		body["max_tokens"] = maxTokens
	case Gemini:
		body["generationConfig"] = map[string]any{"maxOutputTokens": maxTokens}
	}
	sampling := body
	if kind == Gemini {
		sampling = body["generationConfig"].(map[string]any)
	}
	if !rejectSampling {
		if request.Temperature != nil {
			sampling["temperature"] = *request.Temperature
		}
		if request.TopP != nil {
			key := "top_p"
			if kind == Gemini {
				key = "topP"
			}
			sampling[key] = *request.TopP
		}
	}
	if len(request.StopSequences) > 0 {
		switch kind {
		case OpenAIChat:
			body["stop"] = request.StopSequences
		case OpenAIResponses:
			return errors.New("Responses protocol does not support stop sequences; use an explicitly supported configuration")
		case Anthropic:
			body["stop_sequences"] = request.StopSequences
		case Gemini:
			sampling["stopSequences"] = request.StopSequences
		}
	}
	if request.ReasoningEffort != nil {
		effort := *request.ReasoningEffort
		switch kind {
		case OpenAIChat, OpenAIResponses:
			value := openAIEffort(config.Model, effort)
			if value != "" {
				if kind == OpenAIChat {
					body["reasoning_effort"] = value
				} else {
					body["reasoning"] = map[string]any{"effort": value}
				}
			}
		case Anthropic:
			lower := strings.ToLower(config.Model)
			required := strings.Contains(lower, "fable") || strings.Contains(lower, "mythos")
			adaptive := required || anthropicVersion(config.Model, "opus", 4, 6) || anthropicVersion(config.Model, "sonnet", 4, 6)
			if effort == wire.ReasoningNone {
				typ := "disabled"
				if required {
					typ = "adaptive"
				}
				body["thinking"] = map[string]any{"type": typ}
			} else if adaptive {
				body["thinking"] = map[string]any{"type": "adaptive"}
				value := string(effort)
				if effort == wire.ReasoningMinimal {
					value = "low"
				}
				body["output_config"] = map[string]any{"effort": value}
			} else if maxTokens > 1024 {
				budget, _ := effort.SuggestedTokenBudget()
				body["thinking"] = map[string]any{"type": "enabled", "budget_tokens": max(1024, min(int(budget), maxTokens-1))}
			}
		case Gemini:
			model := config.Model
			if strings.Contains(model, "-3-") || strings.Contains(model, "-3.") {
				value := "high"
				switch effort {
				case wire.ReasoningNone, wire.ReasoningLow:
					value = "low"
				case wire.ReasoningMinimal:
					value = "minimal"
				}
				sampling["thinkingConfig"] = map[string]any{"thinkingLevel": value}
			} else if strings.Contains(model, "2.5") {
				budget, _ := effort.SuggestedTokenBudget()
				count := min(int(budget), 24576)
				if effort == wire.ReasoningNone && strings.Contains(model, "pro") {
					count = 128
				}
				sampling["thinkingConfig"] = map[string]any{"thinkingBudget": count}
			}
		}
	}
	// Explicit caller override is shallow and last-write-wins, as in the
	// existing Rust contract. The runtime still independently enforces tools.
	if len(request.ExtraBody) > 0 {
		var extra map[string]json.RawMessage
		if err := json.Unmarshal(request.ExtraBody, &extra); err != nil {
			return err
		}
		for key, value := range extra {
			body[key] = value
		}
	}
	return nil
}
