// INPUT: Validated native JSON, private Magpie conversion types and explicit own protocols.
// OUTPUT: Private fixed errors, conversion evidence, bounded JSON and model/provenance projection.
// POS: Protocol conversion helpers keep the absorbed Magpie fork outside the public SDK.
package codec

import (
	"encoding/json"
	"errors"
	"github.com/QuincySx/Alva/sdk/llm/wire"
	"strings"
)

type converterError struct{ cause error }

func (*converterError) Error() string   { return "native protocol conversion failed" }
func (e *converterError) Unwrap() error { return e.cause }
func guarded[T any](f func() (T, error)) (result T, err error) {
	defer func() {
		if recover() != nil {
			err = errors.New("protocol converter rejected unsupported data shape")
		}
	}()
	return f()
}
func conversionError(err error) error {
	var loss *LossError
	if errors.As(err, &loss) {
		return loss
	}
	return &converterError{cause: err}
}
func conversionResult(from, to Protocol) Result {
	return Result{From: from, To: to, Converter: "magpie", Quality: "adapted", Steps: []Step{{Converter: "magpie.parse", From: from, To: "magpie_ir"}, {Converter: "magpie.build", From: "magpie_ir", To: to}}}
}
func foreignModel(raw json.RawMessage, protocol Protocol, model string) (json.RawMessage, error) {
	if protocol != Gemini {
		return patchModel(raw, protocol, model, true)
	}
	object, err := rawObject(raw, responseLimit)
	if err != nil {
		return nil, err
	}
	delete(object, "modelVersion")
	return marshalBounded(object, responseLimit)
}
func rawObject(raw []byte, maximum int) (map[string]json.RawMessage, error) {
	if err := validateNativeJSON(raw, maximum); err != nil {
		return nil, err
	}
	var object map[string]json.RawMessage
	if json.Unmarshal(raw, &object) != nil || object == nil {
		return nil, errors.New("native JSON object required")
	}
	return object, nil
}
func marshalBounded(value any, maximum int) (json.RawMessage, error) {
	raw, err := json.Marshal(value)
	if err != nil || len(raw) > maximum || !wire.ValidJSON(raw) {
		return nil, errors.New("converted JSON exceeds bounds or is invalid")
	}
	return raw, nil
}
func patchModel(raw json.RawMessage, protocol Protocol, model string, response bool) (json.RawMessage, error) {
	if protocol == Gemini {
		return append(json.RawMessage(nil), raw...), nil
	} // modelVersion is provenance, not an echo alias.
	object, err := rawObject(raw, responseLimit)
	if err != nil {
		return nil, err
	}
	modelJSON, _ := json.Marshal(model)
	if response {
		var kind string
		_ = json.Unmarshal(object["type"], &kind)
		if protocol == Anthropic && kind != "" && kind != "message" {
			if kind == "message_start" {
				return patchNestedModel(object, "message", modelJSON)
			}
			return append(json.RawMessage(nil), raw...), nil
		}
		if protocol == OpenAIResponses && strings.HasPrefix(kind, "response.") {
			if _, ok := object["response"]; ok {
				return patchNestedModel(object, "response", modelJSON)
			}
			return append(json.RawMessage(nil), raw...), nil
		}
	}
	object["model"] = modelJSON
	return marshalBounded(object, responseLimit)
}
func patchNestedModel(object map[string]json.RawMessage, key string, model json.RawMessage) (json.RawMessage, error) {
	nested, err := rawObject(object[key], responseLimit)
	if err != nil {
		return nil, err
	}
	nested["model"] = model
	object[key], err = marshalBounded(nested, responseLimit)
	if err != nil {
		return nil, err
	}
	return marshalBounded(object, responseLimit)
}
