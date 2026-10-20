// INPUT: Explicit catalog query or selected AEP remote call under the actual native tool context, platform/system/strictjson.
// OUTPUT: One small SDK Tool, bounded discovery and reference-only opaque remote results
// POS: [L4 extension] Progressive Plugin capability gateway; large schemas/output never become automatic model context
package loader

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/platform/storage/redaction"
	strictjson "github.com/QuincySx/Alva/platform/system/strictjson"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type pluginGateway struct{ plugin *Plugin }

func (g *pluginGateway) Name() string {
	hash := sha256.Sum256([]byte(g.plugin.config.Bundle.manifest.Name))
	return "aep_" + hex.EncodeToString(hash[:16])
}
func (g *pluginGateway) Description() string {
	return "Search an approved native Plugin's bounded tool catalog or explicitly call one selected remote tool. Full schemas, I/O and opaque results stay session references; no raw media is delivered or interpreted."
}
func (g *pluginGateway) ParametersSchema() json.RawMessage {
	return json.RawMessage(`{"type":"object","properties":{"action":{"type":"string","enum":["catalog","call"]},"query":{"type":"string","maxLength":256},"after":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":8},"tool":{"type":"string","maxLength":256},"arguments":{"type":"object"}},"required":["action"],"additionalProperties":false}`)
}
func (g *pluginGateway) Scheduling(wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{ExecutionMode: abi.SerialGlobal}
}
func (g *pluginGateway) DescribeEffect(call wire.ToolCall) abi.Effect {
	var args struct {
		Action string `json:"action"`
	}
	_ = json.Unmarshal(call.Arguments, &args)
	if args.Action == "catalog" {
		return abi.Effect{Kind: abi.EffectRead, Description: "Read the already-approved Plugin's bounded metadata/catalog; no new process"}
	}
	return abi.Effect{Kind: abi.EffectUnknown, Description: "Execute an explicit approved native Plugin remote tool; its code can access the host"}
}
func (g *pluginGateway) IsReadOnly(call wire.ToolCall) bool {
	return g.DescribeEffect(call).Kind == abi.EffectRead
}

func (g *pluginGateway) Execute(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	p := g.plugin
	if execution.ActualTool != g || g != p.gateway || call.Name != g.Name() || call.ID == "" || execution.CallID != call.ID || execution.SessionID != p.config.NativeID || execution.RunID == "" || execution.Workspace != p.config.Workspace || !p.log.SameAuthority(execution.Session) {
		return wire.ToolResult{}, errors.New("AEP gateway actual invocation authority mismatch")
	}
	if len(call.Arguments) > 256<<10 {
		return wire.ToolResult{}, errors.New("AEP gateway input exceeds 256 KiB")
	}
	p.mu.Lock()
	runID := p.runID
	p.mu.Unlock()
	if runID != execution.RunID {
		return wire.ToolResult{}, errors.New("AEP gateway run mismatch")
	}
	var args struct {
		Action    string          `json:"action"`
		Query     string          `json:"query,omitempty"`
		After     int             `json:"after,omitempty"`
		Limit     int             `json:"limit,omitempty"`
		Tool      string          `json:"tool,omitempty"`
		Arguments json.RawMessage `json:"arguments,omitempty"`
	}
	if err := strictjson.ValidateObject(call.Arguments, &args); err != nil {
		return wire.ToolResult{}, err
	}
	if err := decodeCallback(call.Arguments, &args); err != nil {
		return wire.ToolResult{}, err
	}
	if err := p.healthy(); err != nil {
		return wire.ToolResult{}, err
	}
	select {
	case p.gate <- struct{}{}:
	case <-ctx.Done():
		return wire.ToolResult{}, ctx.Err()
	}
	defer func() { <-p.gate }()
	if err := p.healthy(); err != nil {
		return wire.ToolResult{}, err
	}
	p.mu.Lock()
	runCtx := p.runCtx
	p.mu.Unlock()
	if runCtx == nil {
		return wire.ToolResult{}, errors.New("AEP gateway has no active run")
	}
	operation, cancel := context.WithCancel(ctx)
	defer cancel()
	stopRun := context.AfterFunc(runCtx, cancel)
	defer stopRun()
	if runCtx.Err() != nil {
		cancel()
	}
	ctx = operation
	if err := ctx.Err(); err != nil {
		return wire.ToolResult{}, err
	}
	if err := p.config.Guard.Authorize(ctx, execution, call, g.DescribeEffect(call)); err != nil {
		return wire.ToolResult{}, err
	}
	switch args.Action {
	case "catalog":
		if args.Arguments != nil || args.Tool != "" || args.After < 0 || args.After > len(p.tools) || len(args.Query) > 256 || !utf8.ValidString(args.Query) {
			return wire.ToolResult{}, errors.New("invalid bounded Plugin catalog query")
		}
		if args.Limit == 0 {
			args.Limit = 5
		}
		if args.Limit < 1 || args.Limit > 8 {
			return wire.ToolResult{}, errors.New("catalog limit must be 1..8")
		}
		type summary struct {
			Name        string         `json:"name"`
			Description string         `json:"description"`
			Schema      wire.Reference `json:"schema"`
		}
		var items []summary
		next := args.After
		scanned := 0
		for next < len(p.tools) && len(items) < args.Limit && scanned < 128 {
			tool := p.tools[next]
			next++
			scanned++
			if args.Query != "" && !strings.Contains(strings.ToLower(tool.Name+" "+tool.Description), strings.ToLower(args.Query)) {
				continue
			}
			schema, err := redaction.RedactJSON(tool.InputSchema, p.journal.secrets...)
			if err != nil {
				return wire.ToolResult{}, p.poison(err)
			}
			ref, err := p.refs.StoreText(ctx, string(schema))
			if err != nil {
				return wire.ToolResult{}, p.poison(err)
			}
			description := tool.Description
			if len(description) > 192 {
				description = description[:192]
				for !utf8.ValidString(description) {
					description = description[:len(description)-1]
				}
				description += " [description shortened; full catalog retained]"
			}
			items = append(items, summary{tool.Name, description, ref})
		}
		data, err := json.Marshal(map[string]any{"tools": items, "next": next, "has_more": next < len(p.tools), "catalog_reference": p.catalog, "source_notice": pluginSource})
		if err != nil {
			return wire.ToolResult{}, err
		}
		if len(data) > 8192 {
			return wire.ToolResult{}, errors.New("Plugin catalog metadata exceeded 8 KiB")
		}
		return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: string(data)}}}, nil
	case "call":
		if args.Query != "" || args.After != 0 || args.Limit != 0 || args.Tool == "" || len(args.Arguments) > 256<<10 || !wire.ValidJSON(args.Arguments) || strings.TrimSpace(string(args.Arguments))[0] != '{' {
			return wire.ToolResult{}, errors.New("explicit remote tool and bounded object arguments required")
		}
		found := false
		for _, tool := range p.tools {
			found = found || tool.Name == args.Tool
		}
		if !found {
			return wire.ToolResult{}, errors.New("remote tool is not in the approved handshake")
		}
		data, err := p.client.Call(ctx, "tools/call", map[string]any{"name": args.Tool, "arguments": args.Arguments})
		if err != nil {
			return wire.ToolResult{}, p.poison(err)
		}
		if err = p.healthy(); err != nil {
			return wire.ToolResult{}, err
		}
		return p.resultReference(ctx, call.ID, data, false)
	default:
		return wire.ToolResult{}, errors.New("unknown Plugin gateway action")
	}
}

func (p *Plugin) resultReference(ctx context.Context, callID string, data json.RawMessage, actionResult bool) (wire.ToolResult, error) {
	if !wire.ValidJSON(data) {
		return wire.ToolResult{}, errors.New("invalid Plugin result JSON")
	}
	redacted, redactErr := redaction.RedactJSON(data, p.journal.secrets...)
	if redactErr != nil {
		return wire.ToolResult{}, p.poison(redactErr)
	}
	var object map[string]json.RawMessage
	var isError bool
	if json.Unmarshal(data, &object) == nil && object != nil {
		flag := "isError"
		if actionResult {
			flag = "is_error"
		}
		if raw, ok := object[flag]; ok && json.Unmarshal(raw, &isError) != nil {
			return wire.ToolResult{}, errors.New("invalid Plugin isError result flag")
		}
	}
	ref, err := p.refs.StoreText(ctx, string(redacted))
	if err != nil {
		return wire.ToolResult{}, p.poison(err)
	}
	return wire.ToolResult{CallID: callID, IsError: isError, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: "Opaque Plugin result saved in this actual native session; read/search only relevant text ranges. No media processing or host verification implied. " + pluginSource}, {Kind: wire.ContentReference, Reference: &ref}}}, nil
}
