// INPUT: context, reflect, testing, agent/core, kernel/abi, llm/wire
// OUTPUT: Context hook composition and provider-bound priority ownership regressions
// POS: [L3 capability library] Proves independent context policies compose without discarding earlier decisions or mutating sibling inputs.
package agentcontext

import (
	"context"
	"reflect"
	"testing"

	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type compositionHooks struct {
	ingest func(abi.ContextEntry) abi.IngestAction
	budget func(abi.ContextSnapshot) []abi.CompressAction
}

func (compositionHooks) Name() string { return "composition-fixture" }
func (h compositionHooks) Ingest(_ context.Context, _ abi.ContextHandle, _ string, entry abi.ContextEntry) (abi.IngestAction, error) {
	if h.ingest != nil {
		return h.ingest(entry), nil
	}
	return abi.IngestAction{Kind: abi.IngestKeep}, nil
}
func (h compositionHooks) OnBudgetExceeded(_ context.Context, _ abi.ContextHandle, _ string, snapshot abi.ContextSnapshot) ([]abi.CompressAction, error) {
	if h.budget != nil {
		return h.budget(snapshot), nil
	}
	return nil, nil
}

func TestHooksChainCarriesOwnedPriorityAndMessageIdentity(t *testing.T) {
	priority := abi.Critical
	first := wire.TextMessage(wire.RoleUser, "first")
	second := wire.TextMessage(wire.RoleUser, "second")
	entry := abi.ContextEntry{ID: "actual", Message: wire.TextMessage(wire.RoleUser, "original"), Metadata: abi.ContextMetadata{Priority: abi.Normal}}
	entry.Message.ID = entry.ID
	chain := HooksChain{Hooks: []abi.ContextHooks{
		compositionHooks{ingest: func(abi.ContextEntry) abi.IngestAction {
			return abi.IngestAction{Kind: abi.IngestModify, Message: &first, Priority: &priority}
		}},
		compositionHooks{ingest: func(current abi.ContextEntry) abi.IngestAction {
			if current.Message.ID != entry.ID || current.Metadata.Priority != abi.Critical {
				t.Errorf("previous decision lost before next policy: %#v", current)
			}
			priority = abi.Disposable
			return abi.IngestAction{Kind: abi.IngestModify, Message: &second}
		}},
	}}
	action, err := chain.Ingest(context.Background(), nil, "session", entry)
	if err != nil || action.Priority == nil || *action.Priority != abi.Critical || action.Message == nil || action.Message.ID != entry.ID || action.Message.Text() != "second" {
		t.Fatalf("composed result lost identity/priority: %#v, %v", action, err)
	}
	second.Content[0].Text = "mutated after return"
	if action.Message.Text() != "second" {
		t.Fatal("result retains callback-owned message")
	}
}

func TestHooksChainCriticalDecisionSurvivesRealCompression(t *testing.T) {
	store := NewStore(1, 1000, nil, nil)
	mark := compositionHooks{ingest: func(entry abi.ContextEntry) abi.IngestAction {
		if entry.Message.Text() == "retain me" {
			priority := abi.Critical
			return abi.IngestAction{Kind: abi.IngestModify, Message: &entry.Message, Priority: &priority}
		}
		return abi.IngestAction{Kind: abi.IngestKeep}
	}}
	rewrite := compositionHooks{ingest: func(entry abi.ContextEntry) abi.IngestAction {
		return abi.IngestAction{Kind: abi.IngestModify, Message: &entry.Message}
	}}
	chain := HooksChain{Hooks: []abi.ContextHooks{mark, rewrite, RulesHooks{MaxMessages: 1}}}
	session := core.NewMemorySession("composition")
	model := modelFunc(func(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		if len(request.Messages) != 2 || request.Messages[0].Text() != "retain me" || request.Messages[1].Text() != "latest" {
			t.Fatalf("critical input discarded by later policy: %#v", request.Messages)
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	agent, err := core.NewBuilder(model, session).ContextSystem(&abi.ContextSystem{Hooks: chain, Handle: store, Apply: DefaultApply{}, BudgetTokens: 1}).Build(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	input := []wire.Message{wire.TextMessage(wire.RoleUser, "retain me"), wire.TextMessage(wire.RoleUser, "latest")}
	if err := agent.Run(context.Background(), input, func(abi.Event) error { return nil }); err != nil {
		t.Fatal(err)
	}
	facts, err := session.Messages(context.Background())
	if err != nil || len(facts) < 2 || facts[0].Text() != input[0].Text() || facts[1].Text() != input[1].Text() {
		t.Fatal("working policies changed durable inputs", facts, err)
	}
}

func TestHooksChainBudgetSnapshotIsIndependentPerPolicy(t *testing.T) {
	snapshot := abi.ContextSnapshot{Layers: map[abi.ContextLayer]abi.LayerStats{abi.RuntimeInject: {Tokens: 7}}, Entries: []abi.EntrySnapshot{{ID: "original", Priority: abi.Critical}}, ToolPatterns: []abi.ToolPattern{{ToolName: "original"}}}
	want := abi.ContextSnapshot{Layers: map[abi.ContextLayer]abi.LayerStats{abi.RuntimeInject: {Tokens: 7}}, Entries: []abi.EntrySnapshot{{ID: "original", Priority: abi.Critical}}, ToolPatterns: []abi.ToolPattern{{ToolName: "original"}}}
	chain := HooksChain{Hooks: []abi.ContextHooks{
		compositionHooks{budget: func(current abi.ContextSnapshot) []abi.CompressAction {
			current.Layers[abi.RuntimeInject] = abi.LayerStats{Tokens: 99}
			current.Entries[0].Priority = abi.Disposable
			current.ToolPatterns[0].ToolName = "changed"
			return nil
		}},
		compositionHooks{budget: func(current abi.ContextSnapshot) []abi.CompressAction {
			if !reflect.DeepEqual(current, want) {
				t.Error("earlier policy mutated sibling budget input")
			}
			return nil
		}},
	}}
	if _, err := chain.OnBudgetExceeded(context.Background(), nil, "session", snapshot); err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(snapshot, want) {
		t.Fatal("policy mutated caller snapshot")
	}
}
