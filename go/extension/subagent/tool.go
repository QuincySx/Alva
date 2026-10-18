// INPUT: bytes, context, encoding/json, errors, fmt, io, strings, time, unicode/utf8, agent/abi, kernel/abi, llm/wire, platform/system/httpjson.
// OUTPUT: ErrChildCleanup and ErrChildCompletion for non-recoverable child finalization failures.
// POS: [L4 extension] Delegation executes SDK-backed children inline and never relabels copied parent capabilities or unclosed work as complete.
package subagent

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/go/platform/system/httpjson"
	agentabi "github.com/QuincySx/Alva/go/sdk/agent/abi"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type agentTool struct{ plugin *Plugin }

var ErrChildCleanup = errors.New("child resource cleanup failed")
var ErrChildCompletion = errors.New("child completion callback failed")

func (*agentTool) Name() string { return "agent" }
func (*agentTool) Description() string {
	return "Explicitly delegate a bounded task to an independent child Agent. Grant only exact direct-parent tools; empty tools grants none. Optional registered template/model/communications only, never ambient config. Child gets independent records/references, not parent history. Returns a parent-owned selected child-output reference. Child may recurse within host limits; this is not a Hub WorkerSession."
}
func (t *agentTool) ParametersSchema() json.RawMessage {
	schema := map[string]any{"type": "object", "additionalProperties": false, "required": []string{"task"}, "properties": map[string]any{"task": map[string]any{"type": "string", "minLength": 1, "maxLength": 65536}, "role": map[string]any{"type": "string", "maxLength": 128}, "agent_type": map[string]any{"type": "string", "maxLength": 128}, "system_prompt": map[string]any{"type": "string", "maxLength": 16384}, "tools": map[string]any{"type": "array", "maxItems": 128, "items": map[string]any{"type": "string"}}, "model": map[string]any{"type": "string", "maxLength": 256}, "timeout_ms": map[string]any{"type": "integer", "minimum": 1}, "max_iterations": map[string]any{"type": "integer", "minimum": 1, "maximum": 256}, "comms": map[string]any{"type": "array", "maxItems": 4, "items": map[string]any{"type": "object", "additionalProperties": false, "required": []string{"kind", "config"}, "properties": map[string]any{"kind": map[string]string{"type": "string"}, "config": map[string]string{"type": "object"}}}}}}
	t.plugin.mu.Lock()
	active, state := t.plugin.scope, t.plugin.state
	t.plugin.mu.Unlock()
	if active != nil {
		names := t.plugin.grantableNames(active, state)
		if len(names) > 0 {
			schema["properties"].(map[string]any)["tools"].(map[string]any)["items"] = map[string]any{"type": "string", "enum": names}
		} else {
			schema["properties"].(map[string]any)["tools"].(map[string]any)["maxItems"] = 0
		}
	}
	data, _ := json.Marshal(schema)
	return data
}
func (*agentTool) Scheduling(wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{ConcurrentSafe: true, ExecutionMode: abi.Coordinator, ManagesOwnTimeout: true}
}
func (*agentTool) IsReadOnly(wire.ToolCall) bool { return false }
func (*agentTool) DescribeEffect(wire.ToolCall) abi.Effect {
	return abi.Effect{Kind: abi.EffectUnknown, Description: "Delegate explicit model work and direct-parent-approved capabilities to an independent bounded child session"}
}

type spawnInput struct {
	Task          string    `json:"task"`
	Role          string    `json:"role"`
	AgentType     string    `json:"agent_type"`
	SystemPrompt  string    `json:"system_prompt"`
	Tools         *[]string `json:"tools"`
	Model         string    `json:"model"`
	TimeoutMS     int64     `json:"timeout_ms"`
	MaxIterations int       `json:"max_iterations"`
	Comms         []struct {
		Kind   string          `json:"kind"`
		Config json.RawMessage `json:"config"`
	} `json:"comms"`
}

func (t *agentTool) Execute(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	p := t.plugin
	if execution.ActualTool != t || t != p.tool || call.ID == "" || execution.SessionID != p.config.SessionID || execution.Session == nil || execution.Session.ID() != p.config.SessionID || execution.CallID != call.ID || call.Name != "agent" {
		return wire.ToolResult{}, errors.New("subagent actual parent session/call mismatch")
	}
	p.mu.Lock()
	active, state, budget, runID := p.scope, p.state, p.budget, p.runID
	p.mu.Unlock()
	if active == nil || state == nil || runID == "" || runID != execution.RunID || state.Session().ID() != p.config.SessionID {
		return wire.ToolResult{}, errors.New("subagent Plugin is not bound to this active parent run")
	}
	if len(call.Arguments) > 128<<10 || !utf8.Valid(call.Arguments) {
		return wire.ToolResult{}, errors.New("subagent arguments exceed 128 KiB UTF-8")
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(call.Arguments, &fields); err != nil || fields == nil {
		return wire.ToolResult{}, errors.New("subagent input must be an object")
	}
	for _, value := range fields {
		if bytes.Equal(bytes.TrimSpace(value), []byte("null")) {
			return wire.ToolResult{}, errors.New("subagent fields cannot be null")
		}
	}
	var input spawnInput
	if err := httpjson.ValidateObject(call.Arguments, &input); err != nil {
		return wire.ToolResult{}, err
	}
	decoder := json.NewDecoder(bytes.NewReader(call.Arguments))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&input); err != nil {
		return wire.ToolResult{}, err
	}
	if decoder.Decode(new(any)) != io.EOF {
		return wire.ToolResult{}, errors.New("subagent input must be one object")
	}
	if strings.TrimSpace(input.Task) == "" || len(input.Task) > 64<<10 || len(input.Role) > 128 || len(input.SystemPrompt) > 16<<10 || len(input.AgentType) > 128 || len(input.Model) > 256 || len(input.Comms) > 4 || input.TimeoutMS < 0 || input.TimeoutMS > int64((30*time.Minute)/time.Millisecond) || input.MaxIterations < 0 || input.MaxIterations > 256 {
		return wire.ToolResult{}, errors.New("subagent input exceeds explicit field budget")
	}
	var granted []string
	if input.AgentType != "" {
		if p.config.Templates == nil {
			return wire.ToolResult{}, errors.New("no explicit subagent template registry")
		}
		template, ok := p.config.Templates.Get(input.AgentType)
		if !ok || !validTemplate(template) {
			return wire.ToolResult{}, errors.New("unknown explicit subagent template")
		}
		if input.Role == "" {
			input.Role = template.Role
		}
		if input.SystemPrompt == "" {
			input.SystemPrompt = template.SystemPrompt
		}
		if input.Model == "" {
			input.Model = template.Model
		}
		granted = append([]string(nil), template.Tools...)
	}
	if input.Role == "" {
		input.Role = "assistant"
	}
	if input.SystemPrompt == "" {
		input.SystemPrompt = "You are an independent child agent with role " + input.Role + ". Work only on the explicit task and granted capabilities."
	}
	if input.Tools != nil {
		granted = append([]string(nil), (*input.Tools)...)
	}
	if len(input.Role) > 128 || len(input.SystemPrompt) > 16<<10 || len(input.Model) > 256 || !utf8.ValidString(input.Role) || !utf8.ValidString(input.SystemPrompt) || !utf8.ValidString(input.Model) {
		return wire.ToolResult{}, errors.New("resolved subagent template exceeds field budgets")
	}
	if len(granted) > 128 {
		return wire.ToolResult{}, errors.New("subagent tool grant exceeds 128 names")
	}
	ceiling := map[string]bool{}
	for _, name := range p.grantableNames(active, state) {
		ceiling[name] = true
	}
	var tools []abi.Tool
	seen := map[string]bool{}
	for _, name := range granted {
		tool, exists := state.Tools().Get(name)
		if name == "agent" || !ceiling[name] || !exists || seen[name] {
			return wire.ToolResult{}, fmt.Errorf("tool %q is not a distinct direct-parent grant", name)
		}
		seen[name] = true
		tools = append(tools, tool)
	}
	for _, spec := range input.Comms {
		if spec.Kind == "" || len(spec.Kind) > 128 || len(spec.Config) > 16<<10 || p.config.Communications == nil || len(bytes.TrimSpace(spec.Config)) == 0 || bytes.TrimSpace(spec.Config)[0] != '{' {
			return wire.ToolResult{}, errors.New("unavailable or oversized child communication")
		}
		if _, ok := p.config.Communications.Get(spec.Kind); !ok {
			return wire.ToolResult{}, errors.New("unknown child communication kind")
		}
	}
	if err := p.config.Guard.Authorize(ctx, execution, call, t.DescribeEffect(call)); err != nil {
		return wire.ToolResult{}, err
	}
	if err := budget.admit(); err != nil {
		return wire.ToolResult{}, err
	}
	childScope, err := active.SpawnChild(ctx, agentabi.ChildScopeConfig{Role: input.Role, SystemPrompt: input.SystemPrompt, Timeout: time.Duration(input.TimeoutMS) * time.Millisecond, MaxIterations: input.MaxIterations}, granted)
	if err != nil {
		return wire.ToolResult{}, err
	}
	defer childScope.Complete()
	deadline, cancel := context.WithTimeout(ctx, childScope.Timeout())
	defer cancel()
	model := state.Model()
	if input.Model != "" {
		if p.config.Models == nil {
			return wire.ToolResult{}, errors.New("explicit model override requested but no configured resolver")
		}
		model, err = p.config.Models.Resolve(deadline, input.Model)
		if err != nil {
			return wire.ToolResult{}, err
		}
		if nilImplementation(model) {
			return wire.ToolResult{}, errors.New("model resolver returned no model")
		}
	}
	var hooks []abi.ContextHooks
	var callbacks []agentabi.OnChildComplete
	parentSnapshot, childSnapshot := active.Snapshot(), childScope.Snapshot()
	for _, spec := range input.Comms {
		communication, _ := p.config.Communications.Get(spec.Kind)
		handle, err := communication.Attach(deadline, agentabi.SpawnCommContext{ParentScopeID: parentSnapshot.ID, ParentSessionID: p.config.SessionID, ChildScopeID: childSnapshot.ID, ChildSessionID: childScope.SessionID(), Role: input.Role, Bus: execution.Capabilities}, spec.Config)
		if err != nil {
			return wire.ToolResult{}, err
		}
		if handle.OnComplete != nil && nilImplementation(handle.OnComplete) {
			return wire.ToolResult{}, errors.New("typed-nil child completion callback")
		}
		for _, hook := range handle.Hooks {
			if nilImplementation(hook) {
				return wire.ToolResult{}, errors.New("missing child communication context hook")
			}
		}
		hooks = append(hooks, handle.Hooks...)
		if handle.OnComplete != nil {
			callbacks = append(callbacks, handle.OnComplete)
		}
	}
	child, err := p.config.Factory.Prepare(deadline, ChildRequest{Parent: execution.Session, ParentRunID: runID, SpawnCallID: call.ID, Workspace: execution.Workspace, Scope: childScope, Model: model, Grants: tools, Hooks: hooks, Budget: budget, Templates: p.config.Templates, Models: p.config.Models, Communications: p.config.Communications})
	if err != nil {
		if child.Close != nil {
			cleanup, end := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
			closeErr := child.Close(cleanup)
			end()
			if closeErr != nil {
				err = errors.Join(err, ErrChildCleanup, closeErr)
			}
		}
		return wire.ToolResult{}, err
	}
	if nilImplementation(child.Runner) || nilImplementation(child.Session) || child.Session.ID() != childScope.SessionID() {
		cleanup, end := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
		defer end()
		var closeErr error
		if child.Close != nil {
			closeErr = child.Close(cleanup)
		}
		return wire.ToolResult{}, errors.Join(errors.New("factory did not bind the actual child scope/session"), closeErr)
	}
	var latest strings.Builder
	refused, nonText := false, 0
	childErr := child.Runner.Run(deadline, []wire.Message{wire.TextMessage(wire.RoleUser, input.Task)}, func(event abi.Event) error {
		if event.Record != nil && event.Record.SessionID != childScope.SessionID() {
			return errors.New("child event forged a different native session source")
		}
		switch event.Kind {
		case abi.EventToolStarted:
			latest.Reset()
			refused = false
			nonText = 0
		case abi.EventTextDelta:
			if latest.Len()+len(event.Text) > 1<<20 {
				return errors.New("selected child output exceeds 1 MiB")
			}
			latest.WriteString(event.Text)
		case abi.EventContentBlock:
			if event.ContentBlock != nil {
				text := ""
				switch event.ContentBlock.Kind {
				case wire.ContentText:
					text = event.ContentBlock.Text
				case wire.ContentRefusal:
					if event.ContentBlock.Refusal != nil {
						text = "[Refusal] " + event.ContentBlock.Refusal.Text
						refused = true
					}
				case wire.ContentReasoning, wire.ContentProviderState:
				default:
					nonText++
				}
				if latest.Len()+len(text) > 1<<20 {
					return errors.New("selected child output exceeds 1 MiB")
				}
				latest.WriteString(text)
			}
		}
		return nil
	})
	cleanup, end := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
	defer end()
	if child.Close != nil {
		if err := child.Close(cleanup); err != nil {
			childErr = errors.Join(childErr, ErrChildCleanup, err)
		}
	}
	var reference *wire.Reference
	if latest.Len() > 0 {
		retained, err := p.config.References.StoreText(cleanup, latest.String())
		if err == nil {
			err = retained.Validate()
		}
		if err != nil {
			childErr = errors.Join(childErr, &abi.SessionError{Operation: "retain selected child output", SessionID: p.config.SessionID, Err: err})
		} else {
			retained.Processing += "; Selected child final/partial output only from native session " + childScope.SessionID() + ", not child history"
			reference = &retained
		}
	}
	outcome := agentabi.SpawnResult{Text: latest.String(), IsError: childErr != nil || refused}
	if childErr != nil {
		outcome.Error = childErr.Error()
	}
	for _, callback := range callbacks {
		if err := callback.Call(cleanup, outcome); err != nil {
			childErr = errors.Join(childErr, ErrChildCompletion, err)
		}
	}
	status := "completed"
	if refused {
		status = "refused"
	}
	if childErr != nil {
		status = "failed"
		if abi.OnlyCancellation(childErr) {
			status = "cancelled"
			if errors.Is(childErr, context.DeadlineExceeded) {
				status = "timed_out"
			}
		}
	}
	data, err := json.Marshal(map[string]any{"child_session_id": childScope.SessionID(), "child_scope_id": childSnapshot.ID, "parent_link": child.Link, "selected_output": reference, "is_error": childErr != nil || refused, "outcome": status, "refused": refused, "non_text_blocks_not_delivered": nonText, "source_notice": "Only selected final/partial text/refusal is retained in this parent session. Non-text output remains in child records and is not delivered as fabricated text. Child complete history/artifacts/MCP logs remain independent; the parent link grants no text-reading permission. Interrupted work is not replayed."})
	if err != nil {
		return wire.ToolResult{}, errors.Join(childErr, err)
	}
	if len(data) > 4096 {
		return wire.ToolResult{}, errors.Join(childErr, errors.New("child result metadata exceeds 4 KiB"))
	}
	result := wire.ToolResult{CallID: call.ID, IsError: childErr != nil || refused, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: string(data)}}}
	if ctx.Err() != nil {
		return result, errors.Join(ctx.Err(), childErr)
	}
	if childErr != nil && abi.OnlyCancellation(childErr) {
		return result, nil
	}
	return result, childErr
}
