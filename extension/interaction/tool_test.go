// INPUT: context, JSON, errors, strings, testing/time, portable interaction/references and memory-only SDK.
// OUTPUT: Real clarification references, synchronous cancellation and strict portable input regressions.
// POS: [L4 extension] The same question implementation runs without native storage, cwd, Guard or Plugin lifecycle.
package interaction_test

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/extension/interaction"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	kernel "github.com/QuincySx/Alva/sdk/kernel/core"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type modelFunc func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error

func (f modelFunc) Stream(ctx context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	return f(ctx, request, emit)
}

func TestQuestionToolRunsInStandaloneMemorySDK(t *testing.T) {
	refs, _ := agentcontext.NewMemoryReferences("portable-question", 1<<20)
	answer := strings.Repeat("actual human text ", 2000)
	asks := 0
	tool, err := interaction.NewTool(interaction.HandlerFunc(func(_ context.Context, q interaction.Question) (interaction.Answer, error) {
		asks++
		if q.SessionID != "portable-question" || q.RunID == "" || q.CallID != "ask" || q.Prompt != "Which result?" {
			t.Fatal("question lost actual runtime identity", q)
		}
		return interaction.Answer{Text: &answer}, nil
	}), refs)
	if err != nil {
		t.Fatal(err)
	}
	turns := 0
	model := modelFunc(func(ctx context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		turns++
		if turns == 1 {
			if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "ask", Name: "ask_human", Arguments: json.RawMessage(`{"question":"Which result?"}`)}}); err != nil {
				return err
			}
		} else {
			found := false
			for _, message := range request.Messages {
				for _, block := range message.Content {
					if block.ToolResult == nil || block.ToolResult.CallID != "ask" {
						continue
					}
					var receipt interaction.Receipt
					text := block.ToolResult.Content[0].Text
					if block.ToolResult.IsError || strings.Contains(text, answer) || json.Unmarshal([]byte(text), &receipt) != nil || receipt.Answer.Validate() != nil {
						t.Fatal("answer was inline or lost reference evidence", text)
					}
					page, err := refs.ReadText(ctx, receipt.Answer.URI, 0, 256)
					if err != nil || !strings.Contains(page.Text, "actual human text") || !page.Truncated {
						t.Fatal("full source was not retained for bounded retrieval", page, err)
					}
					found = true
				}
			}
			if !found {
				t.Fatal("no real question result")
			}
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	agent, err := core.NewBuilder(model, core.NewMemorySession("portable-question")).Tool(tool).Build(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	if err := agent.Run(t.Context(), nil, func(abi.Event) error { return nil }); err != nil || turns != 2 || asks != 1 {
		t.Fatal("standalone clarification failed", turns, asks, err)
	}
}

func TestQuestionCancellationJoinsHandlerAndRejectsLateAnswer(t *testing.T) {
	refs, _ := agentcontext.NewMemoryReferences("s", 1<<20)
	joined := false
	tool, err := interaction.NewTool(interaction.HandlerFunc(func(ctx context.Context, _ interaction.Question) (interaction.Answer, error) {
		<-ctx.Done()
		joined = true
		late := "late answer is not accepted"
		return interaction.Answer{Text: &late}, nil
	}), refs)
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(t.Context(), 10*time.Millisecond)
	defer cancel()
	result, err := tool.Execute(ctx, abi.ToolExecutionContext{ActualTool: tool, SessionID: "s", RunID: "r", CallID: "c"}, wire.ToolCall{ID: "c", Name: "ask_human", Arguments: json.RawMessage(`{"question":"wait"}`)})
	if !errors.Is(err, context.DeadlineExceeded) || !joined || len(result.Content) != 0 {
		t.Fatal("late answer succeeded or handler was not joined", result, joined, err)
	}
}

func TestQuestionValidatesWithoutHarness(t *testing.T) {
	refs, _ := agentcontext.NewMemoryReferences("s", 1<<20)
	asks := 0
	tool, _ := interaction.NewTool(interaction.HandlerFunc(func(context.Context, interaction.Question) (interaction.Answer, error) {
		asks++
		return interaction.Answer{}, errors.New("invalid input reached handler")
	}), refs)
	execution := abi.ToolExecutionContext{ActualTool: tool, SessionID: "s", RunID: "r", CallID: "c"}
	for _, raw := range []string{`{"question":"x","SessionID":"foreign"}`, `{"question":"x","question":"y"}`, `{"question":"\ud800"}`, `{"question":"x","options":[{"label":"A","Label":"B"},{"label":"C"}]}`} {
		if _, err := tool.Execute(t.Context(), execution, wire.ToolCall{ID: "c", Name: "ask_human", Arguments: json.RawMessage(raw)}); err == nil {
			t.Fatal("ambiguous question admitted", raw)
		}
	}
	if asks != 0 {
		t.Fatal("bad input reached a human")
	}
	var handler interaction.HandlerFunc
	if _, err := interaction.NewTool(handler, refs); err == nil {
		t.Fatal("typed nil handler admitted")
	}
}

func TestOrdinaryQuestionUsesHostToolTimeout(t *testing.T) {
	refs, _ := agentcontext.NewMemoryReferences("s", 1<<20)
	tool, err := interaction.NewTool(interaction.HandlerFunc(func(ctx context.Context, _ interaction.Question) (interaction.Answer, error) {
		deadline, ok := ctx.Deadline()
		if !ok || time.Until(deadline) > time.Second {
			return interaction.Answer{}, errors.New("ordinary question bypassed the host timeout")
		}
		<-ctx.Done()
		return interaction.Answer{}, ctx.Err()
	}), refs)
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
	defer cancel()
	call := wire.ToolCall{ID: "c", Name: tool.Name(), Arguments: json.RawMessage(`{"question":"wait under host policy"}`)}
	execution := abi.ToolExecutionContext{ActualTool: tool, SessionID: "s", RunID: "r", CallID: "c", Scheduling: tool.Scheduling(call)}
	timeout := kernel.ToolTimeoutMiddleware{Timeout: 10 * time.Millisecond}
	result, err := timeout.WrapTool(ctx, execution, call, tool.Execute)
	if !errors.Is(err, context.DeadlineExceeded) || len(result.Content) != 0 {
		t.Fatal("host timeout did not govern the actual question", result, err)
	}
}
