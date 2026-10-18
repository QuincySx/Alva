// INPUT: context, errors, fmt, reflect, kernel/abi, llm/wire
// OUTPUT: Pre-intent execution preparation and atomic failed-preparation evidence
// POS: [L2 kernel runtime] Separates unchanged model declarations from frozen authorized tool execution inputs.
package core

import (
	"context"
	"errors"
	"fmt"
	"reflect"
	"time"

	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

func prepareToolCalls(ctx context.Context, runtime abi.RuntimeContext, declared []wire.ToolCall, tools map[string]abi.Tool, middleware []abi.Middleware, audits *[]*hookBatch) ([]wire.ToolCall, map[string]wire.ToolResult, int, error) {
	calls := make([]wire.ToolCall, len(declared))
	for i, call := range declared {
		calls[i] = wire.CloneToolCall(call)
	}
	replacements := make(map[string]wire.ToolResult)
	for i := range calls {
		batch := newHookBatch(runtime.RunID, "prepare_tool", calls[i].ID)
		*audits = append(*audits, batch)
		for layerIndex, layer := range middleware {
			if err := ctx.Err(); err != nil {
				return calls, replacements, i, err
			}
			hook, ok := layer.(abi.PrepareToolHook)
			if !ok {
				continue
			}
			owned := wire.CloneToolCall(calls[i])
			started := time.Now()
			preparation := abi.ToolPreparationContext{Runtime: runtime, ActualTool: tools[calls[i].Name]}
			decision, err := invokePrepareHook(ctx, preparation, hook, owned)
			batch.observe(layer, layerIndex, started, err)
			if err != nil {
				return calls, replacements, i, fmt.Errorf("prepare tool %q in %q: %w", calls[i].ID, layer.Name(), err)
			}
			if decision.Arguments != nil {
				calls[i].Arguments = append([]byte(nil), decision.Arguments...)
			}
			if decision.Result != nil {
				result := *decision.Result
				result.Content = wire.CloneContent(result.Content)
				result.CallID = calls[i].ID
				replacements[calls[i].ID] = result
				break
			}
		}
	}
	return calls, replacements, -1, ctx.Err()
}

func invokePrepareHook(ctx context.Context, preparation abi.ToolPreparationContext, hook abi.PrepareToolHook, call wire.ToolCall) (abi.ToolPreparation, error) {
	frozen := wire.CloneToolCall(call)
	decision, err := hook.PrepareTool(ctx, preparation, call)
	// Validation belongs to the observed invocation's effective outcome; a
	// nil hook error cannot make a mutated input/invalid union an "ok" audit.
	if !reflect.DeepEqual(call, frozen) {
		err = errors.Join(err, errors.New("prepare hook mutated its input; return Arguments explicitly"))
	}
	if err != nil {
		return abi.ToolPreparation{}, err
	}
	if decision.Arguments != nil && decision.Result != nil {
		return abi.ToolPreparation{}, errors.New("tool preparation cannot replace arguments and result together")
	}
	if decision.Arguments != nil && !wire.ValidJSON(decision.Arguments) {
		return abi.ToolPreparation{}, errors.New("prepared tool arguments must be valid UTF-8 JSON")
	}
	if decision.Result != nil {
		result := *decision.Result
		result.Content = wire.CloneContent(result.Content)
		if result.CallID != "" && result.CallID != frozen.ID {
			return abi.ToolPreparation{}, errors.New("prepared result has mismatched call ID")
		}
		result.CallID = frozen.ID
		if err := (wire.ContentBlock{Kind: wire.ContentToolResult, ToolResult: &result}).Validate(); err != nil {
			return abi.ToolPreparation{}, fmt.Errorf("invalid prepared result: %w", err)
		}
		decision.Result = &result
	}
	decision.Arguments = append([]byte(nil), decision.Arguments...)
	return decision, nil
}

// A preparation failure prevents the entire batch from executing. Keep the
// successful model outcome and settle all declared calls in that same atomic
// append. Only the failing preparation is failed; siblings are unexecuted.
func failedPreparationRecords(runID string, calls []wire.ToolCall, failed int, cause error) []abi.SessionRecord {
	records := make([]abi.SessionRecord, 0, len(calls)*2)
	for i, call := range calls {
		status := abi.OutcomeCancelled
		if i == failed {
			status = statusFor(cause)
		}
		result := wire.ToolResult{CallID: call.ID, IsError: true, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: "Not executed: tool preparation failed: " + cause.Error()}}}
		message := wire.Message{Role: wire.RoleTool, Content: []wire.ContentBlock{{Kind: wire.ContentToolResult, ToolResult: &result}}}
		records = append(records, abi.SessionRecord{Kind: abi.RecordToolOutcome, ToolOutcome: &abi.ToolOutcome{RunID: runID, CallID: call.ID, Status: status, Result: result, Error: cause.Error()}}, abi.SessionRecord{Kind: abi.RecordConversation, Message: &message})
	}
	return records
}
