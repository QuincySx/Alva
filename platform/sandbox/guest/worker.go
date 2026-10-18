// INPUT: context, encoding/json, errors, os, strings, agent/core, agent/context, kernel/abi, llm/wire, sandbox/abi
// OUTPUT: Run
// POS: [platform / WASM host or guest] Actual shared SDK agent assembly in the guest with reference-first files, explicit escalation and audit reporting.
package guest

import (
	"context"
	"encoding/json"
	"errors"
	"strings"

	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/agent/core"
	kernelabi "github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
	"github.com/QuincySx/Alva/sdk/sandbox/abi"
)

func Run(ctx context.Context) (string, error) {
	config, err := LoadContext()
	if err != nil {
		return "", err
	}
	localRefs, err := agentcontext.NewMemoryReferences(config.SessionID, 8<<20)
	if err != nil {
		return "", err
	}
	refs := guestReferences{local: localRefs}
	session := core.NewMemorySession(config.SessionID)
	scriptNote := "No JavaScript runtime was delegated."
	if config.ScriptAvailable {
		scriptNote = "run_script uses a fresh separately isolated QuickJS WASM instance with this job's existing file/fetch grants; script failures can follow partial writes. It has no module loader or shell. Read its result reference explicitly."
	}
	builder := core.NewBuilder(ProxyModel{}, session).Workspace(config.Workspace).MaxIterations(config.MaxIterations).MaxConcurrentTools(1).MaxResponseBytes(abi.MaxResponseBytes).SystemPrompt(config.SystemPrompt + "\nYou run inside WASIp1. Only explicit mounted guest paths are accessible. File content references are volatile for this job, not durable attachments. Read/search only what this task needs. Host escalation leaves WASM and requires independent host approval. No shell is available. " + scriptNote)
	for _, tool := range FileTools(config, refs) {
		builder.Tool(tool)
	}
	if config.ScriptAvailable {
		builder.Tool(scriptTool{config: config, refs: refs})
	}
	builder.Tool(agentcontext.NewReferenceReadTool(refs, config.SessionID)).Tool(agentcontext.NewReferenceSearchTool(refs, config.SessionID)).Tool(remoteTool{config: config, refs: refs, kind: "fetch"})
	if config.Policy.AllowEscalationRequest {
		builder.Tool(remoteTool{config: config, refs: refs, kind: "request_escalation"})
	}
	if config.HistoryAvailable {
		builder.Tool(historyTool{sessionID: config.SessionID})
	}
	agent, err := builder.Build(ctx)
	if err != nil {
		return "", err
	}
	callNames := map[string]string{}
	input := wire.TextMessage(wire.RoleUser, config.Task)
	for _, ref := range config.Attachments {
		value := ref
		input.Content = append(input.Content, wire.ContentBlock{Kind: wire.ContentReference, Reference: &value})
	}
	err = agent.Run(ctx, []wire.Message{input}, func(event kernelabi.Event) error {
		if event.Kind == kernelabi.EventToolStarted && event.ToolCall != nil {
			callNames[event.ToolCall.ID] = event.ToolCall.Name
			return nil
		}
		if event.Kind != kernelabi.EventToolCompleted {
			return nil
		}
		audit := abi.AuditEvent{Version: abi.Version, Kind: "tool_completed"}
		if event.ToolResult != nil {
			audit.CallID = event.ToolResult.CallID
			audit.IsError = event.ToolResult.IsError
			var texts []string
			for _, content := range event.ToolResult.Content {
				if content.Kind == wire.ContentText {
					texts = append(texts, content.Text)
				} else if content.Kind == wire.ContentReference {
					texts = append(texts, "reference: "+content.Reference.URI)
				}
			}
			summary := strings.Join(texts, "\n")
			runes := []rune(summary)
			if len(runes) > 512 {
				runes = runes[:512]
				audit.Truncated = true
			}
			audit.Summary = string(runes)
		}
		audit.Tool = callNames[audit.CallID]
		delete(callNames, audit.CallID)
		return AppendAudit(audit)
	})
	if err != nil {
		return "", err
	}
	messages, err := session.Messages(ctx)
	if err != nil {
		return "", err
	}
	for i := len(messages) - 1; i >= 0; i-- {
		if messages[i].Role == wire.RoleAssistant {
			return messages[i].Text(), nil
		}
	}
	return "", errors.New("agent completed without assistant message")
}

type remoteTool struct {
	config abi.Context
	refs   agentcontext.ReferenceBackend
	kind   string
}

func (tool remoteTool) Name() string { return tool.kind }
func (tool remoteTool) Description() string {
	if tool.kind == "fetch" {
		return "Fetch through host domain grants. Response bytes become a bounded job-local reference, not automatic model context."
	}
	return "Request explicitly unsandboxed host command execution. Host separately authorizes guest cwd and asks approval; denial is a recoverable result."
}
func (tool remoteTool) ParametersSchema() json.RawMessage {
	if tool.kind == "fetch" {
		return json.RawMessage(`{"type":"object","properties":{"url":{"type":"string"}},"required":["url"],"additionalProperties":false}`)
	}
	return json.RawMessage(`{"type":"object","properties":{"command":{"type":"string"},"cwd":{"type":"string"},"timeout_ms":{"type":"integer","minimum":1,"maximum":3600000}},"required":["command","cwd","timeout_ms"],"additionalProperties":false}`)
}
func (tool remoteTool) Scheduling(wire.ToolCall) kernelabi.Scheduling {
	return kernelabi.Scheduling{ExecutionMode: kernelabi.SerialGlobal}
}
func (tool remoteTool) DescribeEffect(wire.ToolCall) kernelabi.Effect {
	kind := kernelabi.EffectNetwork
	if tool.kind != "fetch" {
		kind = kernelabi.EffectExecute
	}
	return kernelabi.Effect{Kind: kind, Description: tool.Description()}
}
func (remoteTool) IsReadOnly(wire.ToolCall) bool { return false }
func (tool remoteTool) Execute(ctx context.Context, execution kernelabi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	if execution.SessionID != tool.config.SessionID {
		return wire.ToolResult{}, errors.New("foreign guest execution")
	}
	var payload any
	isError := false
	if tool.kind == "fetch" {
		var args struct {
			URL string `json:"url"`
		}
		if err := decodeArgs(call.Arguments, &args); err != nil {
			return fileFailure(call, "Invalid fetch arguments"), nil
		}
		result, err := Fetch(ctx, abi.FetchRequest{Version: abi.Version, Method: "GET", URL: args.URL})
		if err != nil {
			return wire.ToolResult{}, err
		}
		payload = result
		isError = result.Error != "" || (result.Response != nil && result.Response.Status >= 400)
	} else {
		var args struct {
			Command   string `json:"command"`
			Cwd       string `json:"cwd"`
			TimeoutMS uint64 `json:"timeout_ms"`
		}
		if err := decodeArgs(call.Arguments, &args); err != nil {
			return fileFailure(call, "Invalid escalation arguments"), nil
		}
		if !strings.HasPrefix(args.Cwd, "/") {
			args.Cwd = tool.config.Workspace + "/" + args.Cwd
		}
		result, err := Escalate(ctx, abi.EscalationRequest{Version: abi.Version, Command: args.Command, Cwd: args.Cwd, TimeoutMS: args.TimeoutMS})
		if err != nil {
			return wire.ToolResult{}, err
		}
		payload = result
		isError = result.Error != "" || (result.Response != nil && result.Response.ExitCode != 0)
	}
	encoded, err := abi.Encode(payload, abi.MaxResponseBytes)
	if err != nil {
		return wire.ToolResult{}, err
	}
	if tool.refs == nil {
		return wire.ToolResult{}, errors.New("remote response reference backend unavailable")
	}
	ref, err := tool.refs.StoreText(ctx, string(encoded))
	if err != nil {
		return wire.ToolResult{}, err
	}
	ref.Processing = "Complete host response JSON (binary body base64); no media compression, extraction or model understanding; volatile for this job; inspect selected ranges only"
	return wire.ToolResult{CallID: call.ID, IsError: isError, Content: []wire.ContentBlock{{Kind: wire.ContentReference, Reference: &ref}}}, nil
}
