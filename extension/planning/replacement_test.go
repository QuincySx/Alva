// INPUT: context, encoding/json, errors, testing, Task/core, kernel/bus/wire and native session fixtures
// OUTPUT: Actual Service replacement, authority retention and default state isolation regressions
// POS: [L4 extension] A configured planning backend owns business state while the existing native log owns execution and reference evidence.
package planning

import (
	"context"
	"encoding/json"
	"errors"
	"testing"

	agentabi "github.com/QuincySx/Alva/sdk/agent/abi"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/kernel/bus"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type alternateService struct {
	Service
	created []TaskRecord
	bad     bool
}

func (s *alternateService) Create(ctx context.Context, request CreateRequest) (TaskRecord, error) {
	if err := ctx.Err(); err != nil {
		return TaskRecord{}, err
	}
	task, err := agentabi.NewTask(agentabi.TaskLocalWorkflow, request.Title, "", "")
	if err != nil {
		return TaskRecord{}, err
	}
	value := TaskRecord{Task: task, Description: request.Description}
	if s.bad {
		value.Metadata = json.RawMessage(`{"unfinished":`)
	}
	s.created = append(s.created, value)
	return value, nil
}

func TestPlanningActuallyConsumesConfiguredService(t *testing.T) {
	f := newFixture(t)
	external := &alternateService{Service: f.p.Service()}
	c := f.c
	c.Service = external
	p, err := New(t.Context(), c)
	if err != nil {
		t.Fatal(err)
	}
	turns := 0
	model := modelFunc(func(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		turns++
		if turns == 1 {
			if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "create", Name: "task_create", Arguments: json.RawMessage(`{"subject":"external business state","description":"retained detail"}`)}}); err != nil {
				return err
			}
		} else {
			if len(external.created) != 1 {
				t.Fatal("actual tool did not consume configured backend")
			}
			found := false
			for _, message := range request.Messages {
				for _, block := range message.Content {
					if block.ToolResult == nil || block.ToolResult.CallID != "create" {
						continue
					}
					var receipt struct{ Detail wire.Reference }
					if len(block.ToolResult.Content) != 1 || json.Unmarshal([]byte(block.ToolResult.Content[0].Text), &receipt) != nil || receipt.Detail.Validate() != nil {
						t.Fatal("custom task lost bounded native evidence", block.ToolResult)
					}
					_, raw, err := f.repo.ReadArtifactFull(t.Context(), f.c.Owner, f.c.SessionID, receipt.Detail.URI, 128<<10)
					var retained TaskRecord
					if err != nil || json.Unmarshal(raw, &retained) != nil || retained.Task.Title != "external business state" || retained.Description != "retained detail" {
						t.Fatal("source projection not retained in actual session", string(raw), err)
					}
					found = true
				}
			}
			if !found {
				t.Fatal("no actual tool result")
			}
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	agent, err := core.NewBuilder(model, f.log).Workspace(f.c.Cwd).Plugin(p).Build(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	provided, ok := bus.Resolve[Service](agent.Bus())
	if !ok || provided != external || p.Service() != external {
		t.Fatal("Tool, getter and bus did not share the accepted Service")
	}
	if err := agent.Run(t.Context(), nil, func(abi.Event) error { return nil }); err != nil || turns != 2 {
		t.Fatal("SDK did not execute alternate backend", turns, err)
	}
	if _, found, err := f.repo.LatestRecord(t.Context(), f.c.Owner, f.c.SessionID, "alva.planning", "state_snapshot"); err != nil || found {
		t.Fatal("configured backend secretly also wrote default planning state", found, err)
	}
}

func TestPlanningReplacementDoesNotReadDefaultSnapshot(t *testing.T) {
	f := newFixture(t)
	if _, err := f.log.AppendExtensionSnapshot(t.Context(), "alva.planning", "state_snapshot", 0, json.RawMessage(`{"tasks":null}`)); err != nil {
		t.Fatal(err)
	}
	if _, err := New(t.Context(), f.c); err == nil {
		t.Fatal("default loader accepted corrupt state")
	}
	c := f.c
	c.Service = &alternateService{}
	if _, err := New(t.Context(), c); err != nil {
		t.Fatal("explicit replacement still restored unrelated default state", err)
	}
	c.Service = (*alternateService)(nil)
	if _, err := New(t.Context(), c); err == nil {
		t.Fatal("typed-nil replacement admitted")
	}
}

func TestPlanningReplacementStillRequiresActualSession(t *testing.T) {
	f := newFixture(t)
	external := &alternateService{}
	c := f.c
	c.Service = external
	p, err := New(t.Context(), c)
	if err != nil {
		t.Fatal(err)
	}
	model := modelFunc(func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error {
		return errors.New("foreign authority reached model")
	})
	agent, err := core.NewBuilder(model, core.NewMemorySession(f.c.SessionID)).Workspace(f.c.Cwd).Plugin(p).Build(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	if err := agent.Run(t.Context(), nil, func(abi.Event) error { return nil }); err == nil || len(external.created) != 0 {
		t.Fatal("same ID borrowed actual native authority", err)
	}
}

func TestPlanningRejectsUnencodableServiceDetail(t *testing.T) {
	f := newFixture(t)
	service := &alternateService{bad: true}
	value, err := service.Create(t.Context(), CreateRequest{Title: "invalid backend projection"})
	if err != nil {
		t.Fatal(err)
	}
	if _, err := f.p.detail(t.Context(), value); err == nil {
		t.Fatal("invalid backend JSON became an empty successful detail artifact")
	}
}

func TestPlanningDefaultServiceRejectsCancelledReads(t *testing.T) {
	f := newFixture(t)
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	if _, err := f.p.Service().Notes(ctx); !errors.Is(err, context.Canceled) {
		t.Fatal("cancelled notes read succeeded", err)
	}
}
