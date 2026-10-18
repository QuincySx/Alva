// INPUT: context, encoding/json, errors, time, kernel/abi and llm/wire
// OUTPUT: NewTool returning a portable cancellable sleep Tool
// POS: [L4 extension] Stateless waiting uses only the caller's context, without native storage, permissions, services or a new Plugin lifecycle.
package timer

import (
	"context"
	"encoding/json"
	"errors"
	"time"

	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

// A nonzero size preserves distinct pointer identities for actual dispatch.
type tool struct{ identity byte }

func NewTool() abi.Tool { return &tool{} }

func (*tool) Name() string { return "sleep" }
func (*tool) Description() string {
	return "Actually wait for 0..300000 ms with cancellation; no background polling goroutine or held global host execution lease."
}
func (*tool) ParametersSchema() json.RawMessage {
	return json.RawMessage(`{"type":"object","additionalProperties":false,"required":["duration_ms"],"properties":{"duration_ms":{"type":"integer","minimum":0,"maximum":300000}}}`)
}
func (*tool) Scheduling(wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{ExecutionMode: abi.Coordinator, ManagesOwnTimeout: true}
}
func (*tool) IsReadOnly(wire.ToolCall) bool { return true }
func (*tool) DescribeEffect(wire.ToolCall) abi.Effect {
	return abi.Effect{Kind: abi.EffectRead, Description: "Cancellable waiting only; no filesystem, network or session-state access"}
}
func (t *tool) Execute(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	if execution.ActualTool != t || call.ID == "" || execution.CallID != call.ID || call.Name != t.Name() {
		return wire.ToolResult{}, errors.New("sleep requires its actual dispatched tool/call")
	}
	if err := ctx.Err(); err != nil {
		return wire.ToolResult{}, err
	}
	var fields map[string]json.RawMessage
	if len(call.Arguments) > 4096 || !wire.ValidJSON(call.Arguments) || json.Unmarshal(call.Arguments, &fields) != nil || len(fields) != 1 {
		return wire.ToolResult{}, errors.New("sleep requires one bounded UTF-8 object with duration_ms")
	}
	var duration *int64
	if json.Unmarshal(fields["duration_ms"], &duration) != nil || duration == nil || *duration < 0 || *duration > 300000 {
		return wire.ToolResult{}, errors.New("explicit sleep duration 0..300000ms required")
	}
	started := time.Now()
	timer := time.NewTimer(time.Duration(*duration) * time.Millisecond)
	defer timer.Stop()
	select {
	case <-timer.C:
	case <-ctx.Done():
		return wire.ToolResult{}, ctx.Err()
	}
	if err := ctx.Err(); err != nil {
		return wire.ToolResult{}, err
	}
	data, err := json.Marshal(struct {
		Requested int64 `json:"requested_ms"`
		Elapsed   int64 `json:"elapsed_ms"`
		Completed bool  `json:"completed"`
	}{*duration, time.Since(started).Milliseconds(), true})
	if err != nil {
		return wire.ToolResult{}, err
	}
	return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: string(data)}}}, nil
}
