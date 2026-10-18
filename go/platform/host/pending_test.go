// INPUT: context, strings, testing, app, kernel/abi, llm/wire.
// OUTPUT: Controller steering admission and real next-model injection regressions.
// POS: [platform / native execution host] Host lifecycle and optional app queue interact without observer-owned execution or prompt replay.
package host

import (
	"context"
	"encoding/json"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/go/harness"
	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type steeringModel struct {
	entered, release chan struct{}
	round            int
	t                *testing.T
}

func (m *steeringModel) Stream(ctx context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	m.round++
	if m.round == 1 {
		close(m.entered)
		select {
		case <-m.release:
		case <-ctx.Done():
			return ctx.Err()
		}
		if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "local", Name: "local", Arguments: []byte(`{}`)}}); err != nil {
			return err
		}
	} else {
		found := false
		for _, message := range request.Messages {
			found = found || message.Role == wire.RoleUser && message.Text() == "steer next boundary"
		}
		if !found {
			m.t.Error("queued user message absent at next real model boundary")
		}
		if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: "steering accepted by fixture"}); err != nil {
			return err
		}
	}
	return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
}

type steeringTool struct{}

func (steeringTool) Name() string                      { return "local" }
func (steeringTool) Description() string               { return "safe local fixture" }
func (steeringTool) ParametersSchema() json.RawMessage { return json.RawMessage(`{"type":"object"}`) }
func (steeringTool) Execute(_ context.Context, _ abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: "local complete"}}}, nil
}
func (steeringTool) Scheduling(wire.ToolCall) abi.Scheduling { return abi.Scheduling{} }
func (steeringTool) DescribeEffect(wire.ToolCall) abi.Effect { return abi.Effect{Kind: abi.EffectRead} }
func (steeringTool) IsReadOnly(wire.ToolCall) bool           { return true }

func TestControllerSteerCommitsExactlyOnceAtNextModelBoundary(t *testing.T) {
	model := &steeringModel{entered: make(chan struct{}), release: make(chan struct{}), t: t}
	log := core.NewMemorySession("actual")
	pending := harness.NewPendingPlugin("actual")
	agent, err := harness.NewBuilder(model, log).Plugin(pending).Tool(steeringTool{}).Build(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	controller := NewController(agent, 32)
	defer controller.Close(context.Background())
	id, err := controller.StartPrompt(context.Background(), "start")
	if err != nil {
		t.Fatal(err)
	}
	select {
	case <-model.entered:
	case <-time.After(time.Second):
		t.Fatal("model did not start")
	}
	if _, err = controller.Steer(context.Background(), "foreign-run", "bad"); err == nil {
		t.Fatal("foreign host run admitted")
	}
	if _, err = controller.Steer(context.Background(), id, "steer next boundary"); err != nil {
		t.Fatal(err)
	}
	close(model.release)
	deadline := time.Now().Add(3 * time.Second)
	for {
		snapshot, err := controller.Snapshot(id)
		if err != nil {
			t.Fatal(err)
		}
		if snapshot.Status != RunRunning {
			if snapshot.Status != RunCompleted || !strings.Contains(snapshot.Text, "steering accepted") {
				t.Fatalf("run failed: %+v", snapshot)
			}
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("run did not settle")
		}
		time.Sleep(time.Millisecond)
	}
	messages, err := log.Messages(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	count := 0
	for _, message := range messages {
		if message.Text() == "steer next boundary" {
			count++
		}
	}
	if count != 1 {
		t.Fatal("steering lost/replayed", count)
	}
	if _, err = controller.Steer(context.Background(), id, "too late"); err == nil {
		t.Fatal("completed root run admitted steering")
	}
}
