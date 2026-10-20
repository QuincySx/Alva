// INPUT: context, JSON/errors/strings/testing, planning and real memory SDK.
// OUTPUT: No-store plan/task execution and committed-receipt recovery through actual loop settlement.
// POS: [L4 extension] Mutation failures are observable without introducing a runtime or automatically replaying work.
package planning_test

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/extension/planning"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestPlanRunsAloneWithoutTaskServiceOrReferences(t *testing.T) {
	var saved string
	writes := 0
	tool, err := planning.NewPlanTool(func(ctx context.Context, content string) (*wire.Reference, error) {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		writes++
		saved = content
		return nil, nil
	})
	if err != nil {
		t.Fatal(err)
	}
	turns := 0
	model := modelFunc(func(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		turns++
		if turns == 1 {
			if len(request.Tools) != 1 || request.Tools[0].Name != "todo_write" {
				t.Fatal("plan installed unrelated tools", request.Tools)
			}
			if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "plan", Name: "todo_write", Arguments: json.RawMessage(`{"content":"small standalone plan"}`)}}); err != nil {
				return err
			}
		} else {
			if writes != 1 || saved != "small standalone plan" {
				t.Fatal("plan not saved", saved, writes)
			}
			last := request.Messages[len(request.Messages)-1].Content[0].ToolResult
			if last == nil || last.IsError || last.Content[0].Text != `{"saved":true}` {
				t.Fatal("plan needs an artifact reader", last)
			}
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	agent, err := core.NewBuilder(model, core.NewMemorySession("plan")).Tool(tool).Build(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	if err := agent.Run(t.Context(), nil, func(abi.Event) error { return nil }); err != nil || turns != 2 {
		t.Fatal(turns, err)
	}
	execution := abi.ToolExecutionContext{ActualTool: tool, CallID: "bad"}
	for _, raw := range []string{`{"content":null}`, `{"Content":"x"}`, `{"content":"x","content":"y"}`, `{"content":"\ud800"}`, `{"content":"x","owner":"foreign"}`} {
		if _, err := tool.Execute(t.Context(), execution, wire.ToolCall{ID: "bad", Name: tool.Name(), Arguments: json.RawMessage(raw)}); err == nil {
			t.Fatal("invalid plan reached storage", raw)
		}
	}
	if writes != 1 {
		t.Fatal("invalid input mutated plan")
	}
}

func TestCommittedReceiptFailureSurvivesLoopAndRecoversWithoutReplay(t *testing.T) {
	backend := &workflow{}
	storeErr := errors.New("artifact writer failed with private backend details")
	retains := 0
	tools, err := planning.NewTaskTools(planning.TaskToolsConfig{Service: backend, RetainDetail: func(context.Context, planning.TaskRecord) (wire.Reference, error) {
		retains++
		return wire.Reference{}, storeErr
	}})
	if err != nil {
		t.Fatal(err)
	}
	log := core.NewMemorySession("recover")
	model := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "create-once", Name: "task_create", Arguments: json.RawMessage(`{"subject":"committed once"}`)}}); err != nil {
			return err
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	builder := core.NewBuilder(model, log)
	for _, tool := range tools {
		builder.Tool(tool)
	}
	agent, err := builder.Build(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	err = agent.Run(t.Context(), nil, func(abi.Event) error { return nil })
	var committed *planning.CommittedError
	if !errors.As(err, &committed) || !errors.Is(err, storeErr) || committed.CallID != "create-once" || len(backend.created) != 1 || retains != 1 {
		t.Fatal("ambiguous mutation failure", err, len(backend.created), retains)
	}
	if committed.TaskID != backend.created[0].Task.ID {
		t.Fatal("missing recovery identity", committed)
	}
	messages, err := log.Messages(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	result := messages[len(messages)-1].Content[0].ToolResult
	var receipt struct {
		State  string `json:"business_state"`
		TaskID string `json:"task_id"`
		CallID string `json:"call_id"`
	}
	if result == nil || !result.IsError || json.Unmarshal([]byte(result.Content[0].Text), &receipt) != nil || receipt.State != "committed" || receipt.TaskID != committed.TaskID || receipt.CallID != "create-once" || strings.Contains(result.Content[0].Text, "private backend") {
		t.Fatal("kernel settlement lost safe recovery receipt", result)
	}
	// A new consumer can query actual business state without the broken writer.
	inline, err := planning.NewTaskTools(planning.TaskToolsConfig{Service: backend})
	if err != nil {
		t.Fatal(err)
	}
	get := inline[1]
	args, _ := json.Marshal(map[string]string{"task_id": receipt.TaskID})
	got, err := get.Execute(t.Context(), abi.ToolExecutionContext{ActualTool: get, CallID: "recover"}, wire.ToolCall{ID: "recover", Name: get.Name(), Arguments: args})
	if err != nil || got.IsError || !strings.Contains(got.Content[0].Text, "committed once") || len(backend.created) != 1 || retains != 1 {
		t.Fatal("recovery repeated mutation or needed artifact storage", got, err)
	}
}

func TestInlineOversizePreservesCommittedIdentity(t *testing.T) {
	backend := &workflow{}
	tools, err := planning.NewTaskTools(planning.TaskToolsConfig{Service: backend})
	if err != nil {
		t.Fatal(err)
	}
	create := tools[0]
	// Valid input can expand when JSON is encoded; never truncate business state.
	args, _ := json.Marshal(map[string]string{"subject": "large", "description": strings.Repeat("<", 4096)})
	_, err = create.Execute(t.Context(), abi.ToolExecutionContext{ActualTool: create, CallID: "large"}, wire.ToolCall{ID: "large", Name: create.Name(), Arguments: args})
	var committed *planning.CommittedError
	if !errors.As(err, &committed) || committed.TaskID == "" || len(backend.created) != 1 {
		t.Fatal("oversized output hid committed operation", err)
	}
}

func TestPlanCancellationAfterWriteKeepsKnownCommit(t *testing.T) {
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	writes := 0
	tool, err := planning.NewPlanTool(func(context.Context, string) (*wire.Reference, error) {
		writes++
		cancel()
		return nil, nil
	})
	if err != nil {
		t.Fatal(err)
	}
	result, err := tool.Execute(ctx, abi.ToolExecutionContext{ActualTool: tool, CallID: "cancelled-plan"}, wire.ToolCall{ID: "cancelled-plan", Name: tool.Name(), Arguments: json.RawMessage(`{"content":"already saved"}`)})
	var committed *planning.CommittedError
	if !errors.As(err, &committed) || !errors.Is(err, context.Canceled) || committed.CallID != "cancelled-plan" || writes != 1 || len(result.Content) != 0 {
		t.Fatal("cancellation erased commit", result, err, writes)
	}
}

type flushFailureService struct {
	workflow
	failure error
}

func (s *flushFailureService) Create(ctx context.Context, request planning.CreateRequest) (planning.TaskRecord, error) {
	value, err := s.workflow.Create(ctx, request)
	if err != nil {
		return value, err
	}
	return planning.TaskRecord{}, &planning.CommittedError{Operation: "backend-flush", TaskID: value.Task.ID, Err: s.failure}
}

func TestBackendCommitErrorGetsActualCallCorrelation(t *testing.T) {
	failure := errors.New("flush after confirmed commit failed")
	service := &flushFailureService{failure: failure}
	tools, err := planning.NewTaskTools(planning.TaskToolsConfig{Service: service})
	if err != nil {
		t.Fatal(err)
	}
	tool := tools[0]
	_, err = tool.Execute(t.Context(), abi.ToolExecutionContext{ActualTool: tool, CallID: "actual-call"}, wire.ToolCall{ID: "actual-call", Name: tool.Name(), Arguments: json.RawMessage(`{"subject":"one commit"}`)})
	var committed *planning.CommittedError
	if !errors.As(err, &committed) || !errors.Is(err, failure) || committed.CallID != "actual-call" || committed.Operation != "task_create" || len(service.created) != 1 || committed.TaskID != service.created[0].Task.ID {
		t.Fatal("backend commit error lost actual correlation", err)
	}
}
