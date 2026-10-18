// INPUT: context, errors, strings, sync/atomic, testing, agent/core, kernel/abi, llm/wire
// OUTPUT: Retrieval-only context extensibility and source failure probes
// POS: [architecture / executable probes] Replaces context selection through a Plugin without the four-layer Store or compression actions.
package extensibility_test

import (
	"context"
	"errors"
	"strings"
	"sync/atomic"
	"testing"

	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

// This fixture retrieves one explicitly indexed historical fact and the entire
// latest user turn. It is deliberately not a semantic search implementation or
// a bounded native read: ConversationReader.Messages may materialize history.
type indexedView struct {
	factID string
	err    error
}

type countedLog struct {
	*core.MemorySession
	reads atomic.Int32
}

func (log *countedLog) Messages(ctx context.Context) ([]wire.Message, error) {
	log.reads.Add(1)
	return log.MemorySession.Messages(ctx)
}

func (view indexedView) Load(ctx context.Context, reader abi.ConversationReader) ([]wire.Message, error) {
	if _, writable := reader.(abi.ConversationLog); writable {
		return nil, errors.New("source regained write authority")
	}
	if view.err != nil {
		return nil, view.err
	}
	messages, err := reader.Messages(ctx)
	if err != nil {
		return nil, err
	}
	latest := -1
	for i := range messages {
		if messages[i].Role == wire.RoleUser {
			latest = i
		}
	}
	if latest < 0 {
		return nil, errors.New("latest user turn missing")
	}
	var selected []wire.Message
	for i := 0; i < latest; i++ {
		if messages[i].ID == view.factID {
			selected = append(selected, messages[i])
		}
	}
	if len(selected) != 1 {
		return nil, errors.New("indexed fact missing")
	}
	return append(selected, messages[latest:]...), nil
}

func viewPlugin(view indexedView) plugin {
	return plugin{name: "indexed-context", register: func(_ context.Context, r *core.Registrar) error { return r.ContextSource(view) }}
}

func TestAlternateContextPreservesFactsAndWholeToolTurn(t *testing.T) {
	session := core.NewMemorySession("retrieval")
	var history []abi.SessionRecord
	for _, text := range []string{"project fact: cobalt", "unrelated old discussion", "another unrelated discussion"} {
		message := wire.TextMessage(wire.RoleUser, text)
		history = append(history, abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: &message}))
	}
	committed, err := session.AppendRecords(context.Background(), history)
	if err != nil {
		t.Fatal(err)
	}
	turns := 0
	model := modelFunc(func(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		turns++
		var transcript strings.Builder
		for _, message := range request.Messages {
			transcript.WriteString(message.Text())
		}
		if !strings.Contains(transcript.String(), "cobalt") || strings.Contains(transcript.String(), "unrelated") {
			return errors.New("alternative context selection was not applied")
		}
		if turns == 1 {
			return requestTool(emit, "lookup", "lookup-1")
		}
		messages := request.Messages
		if len(messages) != 4 || messages[len(messages)-1].Role != wire.RoleTool || messages[len(messages)-1].Content[0].ToolResult == nil || messages[len(messages)-1].Content[0].ToolResult.CallID != "lookup-1" {
			return errors.New("latest model/tool protocol group was split")
		}
		return finishText(emit, "answer from retrieved fact")
	})
	agent := build(t, core.NewBuilder(model, session).Plugin(viewPlugin(indexedView{factID: committed[0].ID})).Tool(tool{name: "lookup", readOnly: true, execute: func(_ context.Context, _ abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
		return toolResult(call, "retrieved tool fact"), nil
	}}))
	if err := agent.Run(context.Background(), []wire.Message{wire.TextMessage(wire.RoleUser, "answer using project fact")}, discard); err != nil {
		t.Fatal(err)
	}
	if turns != 2 {
		t.Fatal("expected the real model/tool/model loop", turns)
	}
	stored, err := session.Messages(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if len(stored) != 7 || stored[1].Text() != "unrelated old discussion" || stored[0].ID != committed[0].ID {
		t.Fatal("source altered original history", stored)
	}
}

func TestAlternateContextFailureDoesNotFallBackToHistory(t *testing.T) {
	sourceErr := errors.New("retrieval backend unavailable")
	called := false
	session := &countedLog{MemorySession: core.NewMemorySession("failed-retrieval")}
	agent := build(t, core.NewBuilder(modelFunc(func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error { called = true; return nil }), session).Plugin(viewPlugin(indexedView{err: sourceErr})))
	if err := agent.Run(context.Background(), []wire.Message{wire.TextMessage(wire.RoleUser, "query")}, discard); !errors.Is(err, sourceErr) {
		t.Fatal(err)
	}
	if called {
		t.Fatal("model called after source failure")
	}
	if session.reads.Load() != 0 {
		t.Fatal("source failure triggered a full-history read", session.reads.Load())
	}
}
