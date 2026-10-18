// INPUT: bytes, context, encoding/json, errors/io/reflect, strings/utf8, strictjson and ABI/wire.
// OUTPUT: NewConfigTool and NewSearchTool consume an explicit preference store or live registry snapshot.
// POS: [L4 extension] Ordinary metadata Tools are portable and install no native session, database, Guard or middleware.
package utility

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"reflect"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/platform/system/strictjson"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type utilityTool struct {
	preferences PreferenceStore
	snapshot    func() abi.ToolRegistrySnapshot
	retain      func(context.Context, string, json.RawMessage) (wire.Reference, error)
	name        string
}

// NewConfigTool uses only an explicitly scoped presentation preference store.
func NewConfigTool(store PreferenceStore) (abi.Tool, error) {
	if missing(store) {
		return nil, errors.New("presentation preference store required")
	}
	return &utilityTool{name: "config", preferences: store}, nil
}

// NewSearchTool observes the caller's live registry. retain stores only a selected
// schema; discovery never executes a Tool or starts an external service.
func NewSearchTool(snapshot func() abi.ToolRegistrySnapshot, retain func(context.Context, string, json.RawMessage) (wire.Reference, error)) (abi.Tool, error) {
	if snapshot == nil || retain == nil {
		return nil, errors.New("live tool snapshot and selected-schema writer required")
	}
	return &utilityTool{name: "tool_search", snapshot: snapshot, retain: retain}, nil
}
func missing(value any) bool {
	if value == nil {
		return true
	}
	v := reflect.ValueOf(value)
	switch v.Kind() {
	case reflect.Pointer, reflect.Func, reflect.Map, reflect.Slice, reflect.Interface, reflect.Chan:
		return v.IsNil()
	}
	return false
}

func (t *utilityTool) Name() string { return t.name }
func (t *utilityTool) Description() string {
	return map[string]string{"config": "Get/set/list only scoped response presentation preferences; request projection requires the host's ApplyPreferences hook. No provider/key/permissions or arbitrary configuration.", "tool_search": "Search the actual registered tools by name/first4KiB description, or retain one selected schema as reference. Never starts MCP/AEP or loads a remote catalog."}[t.name]
}
func (t *utilityTool) Scheduling(wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{ConcurrentSafe: true}
}
func (t *utilityTool) IsReadOnly(c wire.ToolCall) bool {
	if t.name != "config" {
		return true
	}
	var a struct{ Action string }
	return json.Unmarshal(c.Arguments, &a) == nil && (a.Action == "get" || a.Action == "list")
}
func (t *utilityTool) DescribeEffect(c wire.ToolCall) abi.Effect {
	kind := abi.EffectWrite
	if t.IsReadOnly(c) {
		kind = abi.EffectRead
	}
	return abi.Effect{Kind: kind, Description: "Bounded local utility metadata or harmless response presentation preferences, never host authorization"}
}
func (t *utilityTool) ParametersSchema() json.RawMessage {
	switch t.name {
	case "config":
		return json.RawMessage(`{"type":"object","additionalProperties":false,"required":["action"],"properties":{"action":{"type":"string","enum":["get","set","list"]},"key":{"type":"string","enum":["response.language","response.verbosity","response.format"]},"value":{"type":"string"}}}`)
	default:
		return json.RawMessage(`{"type":"object","additionalProperties":false,"properties":{"action":{"type":"string","enum":["search","schema"]},"query":{"type":"string","maxLength":256},"name":{"type":"string","maxLength":128},"max_results":{"type":"integer","minimum":1,"maximum":20}}}`)
	}
}

type arguments struct {
	Action  string `json:"action"`
	Key     string `json:"key"`
	Value   string `json:"value"`
	Query   string `json:"query"`
	Name    string `json:"name"`
	Maximum int    `json:"max_results"`
}

func (t *utilityTool) decode(raw json.RawMessage) (arguments, error) {
	var a arguments
	if len(raw) > 4096 || !utf8.Valid(raw) {
		return a, errors.New("utility input exceeds 4 KiB UTF-8")
	}
	if err := strictjson.ValidateObject(raw, &a); err != nil {
		return a, err
	}
	var fields map[string]json.RawMessage
	if json.Unmarshal(raw, &fields) != nil || fields == nil {
		return a, errors.New("utility input must be an object")
	}
	var schema struct{ Properties map[string]any }
	json.Unmarshal(t.ParametersSchema(), &schema)
	for key, value := range fields {
		if _, ok := schema.Properties[key]; !ok || bytes.Equal(bytes.TrimSpace(value), []byte("null")) {
			return a, errors.New("unknown/null utility field")
		}
	}
	d := json.NewDecoder(bytes.NewReader(raw))
	d.DisallowUnknownFields()
	if err := d.Decode(&a); err != nil {
		return a, err
	}
	if d.Decode(new(any)) != io.EOF {
		return a, errors.New("one utility input object required")
	}
	return a, nil
}
func (t *utilityTool) Execute(ctx context.Context, e abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	var zero wire.ToolResult
	if e.ActualTool != t || call.ID == "" || e.CallID != call.ID || call.Name != t.name {
		return zero, errors.New("utility requires actual dispatched tool/call identity")
	}
	a, err := t.decode(call.Arguments)
	if err != nil {
		return zero, err
	}
	if err := ctx.Err(); err != nil {
		return zero, err
	}
	var body any
	switch t.name {
	case "config":
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
	case "tool_search":
		if a.Action == "" {
			a.Action = "search"
		}
		snapshot := t.snapshot()
		if len(snapshot.Tools) > 512 {
			return zero, errors.New("actual tool registry exceeds 512-entry search budget")
		}
		if a.Action == "schema" {
			if a.Name == "" || len(a.Name) > 128 || a.Query != "" {
				return zero, errors.New("schema requires one bounded actual tool name")
			}
			var selected abi.Tool
			for _, tool := range snapshot.Tools {
				if tool.Name() == a.Name {
					selected = tool
					break
				}
			}
			if selected == nil {
				return respond(call.ID, map[string]any{"found": false, "name": a.Name}, true)
			}
			schema := selected.ParametersSchema()
			if len(schema) > 64<<10 || !utf8.Valid(schema) || !wire.ValidJSON(schema) {
				return zero, errors.New("selected actual schema invalid or exceeds 64 KiB")
			}
			reference, err := t.retain(ctx, a.Name, append(json.RawMessage(nil), schema...))
			if err != nil {
				return zero, err
			}
			if err := reference.Validate(); err != nil {
				return zero, err
			}
			body = map[string]any{"name": a.Name, "registry_version": snapshot.Version, "schema": reference}
		} else if a.Action == "search" {
			if a.Query == "" || len(a.Query) > 256 || a.Name != "" {
				return zero, errors.New("bounded actual tool query required")
			}
			if a.Maximum == 0 {
				a.Maximum = 5
			}
			if a.Maximum < 1 || a.Maximum > 20 {
				return zero, errors.New("tool search max_results 1..20 required")
			}
			query := strings.ToLower(a.Query)
			items := []any{}
			matches := 0
			for _, tool := range snapshot.Tools {
				name, description := tool.Name(), tool.Description()
				if len(name) > 128 || !utf8.ValidString(name) {
					return zero, errors.New("actual tool metadata exceeds search identity contract")
				}
				scan := prefix(description, 4096)
				if !utf8.ValidString(scan) {
					return zero, errors.New("actual tool description preview is not UTF-8")
				}
				if strings.Contains(strings.ToLower(name), query) || strings.Contains(strings.ToLower(scan), query) {
					matches++
					if len(items) < a.Maximum {
						items = append(items, map[string]any{"name": name, "description_preview": prefix(description, 256), "description_preview_truncated": len(description) > 256})
					}
				}
			}
			body = map[string]any{"tools": items, "matches": matches, "has_more": matches > len(items), "registry_version": snapshot.Version, "scope": "actual tool names and first 4KiB of each description only; selected full schema via action=schema; no external catalog discovery"}
		} else {
			return zero, errors.New("tool search action search/schema required")
		}
	}
	if err := ctx.Err(); err != nil {
		return zero, err
	}
	return respond(call.ID, body, false)
}
func prefix(s string, n int) string {
	if len(s) <= n {
		return s
	}
	for n > 0 && !utf8.RuneStart(s[n]) {
		n--
	}
	return s[:n]
}
func respond(id string, body any, isError bool) (wire.ToolResult, error) {
	data, err := json.Marshal(body)
	if err != nil {
		return wire.ToolResult{}, err
	}
	if len(data) > 16<<10 {
		return wire.ToolResult{}, errors.New("utility result metadata exceeds 16 KiB")
	}
	return wire.ToolResult{CallID: id, IsError: isError, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: string(data)}}}, nil
}
