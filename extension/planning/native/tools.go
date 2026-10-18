// INPUT: context, encoding/json, errors, portable planning Tools, ABI/wire and native artifacts.
// OUTPUT: Native-authorized wrappers and retained task detail references for the shared planning Tools.
// POS: [L4 native adapter] Session identity, Guard and evidence policy wrap the same portable workflow implementation.
package native

import (
	"context"
	"encoding/json"
	"errors"

	planning "github.com/QuincySx/Alva/extension/planning"
	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type planningTool struct {
	abi.Tool
	plugin *Plugin
	name   string
}

func (t *planningTool) Execute(ctx context.Context, e abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	if e.ActualTool != t || call.ID == "" || e.CallID != call.ID || call.Name != t.Name() {
		return wire.ToolResult{}, errors.New("planning requires actual dispatched tool/call identity")
	}
	p := t.plugin
	if err := p.check(e); err != nil {
		return wire.ToolResult{}, err
	}
	if err := p.config.Guard.Authorize(ctx, e, call, t.DescribeEffect(call)); err != nil {
		return wire.ToolResult{}, err
	}
	if err := p.check(e); err != nil {
		return wire.ToolResult{}, err
	}
	// The checked native wrapper is the host authority; only its private delegate
	// receives a rebound implementation identity, never an untrusted named Tool.
	e.ActualTool = t.Tool
	return t.Tool.Execute(ctx, e, call)
}
func (p *Plugin) detail(ctx context.Context, task planning.TaskRecord) (wire.Reference, error) {
	raw, err := json.Marshal(task)
	if err != nil {
		return wire.Reference{}, err
	}
	if len(raw) > 128<<10 {
		return wire.Reference{}, errors.New("configured planning detail exceeds 128 KiB")
	}
	info, err := p.config.Repository.PutArtifact(ctx, p.config.Owner, p.config.SessionID, session.ArtifactInput{Kind: "planning-task-detail", Name: task.Task.ID, MediaType: "application/json", Processing: "Configured planning Service task projection at this read, retained in this native session; backend owns business-state persistence; no execution, permission change or automatic model delivery implied", Data: raw, Text: true})
	if err != nil {
		return wire.Reference{}, &abi.SessionError{Operation: "retain planning detail", SessionID: p.config.SessionID, Err: err}
	}
	return info.Reference(), nil
}
