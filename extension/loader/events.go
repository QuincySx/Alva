// INPUT: Real runtime hooks, event-scoped native state and validated AEP action responses
// OUTPUT: Seven middleware hook implementations with invocation-bound pre-intent arguments and explicit mutation validation
// POS: [L4 extension] Native Plugin phase bridge; immutable model facts stay separate from effective execution inputs
package loader

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"time"

	"github.com/QuincySx/Alva/integration/protocol/aep"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type stateHandle struct {
	id, runID string
	runtime   abi.RuntimeContext
	messages  []wire.Message
}

func (p *Plugin) dispatch(ctx context.Context, runtime abi.RuntimeContext, event string, params map[string]any, messages []wire.Message) (aep.Action, error) {
	if !p.subscribed[event] {
		return aep.Action{Action: "continue"}, p.healthy()
	}
	if err := p.healthy(); err != nil {
		return aep.Action{}, err
	}
	if runtime.State == nil || !p.log.SameAuthority(runtime.State.Session()) || runtime.Workspace != p.config.Workspace {
		return aep.Action{}, errors.New("AEP event state is outside approved native authority")
	}
	select {
	case p.gate <- struct{}{}:
	case <-ctx.Done():
		return aep.Action{}, ctx.Err()
	}
	defer func() { <-p.gate }()
	if err := p.healthy(); err != nil {
		return aep.Action{}, err
	}
	p.mu.Lock()
	p.serial++
	handle := &stateHandle{id: fmt.Sprintf("s-%d", p.serial), runID: runtime.RunID, runtime: runtime, messages: messages}
	p.handle = handle
	p.mu.Unlock()
	defer func() {
		p.mu.Lock()
		if p.handle == handle {
			p.handle = nil
		}
		p.mu.Unlock()
	}()
	if params == nil {
		params = map[string]any{}
	}
	params["stateHandle"] = handle.id
	request, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	data, err := p.client.Call(request, "extension/"+event, params)
	if err != nil {
		return aep.Action{}, p.poison(err)
	}
	if err = p.healthy(); err != nil {
		return aep.Action{}, err
	}
	var action aep.Action
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	if decoder.Decode(&action) != nil || decoder.Decode(new(any)) != io.EOF || !legalAction(event, action) {
		// Authoring errors are retained as native facts, never applied and never
		// sent wholesale to the model. A journal failure is still a real error.
		_, err = p.journal.record(ctx, runtime.RunID, "invalid_action", map[string]string{"event": event, "error": "INVALID_ACTION; ignored, see retained response"})
		return aep.Action{Action: "continue"}, err
	}
	return action, nil
}

func legalAction(event string, a aep.Action) bool {
	allowed := false
	switch a.Action {
	case "continue":
		allowed = true
	case "block":
		allowed = event == "before_tool_call" || event == "on_llm_call_start" || event == "on_agent_start"
	case "modify", "replace_result":
		allowed = event == "before_tool_call"
	case "modify_result":
		allowed = event == "after_tool_call"
	case "modify_messages":
		allowed = event == "on_llm_call_start"
	case "modify_response":
		allowed = event == "on_llm_call_end"
	}
	if !allowed || len(a.Reason) > 4096 {
		return false
	}
	payloads := 0
	for _, value := range []json.RawMessage{a.ModifiedArguments, a.Result, a.Messages, a.Response} {
		if value != nil {
			payloads++
			if !wire.ValidJSON(value) {
				return false
			}
		}
	}
	switch a.Action {
	case "continue":
		return payloads == 0 && a.Reason == ""
	case "block":
		return payloads == 0 && a.Reason != ""
	case "modify":
		return payloads == 1 && len(a.ModifiedArguments) > 0 && bytes.TrimSpace(a.ModifiedArguments)[0] == '{'
	case "replace_result", "modify_result":
		return payloads == 1 && a.Result != nil
	case "modify_messages":
		return payloads == 1 && a.Messages != nil
	case "modify_response":
		return payloads == 1 && a.Response != nil
	}
	return false
}

func (p *Plugin) PrepareTool(ctx context.Context, preparation abi.ToolPreparationContext, call wire.ToolCall) (abi.ToolPreparation, error) {
	a, err := p.dispatch(ctx, preparation.Runtime, "before_tool_call", map[string]any{"toolCall": call}, nil)
	if err != nil {
		return abi.ToolPreparation{}, err
	}
	switch a.Action {
	case "modify":
		return abi.ToolPreparation{Arguments: a.ModifiedArguments}, nil
	case "replace_result":
		result, err := p.resultReference(ctx, call.ID, a.Result, true)
		return abi.ToolPreparation{Result: &result}, err
	case "block":
		result := wire.ToolResult{CallID: call.ID, IsError: true, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: "Blocked by approved Plugin: " + a.Reason}}}
		return abi.ToolPreparation{Result: &result}, nil
	default:
		return abi.ToolPreparation{}, nil
	}
}
func (p *Plugin) AfterTool(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall, result *wire.ToolResult) error {
	if execution.Session == nil || !p.log.SameAuthority(execution.Session) {
		return errors.New("AEP after-tool native authority mismatch")
	}
	p.mu.Lock()
	runtime := p.runtime
	p.mu.Unlock()
	if runtime.RunID != execution.RunID {
		return errors.New("AEP after-tool run identity mismatch")
	}
	resultDTO, err := aep.ToolResultToDTO(*result)
	if err != nil {
		return err
	}
	a, err := p.dispatch(ctx, runtime, "after_tool_call", map[string]any{"toolCall": call, "result": resultDTO}, nil)
	if err != nil {
		return err
	}
	if a.Action == "modify_result" {
		replacement, err := p.resultReference(ctx, call.ID, a.Result, true)
		if err != nil {
			return err
		}
		*result = replacement
	}
	return nil
}
func (p *Plugin) BeforeModel(ctx context.Context, runtime abi.RuntimeContext, request *abi.ModelRequest) error {
	protocolMessages := make([]aep.MessageDTO, 0, len(request.Messages))
	for _, message := range request.Messages {
		value, err := aep.MessageToDTO(message)
		if err != nil {
			return err
		}
		protocolMessages = append(protocolMessages, value)
	}
	a, err := p.dispatch(ctx, runtime, "on_llm_call_start", map[string]any{"messages": protocolMessages}, request.Messages)
	if err != nil {
		return err
	}
	switch a.Action {
	case "block":
		return errors.New("model call blocked by approved Plugin: " + a.Reason)
	case "modify_messages":
		if len(a.Messages) > 1<<20 {
			return errors.New("AEP message mutation exceeds 1 MiB")
		}
		var messages []aep.MessageDTO
		if bytes.TrimSpace(a.Messages)[0] != '[' || json.Unmarshal(a.Messages, &messages) != nil || len(messages) > 2048 {
			return errors.New("invalid AEP modified messages")
		}
		var replacement []wire.Message
		for _, message := range messages {
			value, err := aep.MessageFromDTO(message)
			if err != nil {
				return err
			}
			replacement = append(replacement, value)
		}
		request.Messages = replacement
	}
	return nil
}
func (p *Plugin) AfterModel(ctx context.Context, runtime abi.RuntimeContext, response *wire.Message) error {
	responseDTO, err := aep.MessageToDTO(*response)
	if err != nil {
		return err
	}
	a, err := p.dispatch(ctx, runtime, "on_llm_call_end", map[string]any{"response": responseDTO}, nil)
	if err != nil {
		return err
	}
	if a.Action == "modify_response" {
		if len(a.Response) > 1<<20 {
			return errors.New("AEP response mutation exceeds 1 MiB")
		}
		var protocolResponse aep.MessageDTO
		if json.Unmarshal(a.Response, &protocolResponse) != nil || protocolResponse.Role != wire.RoleAssistant {
			return errors.New("invalid AEP modified assistant response")
		}
		replacement, err := aep.MessageFromDTO(protocolResponse)
		if err != nil {
			return err
		}
		*response = replacement
	}
	return nil
}
func (p *Plugin) InputCommitted(ctx context.Context, runtime abi.RuntimeContext, message wire.Message) error {
	_, err := p.dispatch(ctx, runtime, "on_user_message", map[string]any{"message": map[string]string{"text": message.Text()}}, nil)
	return err
}
