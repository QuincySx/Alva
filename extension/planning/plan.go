// INPUT: context, encoding/json, errors, strings/utf8, strictjson and kernel ABI/wire.
// OUTPUT: NewPlanTool installs only todo_write with a host-bound write callback.
// POS: [L4 extension] Small plan storage requires neither task CRUD, a Plugin nor reference storage.
package planning

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/platform/system/strictjson"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type planTool struct {
	write func(context.Context, string) (*wire.Reference, error)
}

// NewPlanTool accepts a complete plan replacement operation. A nil reference
// with nil error means the host saved the plan without reference output.
// References, when returned, must be readable in the caller's actual scope.
func NewPlanTool(write func(context.Context, string) (*wire.Reference, error)) (abi.Tool, error) {
	if write == nil {
		return nil, errors.New("plan write callback required")
	}
	return &planTool{write: write}, nil
}

func (*planTool) Name() string { return "todo_write" }
func (*planTool) Description() string {
	return "Replace the host-bound plan notes. Does not edit arbitrary files, start tasks or change permissions."
}
func (*planTool) ParametersSchema() json.RawMessage {
	return json.RawMessage(`{"type":"object","additionalProperties":false,"required":["content"],"properties":{"content":{"type":"string","maxLength":65536}}}`)
}
func (*planTool) Scheduling(wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{ExecutionMode: abi.SerialGlobal}
}
func (*planTool) IsReadOnly(wire.ToolCall) bool { return false }
func (*planTool) DescribeEffect(wire.ToolCall) abi.Effect {
	return abi.Effect{Kind: abi.EffectWrite, Description: "Replace host-bound plan notes only"}
}
func (t *planTool) Execute(ctx context.Context, e abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	if e.ActualTool != t || call.ID == "" || e.CallID != call.ID || call.Name != t.Name() {
		return wire.ToolResult{}, errors.New("plan requires actual dispatched tool/call identity")
	}
	var args struct {
		Content *string `json:"content"`
	}
	if len(call.Arguments) > 512<<10 {
		return wire.ToolResult{}, errors.New("plan arguments exceed 512 KiB")
	}
	if err := strictjson.ValidateObject(call.Arguments, &args); err != nil {
		return wire.ToolResult{}, err
	}
	if err := json.Unmarshal(call.Arguments, &args); err != nil {
		return wire.ToolResult{}, err
	}
	if args.Content == nil || !smallPlan(*args.Content) {
		return wire.ToolResult{}, errors.New("bounded UTF-8 plan content required")
	}
	if err := ctx.Err(); err != nil {
		return wire.ToolResult{}, err
	}
	ref, err := t.write(ctx, *args.Content)
	if err != nil {
		return wire.ToolResult{}, correlateCommit(err, t.Name(), call.ID)
	}
	fail := func(err error) (wire.ToolResult, error) {
		return wire.ToolResult{}, &CommittedError{Operation: t.Name(), CallID: call.ID, Err: err}
	}
	if ref != nil {
		if err := ref.Validate(); err != nil {
			return fail(err)
		}
	}
	if err := ctx.Err(); err != nil {
		return fail(err)
	}
	value, err := result(call.ID, struct {
		Saved bool            `json:"saved"`
		Notes *wire.Reference `json:"notes,omitempty"`
	}{true, ref}, false)
	if err != nil {
		return fail(err)
	}
	return value, nil
}

func smallPlan(content string) bool {
	return len(content) <= 64<<10 && utf8.ValidString(content) && !strings.ContainsRune(content, 0)
}
