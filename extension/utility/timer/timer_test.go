// INPUT: context, encoding/json, errors, testing, time, agent/core, kernel/abi, llm/wire and portable timer
// OUTPUT: Standalone SDK timer execution, cancellation and self-validation regressions
// POS: [L4 extension] Runs the same Tool with an in-memory SDK session and no native Plugin, SQLite, host or filesystem.
package timer_test

import (
	"context"
	"encoding/json"
	"errors"
	"testing"
	"time"

	"github.com/QuincySx/Alva/extension/utility/timer"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type modelFunc func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error

func (fn modelFunc) Stream(ctx context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	return fn(ctx, request, emit)
}

func TestTimerRunsInStandaloneSDKWithMemoryOnly(t *testing.T) {
	turns := 0
	model := modelFunc(func(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		turns++
		if turns == 1 {
			if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "wait", Name: "sleep", Arguments: json.RawMessage(`{"duration_ms":1}`)}}); err != nil {
				return err
			}
		} else {
			found := false
			for _, message := range request.Messages {
				for _, block := range message.Content {
					if block.ToolResult != nil && block.ToolResult.CallID == "wait" {
						var receipt struct{ Completed bool }
						if len(block.ToolResult.Content) != 1 || json.Unmarshal([]byte(block.ToolResult.Content[0].Text), &receipt) != nil || !receipt.Completed || block.ToolResult.IsError {
							t.Fatal("timer completion was not real", block.ToolResult)
						}
						found = true
					}
				}
			}
			if !found {
				t.Fatal("no actual timer result in next request")
			}
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	session := core.NewMemorySession("portable-timer")
	agent, err := core.NewBuilder(model, session).Tool(timer.NewTool()).Build(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	if err := agent.Run(t.Context(), nil, func(abi.Event) error { return nil }); err != nil || turns != 2 {
		t.Fatal("standalone timer failed", turns, err)
	}
}

func TestTimerCancellationDoesNotClaimCompletion(t *testing.T) {
	tool := timer.NewTool()
	ctx, cancel := context.WithTimeout(t.Context(), 10*time.Millisecond)
	defer cancel()
	result, err := tool.Execute(ctx, abi.ToolExecutionContext{ActualTool: tool, CallID: "wait"}, wire.ToolCall{ID: "wait", Name: "sleep", Arguments: json.RawMessage(`{"duration_ms":300000}`)})
	if !errors.Is(err, context.DeadlineExceeded) || len(result.Content) != 0 {
		t.Fatal("cancelled wait claimed completion", result, err)
	}
}

func TestTimerValidatesWithoutHarnessMiddleware(t *testing.T) {
	tool := timer.NewTool()
	for _, raw := range []string{`{}`, `{"duration_ms":null}`, `{"duration_ms":-1}`, `{"duration_ms":300001}`, `{"duration_ms":0.5}`, `{"duration_ms":0,"unknown":true}`, `{"duration_ms":0} {}`} {
		if result, err := tool.Execute(t.Context(), abi.ToolExecutionContext{ActualTool: tool, CallID: "wait"}, wire.ToolCall{ID: "wait", Name: "sleep", Arguments: json.RawMessage(raw)}); err == nil || len(result.Content) != 0 {
			t.Fatal("invalid wait accepted", raw, result, err)
		}
	}
	if _, err := tool.Execute(t.Context(), abi.ToolExecutionContext{ActualTool: timer.NewTool(), CallID: "wait"}, wire.ToolCall{ID: "wait", Name: "sleep", Arguments: json.RawMessage(`{"duration_ms":0}`)}); err == nil {
		t.Fatal("another timer instance borrowed actual dispatch")
	}
}
