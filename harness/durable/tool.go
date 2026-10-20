// INPUT: context, encoding/json, errors, strictjson and kernel/abi/wire.
// OUTPUT: Explicit await_external submission Tool, returning a committed request ID without waiting in memory.
// POS: [L5 optional harness] Submits data for a host-managed external operation; never launches it or grants authority.
package durable

import (
	"context"
	"encoding/json"
	"errors"

	"github.com/QuincySx/Alva/platform/system/strictjson"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type waitTool struct{ owner *Controller }

func (*waitTool) Name() string { return "await_external" }
func (*waitTool) Description() string {
	return "Submit a bounded external-result request. The host persists it, exits this run, and explicitly supplies a bound result before resuming. This does not launch a job or authorize side effects."
}
func (*waitTool) ParametersSchema() json.RawMessage {
	return json.RawMessage(`{"type":"object","required":["request"],"additionalProperties":false,"properties":{"request":{"type":"object"}}}`)
}
func (*waitTool) Scheduling(wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{ExecutionMode: abi.SerialGlobal}
}
func (*waitTool) DescribeEffect(wire.ToolCall) abi.Effect {
	return abi.Effect{Kind: abi.EffectRead, Description: "Submit external-result request data only; no external work or host side effect is authorized"}
}
func (*waitTool) IsReadOnly(wire.ToolCall) bool { return true }
func (t *waitTool) Execute(ctx context.Context, e abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	t.owner.mu.Lock()
	active := t.owner.driver != nil && t.owner.runID == e.RunID
	t.owner.mu.Unlock()
	if !active || e.ActualTool != t || !sameLog(e.Session, t.owner.config.Backend.Log()) || e.SessionID != t.owner.config.Backend.Log().ID() || call.Name != t.Name() || call.ID == "" || e.CallID != call.ID {
		return wire.ToolResult{}, errors.New("external wait lacks actual tool/session/call binding")
	}
	var input struct {
		Request json.RawMessage `json:"request"`
	}
	if len(call.Arguments) > 32<<10 {
		return wire.ToolResult{}, errors.New("external wait exceeds 32 KiB")
	}
	if err := strictjson.ValidateObject(call.Arguments, &input); err != nil {
		return wire.ToolResult{}, err
	}
	if err := json.Unmarshal(call.Arguments, &input); err != nil {
		return wire.ToolResult{}, err
	}
	if len(input.Request) == 0 || input.Request[0] != '{' {
		return wire.ToolResult{}, errors.New("external request object required")
	}
	r, err := t.owner.enqueue(ctx, "external", call.ID, input.Request)
	if err != nil {
		return wire.ToolResult{}, err
	}
	data, _ := json.Marshal(struct {
		RequestID, Binding string
		Pending            bool
	}{r.ID, r.Binding, true})
	return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: string(data)}}}, nil
}
