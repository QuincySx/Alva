// INPUT: Actual native-decoded final usage and held converted completion events.
// OUTPUT: Private native usage projection with exact cumulative counts and cache-inclusive input translation.
// POS: [integration / protocol codec] Late usage-only frames remain authoritative even when a third-party converter already marked its target done.
package codec

import (
	"encoding/json"
	"errors"

	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

func projectUsage(protocol Protocol, raw json.RawMessage, source wire.Usage) (json.RawMessage, error) {
	object, err := rawObject(raw, responseLimit)
	if err != nil {
		return nil, err
	}
	usage, parent, key, err := usageObject(protocol, object)
	if err != nil {
		return nil, err
	}
	if usage == nil {
		return raw, nil
	}
	put := func(name string, value uint64) { usage[name], _ = json.Marshal(value) }
	switch protocol {
	case OpenAIChat:
		put("prompt_tokens", source.InputTokens)
		put("completion_tokens", source.OutputTokens)
		put("total_tokens", source.TotalTokens)
		if err := putReasoningUsage(usage, "completion_tokens_details", source.ReasoningTokens); err != nil {
			return nil, err
		}
	case OpenAIResponses:
		put("input_tokens", source.InputTokens)
		put("output_tokens", source.OutputTokens)
		put("total_tokens", source.TotalTokens)
		if err := putReasoningUsage(usage, "output_tokens_details", source.ReasoningTokens); err != nil {
			return nil, err
		}
	case Gemini:
		if source.ReasoningTokens > source.OutputTokens {
			return nil, errors.New("reasoning usage exceeds inclusive output usage")
		}
		put("promptTokenCount", source.InputTokens)
		put("candidatesTokenCount", source.OutputTokens-source.ReasoningTokens)
		put("totalTokenCount", source.TotalTokens)
		put("thoughtsTokenCount", source.ReasoningTokens)
		delete(usage, "toolUsePromptTokenCount") // no native split is known for a foreign source.
		delete(usage, "toolUsePromptTokensDetails")
	case Anthropic:
		input := source.InputTokens
		for _, count := range []*uint64{source.CachedInputTokens, source.CacheWriteInputTokens} {
			if count != nil {
				if *count > input {
					return nil, errors.New("native cache counts exceed total input usage")
				}
				input -= *count
			}
		}
		put("input_tokens", input)
		put("output_tokens", source.OutputTokens)
	}
	parent[key], err = marshalBounded(usage, responseLimit)
	if err != nil {
		return nil, err
	}
	if protocol == Anthropic && object["message"] != nil {
		object["message"], err = marshalBounded(parent, responseLimit)
	}
	if protocol == OpenAIResponses && object["response"] != nil {
		object["response"], err = marshalBounded(parent, responseLimit)
	}
	if err != nil {
		return nil, err
	}
	return marshalBounded(object, responseLimit)
}
func putReasoningUsage(usage map[string]json.RawMessage, key string, count uint64) error {
	details := map[string]json.RawMessage{}
	if raw := usage[key]; raw != nil && string(raw) != "null" {
		if json.Unmarshal(raw, &details) != nil || details == nil {
			return errors.New("invalid converted output usage details")
		}
	}
	details["reasoning_tokens"], _ = json.Marshal(count)
	raw, err := marshalBounded(details, responseLimit)
	if err != nil {
		return err
	}
	usage[key] = raw
	return nil
}
