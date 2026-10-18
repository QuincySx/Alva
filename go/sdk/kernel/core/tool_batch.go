// INPUT: context, errors, fmt, sync, kernel/abi, llm/wire
// OUTPUT: ToolBatchCoordinator
// POS: [L2 kernel runtime] Conflict-aware bounded waves with model-order durable settlement and truthful cancellation.
package core

import (
	"context"
	"errors"
	"fmt"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/kernel/bus"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
	"sync"
)

type ToolBatchCoordinator struct{}
type ToolBatchConfig struct {
	Session            abi.ConversationLog
	Middleware         []abi.Middleware
	Workspace          string
	Bus                bus.Handle
	MaxConcurrentTools int
	PreparedResults    map[string]wire.ToolResult
}
type executedCall struct {
	result  wire.ToolResult
	err     error
	started bool
	audit   *toolHookAudit
}

func (ToolBatchCoordinator) Execute(ctx context.Context, config ToolBatchConfig, runID string, calls []wire.ToolCall, intents []abi.SessionRecord, tools map[string]abi.Tool, runtime *ContextRuntime, emit func(abi.Event) error) error {
	if len(calls) != len(intents) {
		return errors.New("tool intent count mismatch")
	}
	limit := config.MaxConcurrentTools
	if limit <= 0 {
		limit = 8
	}
	if limit > 256 {
		return finishUnstarted(ctx, config.Session, runID, calls, errors.New("tool concurrency limit exceeds 256"))
	}
	var emitMu sync.Mutex
	safeEmit := func(event abi.Event) error { emitMu.Lock(); defer emitMu.Unlock(); return emit(event) }
	for cursor := 0; cursor < len(calls); {
		if err := ctx.Err(); err != nil {
			return finishUnstarted(ctx, config.Session, runID, calls[cursor:], err)
		}
		wave := waveLength(intents[cursor:], limit, config.Workspace)
		for i := 0; i < wave; i++ {
			reference := reference(config.Session.ID(), intents[cursor+i])
			call := wire.CloneToolCall(calls[cursor+i])
			if err := safeEmit(abi.Event{Kind: abi.EventToolStarted, ToolCall: &call, Record: &reference}); err != nil {
				return finishUnstarted(ctx, config.Session, runID, calls[cursor:], err)
			}
		}
		outcomes := make([]executedCall, wave)
		var wait sync.WaitGroup
		wait.Add(wave)
		for i := 0; i < wave; i++ {
			go func(index int) {
				defer wait.Done()
				if err := ctx.Err(); err != nil {
					outcomes[index].err = err
					return
				}
				call := calls[cursor+index]
				if result, exists := config.PreparedResults[call.ID]; exists {
					if !intents[cursor+index].ToolIntent.NoSideEffect {
						outcomes[index].err = errors.New("prepared result requires committed no-tool-execution intent")
						return
					}
					result.Content = wire.CloneContent(result.Content)
					outcomes[index].result = result
					return
				}
				if intents[cursor+index].ToolIntent.NoSideEffect {
					outcomes[index].err = errors.New("no-tool-execution intent missing prepared result")
					return
				}
				execution := abi.ToolExecutionContext{SessionID: config.Session.ID(), Session: config.Session, RunID: runID, CallID: call.ID, Workspace: config.Workspace, Progress: callbackSink{emit: safeEmit}, Capabilities: config.Bus, Scheduling: intents[cursor+index].ToolIntent.Scheduling, ActualTool: tools[call.Name]}
				outcomes[index].started = true
				outcomes[index].audit = newToolHookAudit(runID, call.ID)
				outcomes[index].result, outcomes[index].err = executeTool(ctx, execution, tools[call.Name], call, config.Middleware, outcomes[index].audit)
			}(i)
		}
		wait.Wait()
		var waveErr error
		for i, outcome := range outcomes {
			call := calls[cursor+i]
			result, executionErr := outcome.result, outcome.err
			if result.CallID != "" && result.CallID != call.ID {
				executionErr = errors.Join(executionErr, errors.New("tool returned a mismatched call ID"))
			}
			result.CallID = call.ID
			if err := (wire.ContentBlock{Kind: wire.ContentToolResult, ToolResult: &result}).Validate(); err != nil {
				executionErr = errors.Join(executionErr, fmt.Errorf("invalid tool result: %w", err))
			}
			status := abi.OutcomeSucceeded
			if result.IsError || executionErr != nil {
				status = abi.OutcomeFailed
			}
			if executionErr != nil && ctx.Err() != nil {
				executionErr = errors.Join(executionErr, ctx.Err())
				status = statusFor(executionErr)
			}
			if executionErr != nil {
				result.IsError = true
				prefix := ""
				if !outcome.started {
					prefix = "Not executed: "
				}
				result.Content = []wire.ContentBlock{{Kind: wire.ContentText, Text: prefix + executionErr.Error()}}
			}
			fact := &abi.ToolOutcome{RunID: runID, CallID: call.ID, Status: status, Result: result}
			if executionErr != nil {
				fact.Error = executionErr.Error()
			}
			message := wire.Message{Role: wire.RoleTool, Content: []wire.ContentBlock{{Kind: wire.ContentToolResult, ToolResult: &result}}}
			cleanup, cancel := settlementContext(ctx)
			records, commitErr := appendRecords(cleanup, config.Session, []abi.SessionRecord{{Kind: abi.RecordToolOutcome, ToolOutcome: fact}, {Kind: abi.RecordConversation, Message: &message}})
			// Settle real execution facts FIRST. Observation failure must not
			// erase its outcome, suggest NoSideEffect, or re-execute the call.
			if commitErr == nil && outcome.audit != nil {
				waveErr = errors.Join(waveErr, appendHookBatches(cleanup, config.Session, outcome.audit.before, outcome.audit.wrap, outcome.audit.after))
			}
			cancel()
			waveErr = errors.Join(waveErr, executionErr)
			if commitErr != nil {
				waveErr = errors.Join(waveErr, fmt.Errorf("commit tool outcome: %w", commitErr))
				continue
			}
			if runtime != nil {
				waveErr = errors.Join(waveErr, runtime.OnMessage(ctx, *records[1].Message))
			}
			reference := reference(config.Session.ID(), records[0])
			waveErr = errors.Join(waveErr, safeEmit(abi.Event{Kind: abi.EventToolCompleted, ToolResult: &result, Record: &reference}))
		}
		cursor += wave
		waveErr = errors.Join(waveErr, ctx.Err())
		if waveErr != nil {
			return finishUnstarted(ctx, config.Session, runID, calls[cursor:], waveErr)
		}
	}
	return nil
}
func waveLength(intents []abi.SessionRecord, limit int, workspace string) int {
	first := intents[0].ToolIntent.Scheduling
	if !first.ConcurrentSafe || first.ExecutionMode == abi.SerialGlobal {
		return 1
	}
	wave := 1
	for wave < len(intents) && wave < limit {
		next := intents[wave].ToolIntent.Scheduling
		if !next.ConcurrentSafe || next.ExecutionMode == abi.SerialGlobal {
			break
		}
		conflict := false
		for prior := 0; prior < wave; prior++ {
			if abi.SchedulingConflicts(intents[prior].ToolIntent.Scheduling, next, workspace) {
				conflict = true
				break
			}
		}
		if conflict {
			break
		}
		wave++
	}
	return wave
}
func finishUnstarted(ctx context.Context, session abi.ConversationLog, runID string, calls []wire.ToolCall, cause error) error {
	cleanup, cancel := settlementContext(ctx)
	defer cancel()
	return errors.Join(cause, settleUnexecuted(cleanup, session, runID, calls, cause))
}
