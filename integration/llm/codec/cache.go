// INPUT: Original native usage JSON presence and converted response/event JSON.
// OUTPUT: Private cache counter presence projection independent of normalized Magpie numeric zero.
// POS: [integration / protocol codec] Cache telemetry distinguishes unreported/null from explicitly reported zero across conversion.
package codec

import (
	"encoding/json"
	"errors"
)

type cachePresence struct{ read, write json.RawMessage }

func usageObject(protocol Protocol, object map[string]json.RawMessage) (map[string]json.RawMessage, map[string]json.RawMessage, string, error) {
	parent := object
	key := "usage"
	if protocol == Gemini {
		key = "usageMetadata"
	}
	if protocol == Anthropic && object["message"] != nil {
		var err error
		parent, err = rawObject(object["message"], responseLimit)
		if err != nil {
			return nil, nil, "", err
		}
	}
	if protocol == OpenAIResponses && object["response"] != nil {
		var err error
		parent, err = rawObject(object["response"], responseLimit)
		if err != nil {
			return nil, nil, "", err
		}
	}
	if parent[key] == nil || string(parent[key]) == "null" {
		return nil, parent, key, nil
	}
	usage, err := rawObject(parent[key], responseLimit)
	return usage, parent, key, err
}
func (p *cachePresence) observe(protocol Protocol, raw []byte) error {
	object, err := rawObject(raw, responseLimit)
	if err != nil {
		return err
	}
	usage, _, _, err := usageObject(protocol, object)
	if err != nil || usage == nil {
		return err
	}
	var read, write json.RawMessage
	switch protocol {
	case OpenAIChat:
		var details map[string]json.RawMessage
		if value := usage["prompt_tokens_details"]; value != nil && string(value) != "null" {
			if json.Unmarshal(value, &details) != nil {
				return errors.New("invalid native cache details")
			}
		}
		read = details["cached_tokens"]
	case OpenAIResponses:
		var details map[string]json.RawMessage
		if value := usage["input_tokens_details"]; value != nil && string(value) != "null" {
			if json.Unmarshal(value, &details) != nil {
				return errors.New("invalid native cache details")
			}
		}
		read = details["cached_tokens"]
		write = details["cache_write_tokens"]
	case Anthropic:
		read = usage["cache_read_input_tokens"]
		write = usage["cache_creation_input_tokens"]
	case Gemini:
		read = usage["cachedContentTokenCount"]
	}
	for _, value := range []struct {
		raw    json.RawMessage
		target *json.RawMessage
	}{{read, &p.read}, {write, &p.write}} {
		if value.raw == nil || string(value.raw) == "null" {
			continue
		}
		var number uint64
		if json.Unmarshal(value.raw, &number) != nil {
			return errors.New("invalid native cache counter")
		}
		*value.target = append(json.RawMessage(nil), value.raw...)
	}
	return nil
}
func (p cachePresence) apply(protocol Protocol, raw json.RawMessage) (json.RawMessage, error) {
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
	if p.write != nil && protocol != Anthropic && protocol != OpenAIResponses {
		return nil, &LossError{Diagnostics: []Diagnostic{{Code: "cache_write_counter_unrepresentable", Message: "target protocol has no native cache-write counter", Severity: Warning, To: protocol}}}
	}
	set := func(container map[string]json.RawMessage, name string, value json.RawMessage) {
		if value == nil {
			delete(container, name)
		} else {
			container[name] = value
		}
	}
	switch protocol {
	case OpenAIChat, OpenAIResponses:
		detailsKey := "prompt_tokens_details"
		if protocol == OpenAIResponses {
			detailsKey = "input_tokens_details"
		}
		details := map[string]json.RawMessage{}
		if value := usage[detailsKey]; value != nil && string(value) != "null" {
			if json.Unmarshal(value, &details) != nil {
				return nil, errors.New("invalid converted cache details")
			}
		}
		set(details, "cached_tokens", p.read)
		if protocol == OpenAIResponses {
			set(details, "cache_write_tokens", p.write)
			delete(usage, "cache_write_input_tokens")
		}
		if len(details) == 0 {
			delete(usage, detailsKey)
		} else {
			usage[detailsKey], err = marshalBounded(details, responseLimit)
			if err != nil {
				return nil, err
			}
		}
	case Anthropic:
		set(usage, "cache_read_input_tokens", p.read)
		set(usage, "cache_creation_input_tokens", p.write)
	case Gemini:
		set(usage, "cachedContentTokenCount", p.read)
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
