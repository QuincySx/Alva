// INPUT: context, encoding/json, strings, time, kernel/abi, llm/wire, protocol/mcp, platform/system/httpjson.
// OUTPUT: Session-bound mcp_runtime Tool with bounded discovery and reference-only output.
// POS: [L4 extension] One HITL gateway keeps all remote schemas, full results and protocol transcripts out of automatic context.
package managedmcp

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"path/filepath"
	"sort"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/platform/system/httpjson"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func marshal(value any) (json.RawMessage, error) { data, err := json.Marshal(value); return data, err }

type gateway struct{ plugin *Plugin }

func (*gateway) Name() string { return "mcp_runtime" }
func (*gateway) Description() string {
	return "Approved MCP stdio gateway: list_servers without process startup; search_tools within one explicitly selected server; describe_tool one exact schema; call_tool one exact method with object arguments. All actions require normal host approval. Output is a small retained interaction reference, never the raw catalog/result. Retrieve only needed untrusted text using context_read/context_search/native_history."
}
func (*gateway) ParametersSchema() json.RawMessage {
	return json.RawMessage(`{"type":"object","additionalProperties":false,"required":["action"],"properties":{"action":{"type":"string","enum":["list_servers","search_tools","describe_tool","call_tool"]},"server_id":{"type":"string","minLength":1,"maxLength":128},"tool_name":{"type":"string","minLength":1,"maxLength":256},"query":{"type":"string","maxLength":200},"offset":{"type":"integer","minimum":0,"maximum":1000000},"limit":{"type":"integer","minimum":1,"maximum":20},"arguments":{"type":"object"}}}`)
}
func (*gateway) Scheduling(wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{ExecutionMode: abi.SerialGlobal}
}
func (*gateway) DescribeEffect(wire.ToolCall) abi.Effect {
	return abi.Effect{Kind: abi.EffectUnknown, Description: "Discover or invoke external MCP capabilities; server annotations do not grant host authority"}
}
func (*gateway) IsReadOnly(wire.ToolCall) bool { return false }

type gatewayInput struct {
	Action    string          `json:"action"`
	Server    string          `json:"server_id"`
	Tool      string          `json:"tool_name"`
	Query     string          `json:"query"`
	Offset    int             `json:"offset"`
	Limit     int             `json:"limit"`
	Arguments json.RawMessage `json:"arguments"`
}

func (g *gateway) Execute(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	p := g.plugin
	// Check actual execution before approval, any journal write or host action.
	if execution.SessionID != p.id || !p.journal.log.SameAuthority(execution.Session) || execution.ActualTool != g || g != p.gateway || call.Name != g.Name() || call.ID == "" || execution.CallID != call.ID {
		return wire.ToolResult{}, errors.New("MCP gateway actual session/call mismatch")
	}
	p.mu.Lock()
	active := !p.closed && p.runID != "" && p.runID == execution.RunID
	p.mu.Unlock()
	if !active {
		return wire.ToolResult{}, errors.New("MCP gateway is not bound to this active run")
	}
	if len(call.Arguments) > 256<<10 || !utf8.Valid(call.Arguments) || !wire.ValidJSON(call.Arguments) || bytes.TrimSpace(call.Arguments)[0] != '{' {
		return wire.ToolResult{}, errors.New("MCP gateway input exceeds 256 KiB or is not UTF-8")
	}
	var input gatewayInput
	if err := httpjson.ValidateObject(call.Arguments, &input); err != nil {
		return wire.ToolResult{}, err
	}
	decoder := json.NewDecoder(bytes.NewReader(call.Arguments))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&input); err != nil {
		return wire.ToolResult{}, err
	}
	if decoder.Decode(new(any)) != io.EOF {
		return wire.ToolResult{}, errors.New("MCP gateway input must be one object")
	}
	if input.Limit == 0 {
		input.Limit = 5
	}
	if input.Limit < 1 || input.Limit > 20 || input.Offset < 0 || input.Offset > 1000000 || utf8.RuneCountInString(input.Query) > 200 || len(input.Server) > 128 || len(input.Tool) > 256 {
		return wire.ToolResult{}, errors.New("MCP discovery input exceeds bounds")
	}
	switch input.Action {
	case "list_servers":
	case "search_tools":
		if input.Server == "" {
			return wire.ToolResult{}, errors.New("search_tools requires one explicit server_id")
		}
	case "describe_tool", "call_tool":
		if input.Server == "" || input.Tool == "" {
			return wire.ToolResult{}, errors.New("exact MCP server and tool required")
		}
	default:
		return wire.ToolResult{}, errors.New("unknown MCP gateway action")
	}
	if input.Action == "call_tool" {
		if len(input.Arguments) == 0 {
			input.Arguments = json.RawMessage(`{}`)
		}
		var object map[string]json.RawMessage
		if json.Unmarshal(input.Arguments, &object) != nil || object == nil {
			return wire.ToolResult{}, errors.New("MCP arguments must be an object")
		}
	}
	if input.Action != "list_servers" {
		for _, item := range p.servers {
			if (input.Server == "" || input.Server == item.config.ID) && (!filepath.IsAbs(execution.Workspace) || filepath.Clean(execution.Workspace) != filepath.Clean(item.config.Process.Cwd)) {
				return wire.ToolResult{}, errors.New("MCP approved cwd does not match this actual execution workspace")
			}
		}
	}
	select {
	case p.gate <- struct{}{}:
	case <-ctx.Done():
		return wire.ToolResult{}, ctx.Err()
	}
	defer func() { <-p.gate }()
	p.mu.Lock()
	runID, runCtx, closed := p.runID, p.runCtx, p.closed
	p.mu.Unlock()
	if closed || runCtx == nil || runID == "" || runID != execution.RunID {
		return wire.ToolResult{}, errors.New("MCP gateway is not bound to this active run")
	}
	operation, cancel := context.WithTimeout(ctx, 10*time.Minute)
	defer cancel()
	stop := context.AfterFunc(runCtx, cancel)
	defer stop()
	if runCtx.Err() != nil {
		cancel()
	}
	if err := operation.Err(); err != nil {
		return wire.ToolResult{}, err
	}
	if err := p.guard.Authorize(operation, execution, call, g.DescribeEffect(call)); err != nil {
		return wire.ToolResult{}, err
	}
	if _, err := p.record(operation, runID, input.Server, "gateway_input", map[string]any{"tool_call_id": call.ID, "input": json.RawMessage(call.Arguments)}); err != nil {
		return wire.ToolResult{}, g.failWorkers(operation, err)
	}
	value, operationErr := g.perform(operation, runID, input)
	isError := false
	if operationErr != nil {
		isError = true
		value = map[string]any{"status": "failed_or_cancelled", "acceptance": "unknown", "replay": false, "notice": sourceNotice}
	} else {
		if raw, ok := value.(json.RawMessage); ok {
			var status struct {
				IsError bool `json:"isError"`
			}
			_ = json.Unmarshal(raw, &status)
			isError = status.IsError
		}
	}
	cleanup, end := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
	defer end()
	entry, err := p.record(cleanup, runID, input.Server, "gateway_output", map[string]any{"tool_call_id": call.ID, "is_error": isError, "result": value})
	if err != nil {
		return wire.ToolResult{}, g.failWorkers(cleanup, errors.Join(operationErr, err))
	}
	payload, _ := json.Marshal(map[string]any{"mcp_interaction_reference": entry, "is_error": isError, "retrieve": "Use context_read/context_search on value.uri; native_history lists/searches retained records in this actual session. Retrieve relevant text only.", "source_notice": sourceNotice})
	result := wire.ToolResult{CallID: call.ID, IsError: isError, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: string(payload)}}}
	if operationErr != nil {
		return result, operationErr
	}
	return result, nil
}
func (g *gateway) failWorkers(ctx context.Context, cause error) error {
	for _, item := range g.plugin.servers {
		item.failed = true
		if item.client != nil {
			err := item.client.Close(ctx)
			cause = errors.Join(cause, err)
			if workerClean(item.client, err) {
				item.client = nil
				g.plugin.mu.Lock()
				g.plugin.cleanupErr = errors.Join(g.plugin.cleanupErr, err)
				g.plugin.mu.Unlock()
			}
			item.tools = nil
		}
	}
	return cause
}
func (g *gateway) perform(ctx context.Context, runID string, input gatewayInput) (any, error) {
	p := g.plugin
	selected := make([]*server, 0, len(p.servers))
	for _, item := range p.servers {
		if input.Server == "" || input.Server == item.config.ID {
			selected = append(selected, item)
		}
	}
	if input.Server != "" && len(selected) == 0 {
		return nil, errors.New("MCP server is not in the approved snapshot")
	}
	sort.Slice(selected, func(i, j int) bool { return selected[i].config.ID < selected[j].config.ID })
	if input.Action == "list_servers" {
		rows := make([]map[string]any, 0, len(selected))
		for _, item := range selected {
			status := "not_started"
			if item.failed {
				status = "failed"
			} else if item.client != nil && item.client.Connected() {
				status = "connected"
			}
			rows = append(rows, map[string]any{"server_id": item.config.ID, "status": status})
		}
		start, end := bounds(input.Offset, input.Limit, len(rows))
		return map[string]any{"servers": rows[start:end], "next_offset": end, "has_more": end < len(rows)}, nil
	}
	discovery, endDiscovery := context.WithTimeout(ctx, 60*time.Second)
	defer endDiscovery()
	for _, item := range selected {
		if err := p.connect(discovery, item, runID); err != nil {
			return nil, err
		}
	}
	if input.Action == "search_tools" {
		rows := make([]map[string]string, 0, input.Limit)
		total := 0
		needle := strings.ToLower(input.Query)
		for _, item := range selected {
			for _, tool := range item.tools {
				if needle != "" && !strings.Contains(strings.ToLower(tool.Name+" "+tool.Description), needle) {
					continue
				}
				if total >= input.Offset && len(rows) < input.Limit {
					rows = append(rows, map[string]string{"server_id": item.config.ID, "tool_name": tool.Name, "description": preview(tool.Description, 96)})
				}
				total++
			}
		}
		_, end := bounds(input.Offset, input.Limit, total)
		return map[string]any{"tools": rows, "next_offset": end, "has_more": end < total}, nil
	}
	item := selected[0]
	for _, tool := range item.tools {
		if tool.Name != input.Tool {
			continue
		}
		if input.Action == "describe_tool" {
			return map[string]any{"server_id": item.config.ID, "tool_name": tool.Name, "description": tool.Description, "input_schema": tool.InputSchema, "source_notice": sourceNotice}, nil
		}
		result, err := item.client.CallTool(ctx, tool.Name, input.Arguments)
		if err != nil {
			item.failed = true
			cleanup, end := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
			defer end()
			_, recordErr := p.record(cleanup, runID, item.config.ID, "worker_status", map[string]any{"operation": "call_tool", "status": "failed_or_cancelled_acceptance_unknown", "replay": false})
			return nil, errors.Join(err, recordErr)
		}
		return result, nil
	}
	return nil, errors.New("selected MCP method is unavailable in this run's connected catalog")
}
func bounds(offset, limit, total int) (int, int) {
	if offset > total {
		offset = total
	}
	end := offset + limit
	if end > total {
		end = total
	}
	return offset, end
}
func preview(text string, maxRunes int) string {
	runes := []rune(text)
	if len(runes) > maxRunes {
		return string(runes[:maxRunes]) + "…"
	}
	return text
}
