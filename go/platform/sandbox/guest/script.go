// INPUT: Advertised job-local isolated QuickJS capability and explicit strict script source
// OUTPUT: run_script Tool returning bounded reference-only success or recoverable script error
// POS: [platform / WASM guest] The SDK worker stays alive while the host owns each fresh script WASM instance
package guest

import (
	"context"
	"encoding/json"
	"errors"

	agentcontext "github.com/QuincySx/Alva/go/sdk/agent/context"
	kernelabi "github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
	"github.com/QuincySx/Alva/go/sdk/sandbox/abi"
)

type scriptTool struct {
	config abi.Context
	refs   agentcontext.ReferenceBackend
}

func (scriptTool) Name() string { return "run_script" }
func (scriptTool) Description() string {
	return "Evaluate one strict global JavaScript source in a fresh isolated QuickJS-NG WASM instance. Synchronous readFile/writeFile/appendFile/exists/remove/rename/mkdir/readDir/stat/glob/copyFile/readJson/writeJson/print/fetch use only this job's existing guest-path grants and host fetch allowlist. No modules, require, npm, Node, shell or ambient credentials. 10-second script deadline, 32MiB tracked heap, 512KiB soft C stack and 64MiB isolated WASM memory. Script errors are recoverable; complete bounded result/output stays in a job-local reference."
}
func (scriptTool) ParametersSchema() json.RawMessage {
	return json.RawMessage(`{"type":"object","additionalProperties":false,"required":["script"],"properties":{"script":{"type":"string","minLength":1,"maxLength":65536}}}`)
}
func (scriptTool) Scheduling(wire.ToolCall) kernelabi.Scheduling {
	return kernelabi.Scheduling{ExecutionMode: kernelabi.SerialGlobal, ManagesOwnTimeout: true}
}
func (scriptTool) IsReadOnly(wire.ToolCall) bool { return false }
func (scriptTool) DescribeEffect(wire.ToolCall) kernelabi.Effect {
	return kernelabi.Effect{Kind: kernelabi.EffectUnknown, Description: "Execute explicit QuickJS source using only pre-granted job file/network capabilities; writes may be partial when a script fails"}
}
func (t scriptTool) Execute(ctx context.Context, e kernelabi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	if !t.config.ScriptAvailable || e.SessionID != t.config.SessionID || t.refs == nil {
		return wire.ToolResult{}, errors.New("actual script capability/session/reference authority required")
	}
	var args struct {
		Script string `json:"script"`
	}
	if err := decodeArgs(call.Arguments, &args); err != nil {
		return fileFailure(call, "Invalid strict script arguments"), nil
	}
	request := abi.ScriptRequest{Version: abi.Version, Script: args.Script}
	if err := request.Validate(); err != nil {
		return fileFailure(call, err.Error()), nil
	}
	if err := ctx.Err(); err != nil {
		return wire.ToolResult{}, err
	}
	encoded, err := abi.Encode(request, abi.MaxScriptBytes*6+1024)
	if err != nil {
		return wire.ToolResult{}, err
	}
	data, err := exchange(abi.OpScript, encoded, abi.MaxScriptResultBytes)
	if err != nil {
		return wire.ToolResult{}, err
	}
	var result abi.ScriptResult
	if err = abi.Decode(data, abi.MaxScriptResultBytes, &result); err != nil {
		return wire.ToolResult{}, err
	}
	if err = result.Validate(); err != nil {
		return wire.ToolResult{}, err
	}
	ref, err := t.refs.StoreText(ctx, string(data))
	if err != nil {
		return wire.ToolResult{}, err
	}
	ref.Processing = "Complete bounded isolated QuickJS result/output; script failure may follow partial writes. Job-local reference; no automatic model expansion or replay."
	return wire.ToolResult{CallID: call.ID, IsError: result.Error != "", Content: []wire.ContentBlock{{Kind: wire.ContentReference, Reference: &ref}}}, nil
}
