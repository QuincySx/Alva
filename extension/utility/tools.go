// INPUT: bytes, context, encoding/json, errors/io, strings/utf8, ABI/wire, native artifacts.
// OUTPUT: Actual bounded registry search/schema references and durable allowlisted config Tools.
// POS: [L4 extension] Native preference and discovery policies remain separate from the portable timer implementation.
package utility

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type utilityTool struct {
	plugin *Plugin
	name   string
}

func (t *utilityTool) Name() string { return t.name }
func (t *utilityTool) Description() string {
	return map[string]string{"config": "Get/set/list only durable response presentation preferences with a real next-request consumer; no provider/key/permissions or arbitrary configuration.", "tool_search": "Search the actual registered tools by name/first4KiB description, or retain one selected schema as reference. Never starts MCP/AEP or loads a remote catalog."}[t.name]
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
	return abi.Effect{Kind: kind, Description: "Bounded local utility metadata/timer or harmless response presentation preferences, never host authorization"}
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
	Action, Key, Value, Query, Name string
	Maximum                         int `json:"max_results"`
}

func (t *utilityTool) decode(raw json.RawMessage) (arguments, error) {
	var a arguments
	if len(raw) > 4096 || !utf8.Valid(raw) {
		return a, errors.New("utility input exceeds 4 KiB UTF-8")
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
	if e.ActualTool != t || e.CallID != call.ID || call.Name != t.name {
		return zero, errors.New("utility requires actual dispatched tool/call identity")
	}
	a, err := t.decode(call.Arguments)
	if err != nil {
		return zero, err
	}
	p := t.plugin
	state, err := p.check(e)
	if err != nil {
		return zero, err
	}
	if err = p.config.Guard.Authorize(ctx, e, call, t.DescribeEffect(call)); err != nil {
		return zero, err
	}
	if _, err = p.check(e); err != nil {
		return zero, err
	}
	var body any
	switch t.name {
	case "config":
		switch a.Action {
		case "set":
			if err = p.set(ctx, a.Key, a.Value); err != nil {
				return zero, err
			}
			body = map[string]any{"key": a.Key, "value": a.Value, "applies": "next model request presentation only"}
		case "get":
			if err = validPreference(a.Key, map[string]string{"response.language": "en", "response.verbosity": "normal", "response.format": "text"}[a.Key]); err != nil {
				return zero, err
			}
			p.mu.Lock()
			value, found := p.preferences[a.Key]
			p.mu.Unlock()
			body = map[string]any{"key": a.Key, "value": value, "set": found}
		case "list":
			p.mu.Lock()
			preferences := map[string]string{}
			for key, value := range p.preferences {
				preferences[key] = value
			}
			revision := p.revision
			p.mu.Unlock()
			body = map[string]any{"values": preferences, "revision": revision, "allowed_keys": []string{"response.language", "response.verbosity", "response.format"}}
		default:
			return zero, errors.New("actual config action get/set/list required")
		}
	case "tool_search":
		if a.Action == "" {
			a.Action = "search"
		}
		snapshot := state.Tools().Snapshot()
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
			clean, err := session.RedactJSON(schema)
			if err != nil {
				return zero, err
			}
			info, err := p.config.Repository.PutArtifact(ctx, p.config.Owner, p.config.SessionID, session.ArtifactInput{Kind: "tool-schema", Name: a.Name, MediaType: "application/json", Processing: "Actual selected registered tool schema; known credentials redacted; registry discovery did not start any external service; no automatic model delivery", Data: clean, Text: true})
			if err != nil {
				return zero, &abi.SessionError{Operation: "retain tool schema", SessionID: p.config.SessionID, Err: err}
			}
			body = map[string]any{"name": a.Name, "registry_version": snapshot.Version, "schema": info.Reference()}
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
