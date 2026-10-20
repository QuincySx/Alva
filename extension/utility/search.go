// INPUT: context, encoding/json, errors, strings/utf8 and kernel ABI/wire.
// OUTPUT: SearchConfig and NewSearchTool expose an actual live registry with optional schema retention.
// POS: [L4 extension] Catalog inspection neither executes tools nor requires a database or artifact reader.
package utility

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type SearchConfig struct {
	Snapshot     func() abi.ToolRegistrySnapshot
	RetainSchema func(context.Context, string, json.RawMessage) (wire.Reference, error)
}
type searchTool struct{ config SearchConfig }

func NewSearchTool(config SearchConfig) (abi.Tool, error) {
	if config.Snapshot == nil {
		return nil, errors.New("live tool snapshot required")
	}
	return &searchTool{config: config}, nil
}
func (*searchTool) Name() string { return "tool_search" }
func (*searchTool) Description() string {
	return "Search actual registered tools by name/first4KiB description, or read one selected bounded schema inline or by configured reference. Never starts MCP/AEP or loads a remote catalog."
}
func (*searchTool) ParametersSchema() json.RawMessage {
	return json.RawMessage(`{"type":"object","additionalProperties":false,"properties":{"action":{"type":"string","enum":["search","schema"]},"query":{"type":"string","maxLength":256},"name":{"type":"string","maxLength":128},"max_results":{"type":"integer","minimum":1,"maximum":20}}}`)
}
func (*searchTool) Scheduling(wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{ConcurrentSafe: true}
}
func (*searchTool) IsReadOnly(wire.ToolCall) bool { return true }
func (*searchTool) DescribeEffect(wire.ToolCall) abi.Effect {
	return abi.Effect{Kind: abi.EffectRead, Description: "Read only actual registered tool metadata"}
}
func (t *searchTool) Execute(ctx context.Context, e abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	var zero wire.ToolResult
	if err := actual(t, e, call); err != nil {
		return zero, err
	}
	var a struct {
		Action  string `json:"action"`
		Query   string `json:"query"`
		Name    string `json:"name"`
		Maximum int    `json:"max_results"`
	}
	if err := decode(call.Arguments, &a); err != nil {
		return zero, err
	}
	if err := ctx.Err(); err != nil {
		return zero, err
	}
	var body any

	if a.Action == "" {
		a.Action = "search"
	}
	snapshot := t.config.Snapshot()
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
		if t.config.RetainSchema == nil {
			body = map[string]any{"name": a.Name, "registry_version": snapshot.Version, "schema": json.RawMessage(schema)}
		} else {
			reference, err := t.config.RetainSchema(ctx, a.Name, append(json.RawMessage(nil), schema...))
			if err != nil {
				return zero, err
			}
			if err := reference.Validate(); err != nil {
				return zero, err
			}
			body = map[string]any{"name": a.Name, "registry_version": snapshot.Version, "schema": reference}
		}
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
		body = map[string]any{"tools": items, "matches": matches, "has_more": matches > len(items), "registry_version": snapshot.Version, "scope": "actual tool names and first 4KiB of each description only; selected full schema via action=schema under host output policy; no external catalog discovery"}
	} else {
		return zero, errors.New("tool search action search/schema required")
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
