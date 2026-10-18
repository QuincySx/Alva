// INPUT: context, json, fmt, strings, testing, agent/core, kernel/abi, wire
// OUTPUT: AfterModel content-limit and atomic-settlement capacity regression
// POS: [SDK runtime testing] Middleware cannot expand a response beyond the wire limit used to size atomic batches.
package core_test

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type oversizedAfterModel struct{}

func (oversizedAfterModel) Name() string { return "oversized-after-model" }
func (oversizedAfterModel) AfterModel(_ context.Context, _ abi.RuntimeContext, message *wire.Message) error {
	for i := 0; i <= wire.MaxStreamContentBlocks; i++ {
		call := wire.ToolCall{ID: fmt.Sprintf("call-%d", i), Name: "test", Arguments: json.RawMessage(`{}`)}
		message.Content = append(message.Content, wire.ContentBlock{Kind: wire.ContentToolCall, ToolCall: &call})
	}
	return nil
}

func TestAfterModelCannotExceedAtomicResponseCapacity(t *testing.T) {
	log := core.NewMemorySession("response-capacity")
	model := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	executions := 0
	tool := &testTool{execute: func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
		executions++
		return wire.ToolResult{}, nil
	}}
	agent := build(t, core.NewBuilder(model, log).Tool(tool).Middleware(oversizedAfterModel{}))
	err := agent.Run(t.Context(), nil, discard)
	if err == nil || !strings.Contains(err.Error(), "content block count limit") || executions != 0 {
		t.Fatal("oversized modified response admitted", executions, err)
	}
	models := 0
	for _, record := range records(t, log) {
		if record.ToolIntent != nil {
			t.Fatal("oversized response committed tool intent")
		}
		if record.ModelOutcome != nil {
			models++
			if record.ModelOutcome.Status != abi.OutcomeFailed {
				t.Fatal("oversized response not settled as failure")
			}
		}
	}
	if models != 1 {
		t.Fatal("model request left unsettled")
	}
}
