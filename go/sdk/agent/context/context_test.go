// INPUT: context, encoding/json, errors, reflect, strings, testing, agent/core, kernel/abi, llm/wire
// OUTPUT: Actual context lifecycle/compression/reference retrieval regression tests
// POS: [L3 capability library] Proves context policy changes provider working sets without rewriting committed facts.
package agentcontext

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
	"reflect"
	"strings"
	"testing"
)

type modelFunc func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error

func (fn modelFunc) Stream(ctx context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	return fn(ctx, request, emit)
}

type lifecycleHooks struct {
	events *[]string
	skip   bool
}

func (lifecycleHooks) Name() string { return "lifecycle" }
func (h lifecycleHooks) Bootstrap(context.Context, abi.ContextHandle, string) error {
	*h.events = append(*h.events, "bootstrap")
	return nil
}
func (h lifecycleHooks) Ingest(_ context.Context, _ abi.ContextHandle, _ string, entry abi.ContextEntry) (abi.IngestAction, error) {
	*h.events = append(*h.events, "ingest:"+string(entry.Message.Role))
	if entry.Message.Role == wire.RoleUser {
		if h.skip {
			return abi.IngestAction{Kind: abi.IngestSkip}, nil
		}
		message := wire.TextMessage(wire.RoleUser, "modified working text")
		return abi.IngestAction{Kind: abi.IngestModify, Message: &message}, nil
	}
	return abi.IngestAction{Kind: abi.IngestKeep}, nil
}
func (h lifecycleHooks) OnMessage(_ context.Context, _ abi.ContextHandle, _ string, message wire.Message) ([]abi.Injection, error) {
	*h.events = append(*h.events, "message:"+string(message.Role))
	if message.Role == wire.RoleUser {
		return []abi.Injection{{Kind: abi.InjectSystem, Section: &abi.PromptSection{ID: "policy", Content: "injected policy"}}}, nil
	}
	return nil, nil
}
func (h lifecycleHooks) Assemble(_ context.Context, _ abi.ContextHandle, _ string, entries []abi.ContextEntry, _ int) ([]abi.ContextEntry, error) {
	*h.events = append(*h.events, "assemble")
	return entries, nil
}
func (h lifecycleHooks) OnBudgetExceeded(context.Context, abi.ContextHandle, string, abi.ContextSnapshot) ([]abi.CompressAction, error) {
	*h.events = append(*h.events, "budget")
	return []abi.CompressAction{{Kind: abi.CompressWindow, KeepRecent: 1}}, nil
}
func (h lifecycleHooks) AfterTurn(context.Context, abi.ContextHandle, string) error {
	*h.events = append(*h.events, "after_turn")
	return nil
}
func (h lifecycleHooks) Dispose(context.Context) error {
	*h.events = append(*h.events, "dispose")
	return nil
}
func TestSevenContextHooksAffectWorkingRequestNotDurableFacts(t *testing.T) {
	var events []string
	store := NewStore(1, 100, nil, nil)
	session := core.NewMemorySession("context-lifecycle")
	model := modelFunc(func(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		if len(request.Messages) != 2 || !strings.Contains(request.Messages[0].Text(), "injected policy") || request.Messages[1].Text() != "modified working text" {
			t.Fatalf("context not applied: %#v", request.Messages)
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	agent, err := core.NewBuilder(model, session).ContextSystem(&abi.ContextSystem{Hooks: lifecycleHooks{events: &events}, Handle: store, Apply: DefaultApply{}, BudgetTokens: 1}).Build(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if err := agent.Run(context.Background(), []wire.Message{wire.TextMessage(wire.RoleUser, "original durable text")}, func(abi.Event) error { return nil }); err != nil {
		t.Fatal(err)
	}
	messages, _ := session.Messages(context.Background())
	if messages[0].Text() != "original durable text" {
		t.Fatal("context rewrote durable authority")
	}
	want := []string{"bootstrap", "ingest:user", "message:user", "assemble", "budget", "ingest:assistant", "message:assistant", "after_turn", "dispose"}
	if !reflect.DeepEqual(events, want) {
		t.Fatalf("events=%v", events)
	}
}
func TestContextIngestSkipReallyOmitsWorkingInput(t *testing.T) {
	var events []string
	session := core.NewMemorySession("skip")
	model := modelFunc(func(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		if len(request.Messages) != 0 {
			t.Fatal("skipped input still in model context")
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	agent, err := core.NewBuilder(model, session).ContextSystem(&abi.ContextSystem{Hooks: lifecycleHooks{events: &events, skip: true}, Handle: NewStore(100, 100, nil, nil), Apply: DefaultApply{}}).Build(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if err := agent.Run(context.Background(), []wire.Message{wire.TextMessage(wire.RoleUser, "durable but excluded")}, func(abi.Event) error { return nil }); err != nil {
		t.Fatal(err)
	}
	messages, _ := session.Messages(context.Background())
	if len(messages) != 2 || messages[0].Text() != "durable but excluded" {
		t.Fatal("skip destroyed historical facts")
	}
}
func TestWindowAndSummaryPreserveToolProtocolBoundaries(t *testing.T) {
	call := wire.ToolCall{ID: "c", Name: "tool", Arguments: json.RawMessage(`{}`)}
	result := wire.ToolResult{CallID: "c", Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: "result"}}}
	messages := []wire.Message{wire.TextMessage(wire.RoleUser, "old"), {Role: wire.RoleAssistant, Content: []wire.ContentBlock{{Kind: wire.ContentToolCall, ToolCall: &call}}}, {Role: wire.RoleTool, Content: []wire.ContentBlock{{Kind: wire.ContentToolResult, ToolResult: &result}}}, wire.TextMessage(wire.RoleUser, "new")}
	store := NewStore(100, 100, nil, nil)
	for i := range messages {
		messages[i].ID = string(rune('a' + i))
		if err := store.InjectMessage("s", abi.RuntimeInject, messages[i]); err != nil {
			t.Fatal(err)
		}
	}
	window := SafeWindow(messages, 2)
	if len(window) != 3 || window[0].Role != wire.RoleAssistant {
		t.Fatal("window orphaned result")
	}
	working := wire.CloneMessages(messages)
	action := abi.CompressAction{Kind: abi.CompressSummary, Range: abi.MessageRange{From: abi.MessageSelector{Kind: abi.ByIndex, Index: 1}, To: abi.MessageSelector{Kind: abi.ByIndex, Index: 2}}}
	if err := (DefaultApply{}).ApplyCompressions(context.Background(), []abi.CompressAction{action}, &working, store, "s"); err != nil {
		t.Fatal(err)
	}
	if len(working) != 3 || !strings.Contains(working[1].Text(), "conversation_summary") || len(store.Entries("s")) != 4 {
		t.Fatal("summary broken or durable/store history destroyed")
	}
}
func TestContextStoreCopyMetadataBudgetAndMemory(t *testing.T) {
	store := NewStore(100, 200, nil, nil)
	message := wire.TextMessage(wire.RoleUser, "hello")
	message.ID = "message"
	store.InjectMessage("agent", abi.AlwaysPresent, message)
	store.TagPriority("agent", "message", abi.Critical)
	entry, _ := store.ReadMessage("agent", "message")
	entry.Message.Content[0].Text = "mutated"
	if next, _ := store.ReadMessage("agent", "message"); next.Message.Text() != "hello" {
		t.Fatal("store ownership leaked")
	}
	snapshot := store.Snapshot("agent")
	if snapshot.TotalTokens <= 0 || snapshot.Layers[abi.AlwaysPresent].Entries != 1 || snapshot.Entries[0].Priority != abi.Critical {
		t.Fatal(snapshot)
	}
	store.StoreMemory(context.Background(), abi.MemoryFact{ID: "fact", Text: "prefers Go", Confidence: .9})
	facts, err := store.QueryMemory(context.Background(), "Go", 3)
	if err != nil || len(facts) != 1 || facts[0].Fingerprint == "" || facts[0].AccessCount != 1 {
		t.Fatal(facts, err)
	}
}
func TestDefaultBudgetDeciderHasDeterministicFallback(t *testing.T) {
	hooks := DefaultHooks{DecideBudget: func(context.Context, abi.ContextHandle, string, abi.ContextSnapshot) ([]abi.CompressAction, error) {
		return nil, errors.New("no LLM available")
	}}
	actions, err := hooks.OnBudgetExceeded(context.Background(), NewStore(1, 1, nil, nil), "s", abi.ContextSnapshot{Entries: []abi.EntrySnapshot{{Priority: abi.Disposable}}})
	if err != nil || len(actions) != 2 || actions[0].Kind != abi.CompressPriority || actions[1].KeepRecent != 30 {
		t.Fatal(actions, err)
	}
}
func TestVolatileReferenceUTF8OwnershipBudgetAndEncodedToolLimit(t *testing.T) {
	backend, err := NewMemoryReferences("session", 1<<20)
	if err != nil {
		t.Fatal(err)
	}
	text := strings.Repeat("\x00🙂日志\n", 500)
	reference, err := backend.StoreText(context.Background(), text)
	if err != nil {
		t.Fatal(err)
	}
	reader := NewReferenceReadTool(backend, "session")
	execution := abi.ToolExecutionContext{SessionID: "session"}
	offset := int64(0)
	var reconstructed strings.Builder
	for offset < int64(len(text)) {
		args, _ := json.Marshal(struct {
			URI    string `json:"uri"`
			Offset int64  `json:"offset"`
		}{reference.URI, offset})
		result, err := reader.Execute(context.Background(), execution, wire.ToolCall{ID: "read", Arguments: args})
		if err != nil {
			t.Fatal(err)
		}
		if len(result.Content[0].Text) > 4096 {
			t.Fatal("encoded page exceeded inline budget")
		}
		var page TextPage
		if err := json.Unmarshal([]byte(result.Content[0].Text), &page); err != nil {
			t.Fatal(err)
		}
		if page.NextOffset <= offset {
			t.Fatal("page made no progress")
		}
		reconstructed.WriteString(page.Text)
		offset = page.NextOffset
	}
	if reconstructed.String() != text {
		t.Fatal("UTF-8 paging lost bytes")
	}
	args, _ := json.Marshal(struct {
		URI string `json:"uri"`
	}{reference.URI})
	if _, err := reader.Execute(context.Background(), abi.ToolExecutionContext{SessionID: "foreign"}, wire.ToolCall{ID: "read", Arguments: args}); err == nil {
		t.Fatal("foreign execution read reference")
	}
	other, _ := NewMemoryReferences("other", 1024)
	if _, err := other.ReadText(context.Background(), reference.URI, 0, 20); err == nil {
		t.Fatal("hash granted cross-session access")
	}
	small, _ := NewMemoryReferences("small", 2)
	if _, err := small.StoreText(context.Background(), "large"); err == nil {
		t.Fatal("over-budget store succeeded")
	}
}
func TestReferenceSearchIsLiteralAndBounded(t *testing.T) {
	backend, _ := NewMemoryReferences("search", 1<<20)
	reference, _ := backend.StoreText(context.Background(), strings.Repeat("🙂literal.*pattern\n", 40))
	matches, err := backend.SearchText(context.Background(), reference.URI, "literal.*", 20)
	if err != nil || len(matches) != 20 || matches[0].Offset != 4 {
		t.Fatal(matches, err)
	}
	if _, err := backend.ReadText(context.Background(), reference.URI, 1, 4096); err == nil {
		t.Fatal("mid-rune offset accepted")
	}
	if _, err := backend.ReadText(context.Background(), reference.URI, 0, 1); err == nil {
		t.Fatal("zero-progress rune page accepted")
	}
}

type decoratedHandle struct{ abi.ContextHandle }

func TestRulesUseOnlyPublishedWorkingSetContract(t *testing.T) {
	store := NewStore(1000, 1000, nil, nil)
	message := wire.TextMessage(wire.RoleSystem, "constant policy")
	message.ID = "constant"
	if err := store.InjectMessage("session", abi.AlwaysPresent, message); err != nil {
		t.Fatal(err)
	}
	handle := decoratedHandle{ContextHandle: store}
	hooks := RulesHooks{}
	entries, err := hooks.Assemble(t.Context(), handle, "session", nil, 1000)
	if err != nil || len(entries) != 1 || entries[0].Message.Text() != "constant policy" {
		t.Fatal("decorator lost policy", entries, err)
	}
	if err := hooks.AfterTurn(t.Context(), handle, "session"); err != nil {
		t.Fatal(err)
	}
	if snapshot := handle.Snapshot("session"); len(snapshot.Entries) != 1 || snapshot.Entries[0].AgeTurns != 1 {
		t.Fatal("decorator lost lifecycle", snapshot)
	}
	old := wire.TextMessage(wire.RoleUser, "old working data")
	old.ID = "old"
	if err := store.InjectMessage("session", abi.RuntimeInject, old); err != nil {
		t.Fatal(err)
	}
	entries, err = hooks.Assemble(t.Context(), handle, "session", nil, 1000)
	if err != nil || len(entries) != 1 {
		t.Fatal("previous runtime history reentered bounded view", entries, err)
	}
}
