// INPUT: context, encoding/json, errors, PreferenceStore and kernel ABI/wire.
// OUTPUT: NewConfigTool binds only presentation preferences.
// POS: [L4 extension] Preference mutation and query are independent from catalog search and timers.
package utility

import (
	"context"
	"encoding/json"
	"errors"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type configTool struct{ preferences PreferenceStore }

func NewConfigTool(store PreferenceStore) (abi.Tool, error) {
	if missing(store) {
		return nil, errors.New("presentation preference store required")
	}
	return &configTool{preferences: store}, nil
}
func (*configTool) Name() string { return "config" }
func (*configTool) Description() string {
	return "Get/set/list only scoped response presentation preferences; request projection requires the host's ApplyPreferences hook. No provider/key/permissions or arbitrary configuration."
}
func (*configTool) ParametersSchema() json.RawMessage {
	return json.RawMessage(`{"type":"object","additionalProperties":false,"required":["action"],"properties":{"action":{"type":"string","enum":["get","set","list"]},"key":{"type":"string","enum":["response.language","response.verbosity","response.format"]},"value":{"type":"string"}}}`)
}
func (*configTool) Scheduling(wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{ConcurrentSafe: true}
}
func (*configTool) IsReadOnly(c wire.ToolCall) bool {
	var a struct{ Action string }
	return json.Unmarshal(c.Arguments, &a) == nil && (a.Action == "get" || a.Action == "list")
}
func (t *configTool) DescribeEffect(c wire.ToolCall) abi.Effect {
	kind := abi.EffectWrite
	if t.IsReadOnly(c) {
		kind = abi.EffectRead
	}
	return abi.Effect{Kind: kind, Description: "Scoped response presentation preferences, never host authorization"}
}
func (t *configTool) Execute(ctx context.Context, e abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	var zero wire.ToolResult
	if err := actual(t, e, call); err != nil {
		return zero, err
	}
	var a struct {
		Action string `json:"action"`
		Key    string `json:"key"`
		Value  string `json:"value"`
	}
	if err := decode(call.Arguments, &a); err != nil {
		return zero, err
	}
	if err := ctx.Err(); err != nil {
		return zero, err
	}
	var body any
	var err error
	switch a.Action {
	case "set":
		if err = ValidatePreference(a.Key, a.Value); err != nil {
			return zero, err
		}
		if err = t.preferences.SetPreference(ctx, a.Key, a.Value); err != nil {
			return zero, err
		}
		body = map[string]any{"key": a.Key, "value": a.Value, "applies": "next model request presentation only"}
	case "get":
		if err = ValidatePreference(a.Key, map[string]string{"response.language": "en", "response.verbosity": "normal", "response.format": "text"}[a.Key]); err != nil {
			return zero, err
		}
		snapshot, err := preferences(ctx, t.preferences)
		if err != nil {
			return zero, err
		}
		value, found := snapshot.Values[a.Key]
		body = map[string]any{"key": a.Key, "value": value, "set": found}
	case "list":
		snapshot, err := preferences(ctx, t.preferences)
		if err != nil {
			return zero, err
		}
		preferences, revision := snapshot.Values, snapshot.Revision
		body = map[string]any{"values": preferences, "revision": revision, "allowed_keys": []string{"response.language", "response.verbosity", "response.format"}}
	default:
		return zero, errors.New("actual config action get/set/list required")
	}
	if err := ctx.Err(); err != nil {
		return zero, err
	}
	return respond(call.ID, body, false)
}
