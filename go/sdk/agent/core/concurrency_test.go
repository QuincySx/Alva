// INPUT: context, errors, reflect, sync/atomic, testing, time, kernel/abi, llm/wire
// OUTPUT: Actual tool-wave concurrency, ordered commits and cancellation regressions
// POS: [L2.5 SDK assembly] Verifies scheduling declarations change execution without changing durable model order.
package core_test

import (
	"context"
	"errors"
	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
	"reflect"
	"sync/atomic"
	"testing"
	"time"
)

type parallelTool struct {
	testTool
	schedule func(wire.ToolCall) abi.Scheduling
}

func (tool *parallelTool) Scheduling(call wire.ToolCall) abi.Scheduling {
	if tool.schedule != nil {
		return tool.schedule(call)
	}
	return abi.Scheduling{ConcurrentSafe: true, Resources: []abi.ResourceClaim{{Resource: call.ID, Access: abi.AccessWrite}}}
}
func batchModel(ids []string) abi.LanguageModel {
	turn := 0
	return modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		turn++
		if turn == 1 {
			for _, id := range ids {
				if err := emit(callChunk(id)); err != nil {
					return err
				}
			}
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
}
func waitSignal(t *testing.T, signal <-chan string) string {
	t.Helper()
	select {
	case value := <-signal:
		return value
	case <-time.After(3 * time.Second):
		t.Fatal("tool did not reach concurrency barrier")
		return ""
	}
}
func TestParallelToolsActuallyOverlapButCommitInModelOrder(t *testing.T) {
	session := core.NewMemorySession("parallel")
	started, finished := make(chan string, 2), make(chan string, 2)
	first, second := make(chan struct{}), make(chan struct{})
	tool := &parallelTool{testTool: testTool{execute: func(ctx context.Context, _ abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
		started <- call.ID
		if call.ID == "first" {
			select {
			case <-first:
			case <-ctx.Done():
				return wire.ToolResult{}, ctx.Err()
			}
		} else {
			select {
			case <-second:
			case <-ctx.Done():
				return wire.ToolResult{}, ctx.Err()
			}
		}
		finished <- call.ID
		return wire.ToolResult{CallID: call.ID}, nil
	}}}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	agent := build(t, core.NewBuilder(batchModel([]string{"first", "second"}), session).Tool(tool).MaxConcurrentTools(2))
	done := make(chan error, 1)
	go func() { done <- agent.Run(ctx, nil, discard) }()
	waitSignal(t, started)
	waitSignal(t, started)
	close(second)
	if waitSignal(t, finished) != "second" {
		t.Fatal("completion fixture invalid")
	}
	close(first)
	if err := <-done; err != nil {
		t.Fatal(err)
	}
	var order []string
	for _, record := range records(t, session) {
		if record.ToolOutcome != nil {
			order = append(order, record.ToolOutcome.CallID)
		}
	}
	if !reflect.DeepEqual(order, []string{"first", "second"}) {
		t.Fatal("outcomes not in model order", order)
	}
}
func TestParallelFatalFailureSettlesStartedSiblingsAndBackfillsOnlyLaterWave(t *testing.T) {
	fixture := errors.New("parallel failure")
	var calls atomic.Int32
	barrier := make(chan string, 2)
	release := make(chan struct{})
	session := core.NewMemorySession("parallel-failure")
	tool := &parallelTool{schedule: func(call wire.ToolCall) abi.Scheduling {
		if call.ID == "third" {
			return abi.Scheduling{ExecutionMode: abi.SerialGlobal}
		}
		return abi.Scheduling{ConcurrentSafe: true}
	}, testTool: testTool{execute: func(ctx context.Context, _ abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
		calls.Add(1)
		barrier <- call.ID
		select {
		case <-release:
		case <-ctx.Done():
			return wire.ToolResult{}, ctx.Err()
		}
		if call.ID == "first" {
			return wire.ToolResult{}, fixture
		}
		return wire.ToolResult{CallID: call.ID}, nil
	}}}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	agent := build(t, core.NewBuilder(batchModel([]string{"first", "second", "third"}), session).Tool(tool))
	done := make(chan error, 1)
	go func() { done <- agent.Run(ctx, nil, discard) }()
	waitSignal(t, barrier)
	waitSignal(t, barrier)
	close(release)
	if err := <-done; !errors.Is(err, fixture) {
		t.Fatal(err)
	}
	if calls.Load() != 2 {
		t.Fatal("later wave executed", calls.Load())
	}
	var outcomes []*abi.ToolOutcome
	for _, record := range records(t, session) {
		if record.ToolOutcome != nil {
			outcomes = append(outcomes, record.ToolOutcome)
		}
	}
	if len(outcomes) != 3 || outcomes[0].Status != abi.OutcomeFailed || outcomes[1].Status != abi.OutcomeSucceeded || outcomes[2].Status != abi.OutcomeCancelled {
		t.Fatalf("wrong wave settlement: %#v", outcomes)
	}
}
func TestCancellationDoesNotRewriteSuccessfulToolAsUnexecuted(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	session := core.NewMemorySession("truthful-cancel")
	tool := &testTool{execute: func(_ context.Context, _ abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
		cancel()
		return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: "side effect completed"}}}, nil
	}}
	err := build(t, core.NewBuilder(batchModel([]string{"first", "second"}), session).Tool(tool)).Run(ctx, nil, discard)
	if !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	var outcomes []*abi.ToolOutcome
	for _, record := range records(t, session) {
		if record.ToolOutcome != nil {
			outcomes = append(outcomes, record.ToolOutcome)
		}
	}
	if len(outcomes) != 2 || outcomes[0].Status != abi.OutcomeSucceeded || outcomes[0].Result.Content[0].Text != "side effect completed" || outcomes[1].Status != abi.OutcomeCancelled {
		t.Fatalf("cancellation falsely rewrote execution facts: %#v", outcomes)
	}
}
