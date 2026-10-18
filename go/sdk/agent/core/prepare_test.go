// INPUT: context, encoding/json, errors, reflect, strings, sync/atomic, testing, time, kernel/abi, llm/wire
// OUTPUT: Pre-intent argument/result preparation and post-intent immutability regressions
// POS: [L2.5 SDK assembly] Verifies effective scheduling, original facts, atomic failed batches and no unauthorized next invocation.
package core_test

import (
	"context"
	"encoding/json"
	"errors"
	"reflect"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type prepareFixture struct {
	name string
	fn   func(context.Context, abi.ToolPreparationContext, wire.ToolCall) (abi.ToolPreparation, error)
}

func (hook prepareFixture) Name() string { return hook.name }
func (hook prepareFixture) PrepareTool(ctx context.Context, preparation abi.ToolPreparationContext, call wire.ToolCall) (abi.ToolPreparation, error) {
	return hook.fn(ctx, preparation, call)
}

func TestPreparedArgumentsRecomputeSchedulingAndPreserveModelDeclaration(t *testing.T) {
	session := core.NewMemorySession("prepared")
	started := make(chan string, 2)
	release := make(chan struct{})
	tool := &parallelTool{testTool: testTool{execute: func(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
		if string(call.Arguments) != `{"path":"shared"}` || len(execution.Scheduling.Resources) != 1 || execution.Scheduling.Resources[0].Resource != `{"path":"shared"}` {
			return wire.ToolResult{}, errors.New("execution did not use prepared arguments/claims")
		}
		started <- call.ID
		if call.ID == "first" {
			select {
			case <-release:
			case <-ctx.Done():
				return wire.ToolResult{}, ctx.Err()
			}
		}
		return wire.ToolResult{CallID: call.ID}, nil
	}}, schedule: func(call wire.ToolCall) abi.Scheduling {
		return abi.Scheduling{ConcurrentSafe: true, Resources: []abi.ResourceClaim{{Resource: string(call.Arguments), Access: abi.AccessWrite}}}
	}}
	turn := 0
	model := modelFunc(func(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		turn++
		if turn == 1 {
			for _, id := range []string{"first", "second"} {
				call := wire.ToolCall{ID: id, Name: "test", Arguments: json.RawMessage(`{"path":"` + id + `"}`)}
				if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &call}); err != nil {
					return err
				}
			}
		} else {
			for _, message := range request.Messages {
				for _, block := range message.Content {
					if block.ToolCall != nil && string(block.ToolCall.Arguments) != `{"path":"`+block.ToolCall.ID+`"}` {
						return errors.New("durable assistant declaration was rewritten")
					}
				}
			}
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	hook := prepareFixture{"rewrite", func(context.Context, abi.ToolPreparationContext, wire.ToolCall) (abi.ToolPreparation, error) {
		return abi.ToolPreparation{Arguments: json.RawMessage(`{"path":"shared"}`)}, nil
	}}
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	agent := build(t, core.NewBuilder(model, session).Tool(tool).Middleware(hook))
	done := make(chan error, 1)
	go func() { done <- agent.Run(ctx, nil, discard) }()
	if waitSignal(t, started) != "first" {
		t.Fatal("changed model call order")
	}
	select {
	case <-started:
		t.Fatal("prepared resource conflict did not serialize execution")
	case <-time.After(20 * time.Millisecond):
	}
	close(release)
	if waitSignal(t, started) != "second" {
		t.Fatal("second call missing")
	}
	if err := <-done; err != nil {
		t.Fatal(err)
	}
	count := 0
	for _, record := range records(t, session) {
		if intent := record.ToolIntent; intent != nil {
			count++
			if intent.DeclaredCall == nil || string(intent.DeclaredCall.Arguments) != `{"path":"`+intent.Call.ID+`"}` || string(intent.Call.Arguments) != `{"path":"shared"}` || intent.NoSideEffect || intent.ReadOnly {
				t.Fatalf("lost declaration/effective distinction: %#v", intent)
			}
		}
	}
	if count != 2 {
		t.Fatal("missing intents", count)
	}
}

type executionObserver struct{ count *atomic.Int32 }

func (hook executionObserver) Name() string { return "execution-observer" }
func (hook executionObserver) BeforeTool(context.Context, abi.ToolExecutionContext, wire.ToolCall) error {
	hook.count.Add(1)
	return nil
}
func (hook executionObserver) WrapTool(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall, next abi.ToolFunc) (wire.ToolResult, error) {
	hook.count.Add(1)
	return next(ctx, execution, call)
}
func (hook executionObserver) AfterTool(context.Context, abi.ToolExecutionContext, wire.ToolCall, *wire.ToolResult) error {
	hook.count.Add(1)
	return nil
}

func TestPreparedResultSkipsToolAndExecutionHooksButRemainsModelRecoverable(t *testing.T) {
	session := core.NewMemorySession("replacement")
	var executions, innerPrepares, executionHooks atomic.Int32
	tool := &testTool{execute: func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
		executions.Add(1)
		return wire.ToolResult{}, nil
	}}
	outer := prepareFixture{"outer", func(context.Context, abi.ToolPreparationContext, wire.ToolCall) (abi.ToolPreparation, error) {
		return abi.ToolPreparation{Result: &wire.ToolResult{IsError: true, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: "policy blocked"}}}}, nil
	}}
	inner := prepareFixture{"inner", func(context.Context, abi.ToolPreparationContext, wire.ToolCall) (abi.ToolPreparation, error) {
		innerPrepares.Add(1)
		return abi.ToolPreparation{}, nil
	}}
	if err := build(t, core.NewBuilder(batchModel([]string{"one"}), session).Tool(tool).Middleware(outer).Middleware(inner).Middleware(executionObserver{&executionHooks})).Run(context.Background(), nil, discard); err != nil {
		t.Fatal(err)
	}
	if executions.Load() != 0 || innerPrepares.Load() != 0 || executionHooks.Load() != 0 {
		t.Fatal("replacement invoked tool/inner hooks")
	}
	var intent *abi.ToolIntent
	var outcome *abi.ToolOutcome
	for _, record := range records(t, session) {
		if record.ToolIntent != nil {
			intent = record.ToolIntent
		}
		if record.ToolOutcome != nil {
			outcome = record.ToolOutcome
		}
	}
	if intent == nil || !intent.NoSideEffect || outcome == nil || outcome.Status != abi.OutcomeFailed || !outcome.Result.IsError || outcome.Error != "" {
		t.Fatal("replacement evidence missing", intent, outcome)
	}
}

type prepareBatchLog struct {
	*core.MemorySession
	responseBatch []abi.SessionRecord
}

func (log *prepareBatchLog) AppendRecords(ctx context.Context, records []abi.SessionRecord) ([]abi.SessionRecord, error) {
	if len(records) > 0 && records[0].Message != nil && records[0].Message.Role == wire.RoleAssistant {
		log.responseBatch = abi.CloneRecords(records)
	}
	return log.MemorySession.AppendRecords(ctx, records)
}

func TestPreparationFailureAtomicallySettlesBatchWithoutFailingCompletedModel(t *testing.T) {
	session := &prepareBatchLog{MemorySession: core.NewMemorySession("prepare-fail")}
	failure := errors.New("AEP connection lost")
	var executions atomic.Int32
	tool := &testTool{execute: func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
		executions.Add(1)
		return wire.ToolResult{}, nil
	}}
	hook := prepareFixture{"prepare-fail", func(_ context.Context, _ abi.ToolPreparationContext, call wire.ToolCall) (abi.ToolPreparation, error) {
		if call.ID == "second" {
			return abi.ToolPreparation{}, failure
		}
		return abi.ToolPreparation{Arguments: json.RawMessage(`{"path":"effective"}`)}, nil
	}}
	err := build(t, core.NewBuilder(batchModel([]string{"first", "second"}), session).Tool(tool).Middleware(hook)).Run(context.Background(), nil, discard)
	if !errors.Is(err, failure) || executions.Load() != 0 || len(session.responseBatch) != 8 {
		t.Fatal("preparation not atomically settled", err, executions.Load(), len(session.responseBatch))
	}
	var statuses []abi.OutcomeStatus
	for _, record := range session.responseBatch {
		if record.ModelOutcome != nil && record.ModelOutcome.Status != abi.OutcomeSucceeded {
			t.Fatal("completed model incorrectly failed")
		}
		if record.ToolIntent != nil && !record.ToolIntent.NoSideEffect {
			t.Fatal("unexecuted batch falsely implies execution")
		}
		if record.ToolOutcome != nil {
			statuses = append(statuses, record.ToolOutcome.Status)
			if !strings.Contains(record.ToolOutcome.Error, failure.Error()) || !strings.HasPrefix(record.ToolOutcome.Result.Content[0].Text, "Not executed") {
				t.Fatal("failure evidence missing")
			}
		}
	}
	if !reflect.DeepEqual(statuses, []abi.OutcomeStatus{abi.OutcomeCancelled, abi.OutcomeFailed}) {
		t.Fatal(statuses)
	}
}

type argumentMutation struct{ before, swallow bool }

func (hook argumentMutation) Name() string { return "post-intent-mutation" }
func (hook argumentMutation) BeforeTool(_ context.Context, _ abi.ToolExecutionContext, call wire.ToolCall) error {
	if hook.before {
		copy(call.Arguments, []byte(`{"path":"other"}`))
	}
	return nil
}
func (hook argumentMutation) WrapTool(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall, next abi.ToolFunc) (wire.ToolResult, error) {
	if hook.before {
		return next(ctx, execution, call)
	}
	call.Arguments = json.RawMessage(`{"path":"other"}`)
	result, err := next(ctx, execution, call)
	if hook.swallow {
		return wire.ToolResult{CallID: call.ID}, nil
	}
	return result, err
}

func TestPostIntentMutationCannotExecuteOrHideNextViolation(t *testing.T) {
	for _, hook := range []argumentMutation{{before: true}, {}, {swallow: true}} {
		var executions atomic.Int32
		session := core.NewMemorySession("mutation")
		tool := &testTool{execute: func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
			executions.Add(1)
			return wire.ToolResult{}, nil
		}}
		err := build(t, core.NewBuilder(batchModel([]string{"first", "second"}), session).Tool(tool).Middleware(hook)).Run(context.Background(), nil, discard)
		if err == nil || executions.Load() != 0 {
			t.Fatal("post-intent mutation executed", err, executions.Load())
		}
		var outcomes []*abi.ToolOutcome
		for _, record := range records(t, session) {
			if record.ToolIntent != nil && string(record.ToolIntent.Call.Arguments) != `{"path":"notes"}` {
				t.Fatal("middleware mutated durable intent")
			}
			if record.ToolOutcome != nil {
				outcomes = append(outcomes, record.ToolOutcome)
			}
		}
		if len(outcomes) != 2 || outcomes[0].Status != abi.OutcomeFailed || outcomes[1].Status != abi.OutcomeCancelled {
			t.Fatal("mutation batch not settled", outcomes)
		}
	}
}

func TestInvalidPreparationNeverExecutesOrLeavesDanglingCalls(t *testing.T) {
	for _, decide := range []func(wire.ToolCall) abi.ToolPreparation{
		func(wire.ToolCall) abi.ToolPreparation {
			return abi.ToolPreparation{Arguments: json.RawMessage(`{invalid}`)}
		},
		func(wire.ToolCall) abi.ToolPreparation {
			return abi.ToolPreparation{Arguments: json.RawMessage(`{}`), Result: &wire.ToolResult{}}
		},
		func(wire.ToolCall) abi.ToolPreparation {
			return abi.ToolPreparation{Result: &wire.ToolResult{CallID: "foreign"}}
		},
		func(call wire.ToolCall) abi.ToolPreparation { call.Arguments[0] = 'x'; return abi.ToolPreparation{} },
	} {
		var executed atomic.Int32
		session := core.NewMemorySession("invalid-prepare")
		tool := &testTool{execute: func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
			executed.Add(1)
			return wire.ToolResult{}, nil
		}}
		hook := prepareFixture{"invalid", func(_ context.Context, _ abi.ToolPreparationContext, call wire.ToolCall) (abi.ToolPreparation, error) {
			return decide(call), nil
		}}
		err := build(t, core.NewBuilder(batchModel([]string{"one"}), session).Tool(tool).Middleware(hook)).Run(context.Background(), nil, discard)
		if err == nil || executed.Load() != 0 {
			t.Fatal("invalid preparation executed", err)
		}
		intents, outcomes := 0, 0
		for _, record := range records(t, session) {
			if record.ToolIntent != nil {
				intents++
			}
			if record.ToolOutcome != nil {
				outcomes++
			}
			if record.ModelOutcome != nil && record.ModelOutcome.Status != abi.OutcomeSucceeded {
				t.Fatal("completed model misreported")
			}
		}
		if intents != 1 || outcomes != 1 {
			t.Fatal("dangling prepared call", intents, outcomes)
		}
	}
}

func TestCanceledPreparationCommitsEvidenceWithoutCanceledStorageContext(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	session := core.NewMemorySession("cancel-prepare")
	hook := prepareFixture{"cancel", func(context.Context, abi.ToolPreparationContext, wire.ToolCall) (abi.ToolPreparation, error) {
		cancel()
		return abi.ToolPreparation{}, context.Canceled
	}}
	err := build(t, core.NewBuilder(batchModel([]string{"one", "two"}), session).Tool(&testTool{}).Middleware(hook)).Run(ctx, nil, discard)
	if !abi.OnlyCancellation(err) {
		t.Fatal("ordinary canceled preparation misclassified", err)
	}
	outcomes := 0
	for _, record := range records(t, session) {
		if record.ModelOutcome != nil && record.ModelOutcome.Status != abi.OutcomeSucceeded {
			t.Fatal("completed model canceled")
		}
		if record.ToolOutcome != nil {
			outcomes++
			if record.ToolOutcome.Status != abi.OutcomeCancelled {
				t.Fatal("wrong cancellation evidence")
			}
		}
	}
	if outcomes != 2 {
		t.Fatal("canceled preparation lost outcomes", outcomes)
	}
}
