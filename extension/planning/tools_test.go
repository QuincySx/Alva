// INPUT: context, JSON, errors, testing, portable planning/reference capabilities and memory-only SDK.
// OUTPUT: Standalone workflow execution, retained detail and admission/failure regressions.
// POS: [L4 extension] Real custom business state is consumed without native storage, Guard, cwd or Plugin hooks.
package planning_test

import (
	"context"
	"encoding/json"
	"errors"
	"testing"

	"github.com/QuincySx/Alva/extension/planning"
	agentabi "github.com/QuincySx/Alva/sdk/agent/abi"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

// This host supplies only the business operations exercised below. Embedding
// Service makes any unexpected operation fail visibly instead of faking success.
type workflow struct {
	planning.Service
	created []planning.TaskRecord
}

func (s *workflow) Create(ctx context.Context, request planning.CreateRequest) (planning.TaskRecord, error) {
	if err := ctx.Err(); err != nil {
		return planning.TaskRecord{}, err
	}
	task, err := agentabi.NewTask(agentabi.TaskLocalWorkflow, request.Title, "", "")
	if err != nil {
		return planning.TaskRecord{}, err
	}
	record := planning.TaskRecord{Task: task, Description: request.Description}
	s.created = append(s.created, record)
	return record, nil
}

func (s *workflow) Get(ctx context.Context, id string) (planning.TaskRecord, bool, error) {
	if err := ctx.Err(); err != nil {
		return planning.TaskRecord{}, false, err
	}
	for _, record := range s.created {
		if record.Task.ID == id {
			return record, true, nil
		}
	}
	return planning.TaskRecord{}, false, nil
}

type modelFunc func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error

func (f modelFunc) Stream(ctx context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	return f(ctx, request, emit)
}

func TestWorkflowToolsRunInStandaloneMemorySDK(t *testing.T) {
	backend := &workflow{}
	refs, err := agentcontext.NewMemoryReferences("portable-plan", 1<<20)
	if err != nil {
		t.Fatal(err)
	}
	tools, err := planning.NewTools(backend, func(ctx context.Context, record planning.TaskRecord) (wire.Reference, error) {
		raw, err := json.Marshal(record)
		if err != nil {
			return wire.Reference{}, err
		}
		return refs.StoreText(ctx, string(raw))
	})
	if err != nil {
		t.Fatal(err)
	}
	turns := 0
	model := modelFunc(func(ctx context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		turns++
		if turns == 1 {
			if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "create", Name: "task_create", Arguments: json.RawMessage(`{"subject":"portable workflow","description":"retained source"}`)}}); err != nil {
				return err
			}
		} else {
			if len(backend.created) != 1 {
				t.Fatal("custom backend did not receive the actual create")
			}
			found := false
			for _, message := range request.Messages {
				for _, block := range message.Content {
					if block.ToolResult == nil || block.ToolResult.CallID != map[int]string{2: "create", 3: "get"}[turns] {
						continue
					}
					var receipt struct{ Detail wire.Reference }
					if block.ToolResult.IsError || json.Unmarshal([]byte(block.ToolResult.Content[0].Text), &receipt) != nil || receipt.Detail.Validate() != nil {
						t.Fatal("actual task did not produce a readable reference", block.ToolResult)
					}
					page, err := refs.ReadText(ctx, receipt.Detail.URI, 0, 4096)
					var retained planning.TaskRecord
					if err != nil || json.Unmarshal([]byte(page.Text), &retained) != nil || retained.Description != "retained source" || retained.Task.ID != backend.created[0].Task.ID {
						t.Fatal("reference lost business state", page, err)
					}
					found = true
				}
			}
			if !found {
				t.Fatal("no actual tool result in the next request")
			}
			if turns == 2 {
				raw, _ := json.Marshal(map[string]string{"task_id": backend.created[0].Task.ID})
				if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "get", Name: "task_get", Arguments: raw}}); err != nil {
					return err
				}
			}
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	builder := core.NewBuilder(model, core.NewMemorySession("portable-plan"))
	for _, tool := range tools {
		builder.Tool(tool)
	}
	agent, err := builder.Build(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	if len(agent.Snapshot().Plugins) != 0 {
		t.Fatal("ordinary tools secretly required a Plugin")
	}
	if err := agent.Run(t.Context(), nil, func(abi.Event) error { return nil }); err != nil || turns != 3 {
		t.Fatal("standalone workflow failed", turns, err)
	}
}

func TestWorkflowRejectsAmbiguityAndPreservesDetailFailure(t *testing.T) {
	backend := &workflow{}
	retainErr := errors.New("reference store unavailable")
	tools, err := planning.NewTools(backend, func(context.Context, planning.TaskRecord) (wire.Reference, error) { return wire.Reference{}, retainErr })
	if err != nil {
		t.Fatal(err)
	}
	tool := tools[0]
	execution := abi.ToolExecutionContext{ActualTool: tool, CallID: "create"}
	for _, raw := range []string{`{"subject":"a","subject":"b"}`, `{"subject":"\ud800"}`, `{"Subject":"x"}`, `{"subject":"x","metadata":{"k":1,"k":2}}`} {
		if _, err := tool.Execute(t.Context(), execution, wire.ToolCall{ID: "create", Name: tool.Name(), Arguments: json.RawMessage(raw)}); err == nil {
			t.Fatal("ambiguous arguments reached business state", raw)
		}
	}
	if len(backend.created) != 0 {
		t.Fatal("rejected arguments mutated state")
	}
	result, err := tool.Execute(t.Context(), execution, wire.ToolCall{ID: "create", Name: tool.Name(), Arguments: json.RawMessage(`{"subject":"committed business operation"}`)})
	if !errors.Is(err, retainErr) || len(result.Content) != 0 || len(backend.created) != 1 {
		t.Fatal("failed evidence claimed success or replayed the business operation", result, err)
	}
	if _, err := planning.NewTools((*workflow)(nil), func(context.Context, planning.TaskRecord) (wire.Reference, error) { return wire.Reference{}, nil }); err == nil {
		t.Fatal("nil backend admitted")
	}
}
