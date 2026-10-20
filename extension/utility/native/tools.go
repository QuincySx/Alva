// INPUT: context, JSON, errors, native session/Guard, ABI/wire and portable utility Tools.
// OUTPUT: Native-authorized wrappers and selected schema evidence for the shared utility Tools.
// POS: [L4 native adapter] Durable preference and schema policy stays outside portable registry/search implementations.
package native

import (
	"context"
	"encoding/json"
	"errors"

	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/platform/storage/redaction"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type utilityTool struct {
	abi.Tool
	plugin *Plugin
	name   string
}

func (t *utilityTool) Execute(ctx context.Context, e abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	if e.ActualTool != t || call.ID == "" || e.CallID != call.ID || call.Name != t.Name() {
		return wire.ToolResult{}, errors.New("utility requires actual dispatched tool/call identity")
	}
	p := t.plugin
	if _, err := p.check(e); err != nil {
		return wire.ToolResult{}, err
	}
	if err := p.config.Guard.Authorize(ctx, e, call, t.DescribeEffect(call)); err != nil {
		return wire.ToolResult{}, err
	}
	if _, err := p.check(e); err != nil {
		return wire.ToolResult{}, err
	}
	e.ActualTool = t.Tool
	return t.Tool.Execute(ctx, e, call)
}
func (p *Plugin) retainSchema(ctx context.Context, name string, schema json.RawMessage) (wire.Reference, error) {
	clean, err := redaction.RedactJSON(schema)
	if err != nil {
		return wire.Reference{}, err
	}
	info, err := p.config.Repository.PutArtifact(ctx, p.config.Owner, p.config.SessionID, storage.ArtifactInput{Kind: "tool-schema", Name: name, MediaType: "application/json", Processing: "Actual selected registered tool schema; known credentials redacted; registry discovery did not start any external service; no automatic model delivery", Data: clean, Text: true})
	if err != nil {
		return wire.Reference{}, &abi.SessionError{Operation: "retain tool schema", SessionID: p.config.SessionID, Err: err}
	}
	return info.Reference(), nil
}
