// INPUT: context, errors, testing, agent/core, kernel/abi, llm/wire
// OUTPUT: Committed-message authority/ownership/settlement regression tests
// POS: [L2.5 SDK assembly] Verifies observers receive only complete durable assistant facts rather than partial or guessed output.
package core_test

import (
	"context"
	"errors"
	"testing"

	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type committedProjection struct{}

func (committedProjection) Name() string { return "effective-answer" }
func (committedProjection) AfterModel(_ context.Context, _ abi.RuntimeContext, message *wire.Message) error {
	message.Content = []wire.ContentBlock{{Kind: wire.ContentReasoning, Reasoning: &wire.Reasoning{Text: "not answer", Signature: "signed"}}, {Kind: wire.ContentText, Text: "effective answer"}}
	return nil
}
func TestCommittedMessageEventUsesActualStoredEffectiveResponseAndOwnedCopies(t *testing.T) {
	session := core.NewMemorySession("committed")
	count := 0
	err := build(t, core.NewBuilder(textModel("stream answer"), session).Middleware(committedProjection{})).Run(context.Background(), nil, func(event abi.Event) error {
		if event.Kind != abi.EventMessageCommitted {
			return nil
		}
		count++
		if event.Message == nil || event.Record == nil || event.Message.Text() != "effective answer" || event.Message.ID != event.Record.RecordID || event.Record.SessionID != session.ID() {
			t.Fatal("not actual committed identity", event)
		}
		stored := records(t, session)
		found := false
		for _, record := range stored {
			if record.ID == event.Record.RecordID && record.Message != nil && record.Message.Text() == "effective answer" {
				found = true
			}
		}
		if !found {
			t.Fatal("event arrived before real commit")
		}
		copy := abi.CloneEvent(event)
		copy.Message.Content[0].Reasoning.Signature = "changed"
		copy.Record.RecordID = "other"
		if event.Message.Content[0].Reasoning.Signature != "signed" || event.Record.RecordID == "other" {
			t.Fatal("CloneEvent retained pointer aliases")
		}
		event.Message.Content[1].Text = "observer mutation"
		return nil
	})
	if err != nil || count != 1 {
		t.Fatal("committed event missing", err, count)
	}
	messages, _ := session.Messages(context.Background())
	if messages[0].Text() != "effective answer" {
		t.Fatal("observer changed durable fact")
	}
}
func TestFailedAppendAndPartialStreamNeverEmitCompleteMessage(t *testing.T) {
	for _, kind := range []string{"append", "partial"} {
		t.Run(kind, func(t *testing.T) {
			session := &failingSession{MemorySession: core.NewMemorySession("failed")}
			model := textModel("text")
			if kind == "append" {
				session.appendErrAt = 2
			} else {
				model = modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
					if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: "incomplete"}); err != nil {
						return err
					}
					return errors.New("stream failed")
				})
			}
			count := 0
			err := build(t, core.NewBuilder(model, session)).Run(context.Background(), nil, func(event abi.Event) error {
				if event.Kind == abi.EventMessageCommitted {
					count++
				}
				return nil
			})
			if err == nil || count != 0 {
				t.Fatal("incomplete/noncommitted response advertised", err, count)
			}
		})
	}
}
func TestCommittedMessageDeliveryFailureBackfillsAllUnexecutedTools(t *testing.T) {
	session := core.NewMemorySession("delivery")
	executed := 0
	delivery := errors.New("subscriber unavailable")
	model := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		if err := emit(callChunk("call")); err != nil {
			return err
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	tool := &testTool{execute: func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
		executed++
		return wire.ToolResult{}, nil
	}}
	err := build(t, core.NewBuilder(model, session).Tool(tool)).Run(context.Background(), nil, func(event abi.Event) error {
		if event.Kind == abi.EventMessageCommitted {
			return delivery
		}
		return nil
	})
	if !errors.Is(err, delivery) || executed != 0 {
		t.Fatal("delivery failure ran side effect", err, executed)
	}
	intents, outcomes := 0, 0
	for _, record := range records(t, session) {
		if record.ToolIntent != nil {
			intents++
		}
		if record.ToolOutcome != nil {
			outcomes++
		}
	}
	if intents != 1 || outcomes != 1 {
		t.Fatal("delivery failure left dangling tool", intents, outcomes)
	}
}
