// INPUT: context, encoding/json, strings, testing, agent/core, kernel/bus, kernel/abi, llm/wire.
// OUTPUT: Default Plugin replacement, multi-turn session and reference-first regressions.
// POS: [L5 harness] In-memory harness behavior tests without external providers or tools.
package harness

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"

	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/agent/memory"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/kernel/bus"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type absentHarnessPlugin struct{ core.Plugin }
type absentHarnessLog struct{ abi.ConversationLog }

func TestHarnessRejectsTypedNilInputsBeforeDefaultAssembly(t *testing.T) {
	if _, err := NewBuilder(demoModel{}, (*absentHarnessLog)(nil)).Build(t.Context()); err == nil {
		t.Fatal("typed-nil session reached defaults")
	}
	if _, err := NewBuilder(demoModel{}, core.NewMemorySession("harness-nil")).Workspace(t.TempDir()).Plugin((*absentHarnessPlugin)(nil)).Build(t.Context()); err == nil {
		t.Fatal("typed-nil plugin reached name/default replacement")
	}
}

func TestHarnessDelegatesMiddlewareAssemblyOrder(t *testing.T) {
	var trace []string
	layer := func(name string) abi.Middleware {
		return core.BeforeModel(name, func(context.Context, abi.RuntimeContext, *abi.ModelRequest) error {
			trace = append(trace, name)
			return nil
		})
	}
	order := []int{1000}
	builder := NewDemoBuilder(core.NewMemorySession("harness-order")).Workspace(t.TempDir()).
		Middleware(layer("late"), 5000).Middleware(layer("early"), order...)
	order[0] = 6000
	agent, err := builder.Build(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	if err := agent.Run(t.Context(), []wire.Message{wire.TextMessage(wire.RoleUser, "ordered")}, func(abi.Event) error { return nil }); err != nil {
		t.Fatal(err)
	}
	if got := strings.Join(trace, ","); got != "early,late,early,late" {
		t.Fatal("harness dropped or retained mutable order arguments", got)
	}
}

func TestSameNamePluginReplacesOnlyItsDefault(t *testing.T) {
	custom := NewMemoryPlugin()
	agent, err := NewBuilder(demoModel{}, core.NewMemorySession("fixture")).Plugin(custom).Tool(echoTool{}).Build(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	service, ok := bus.Resolve[*memory.Service](agent.Bus())
	if !ok || service != custom.service {
		t.Fatal("custom memory did not replace default")
	}
	names := map[string]int{}
	for _, plugin := range agent.Snapshot().Plugins {
		names[plugin.Name]++
	}
	for _, name := range []string{"memory", "security", "system-context", "context-retrieval"} {
		if names[name] != 1 {
			t.Fatalf("default role duplicated/lost: %#v", names)
		}
	}
}

func TestDemoRetainsRealMultiTurnConversation(t *testing.T) {
	runner, err := NewDemo(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	for _, text := range []string{"first", "second"} {
		var output strings.Builder
		if err := runner.Run(context.Background(), []wire.Message{wire.TextMessage(wire.RoleUser, text)}, func(event abi.Event) error {
			if event.Kind == abi.EventTextDelta {
				output.WriteString(event.Text)
			}
			return nil
		}); err != nil {
			t.Fatal(err)
		}
		if !strings.Contains(output.String(), "Tool result: "+text) {
			t.Fatalf("tool roundtrip missing: %q", output.String())
		}
	}
	agent := runner.(*core.Agent)
	messages, err := agent.Session().Messages(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if len(messages) != 8 {
		t.Fatalf("conversation duplicated/lost: %d", len(messages))
	}
	if messages[0].Text() != "first" || messages[4].Text() != "second" {
		t.Fatal("turn ordering lost")
	}
	if messages[1].Content[0].ToolCall.ID == messages[5].Content[0].ToolCall.ID {
		t.Fatal("tool call IDs reused across turns")
	}
}

func TestReferenceProjectionPreservesRawFactsAndScopesReads(t *testing.T) {
	plugin := memoryContextPlugin(t, "owned-session")
	raw := strings.Repeat("日志🙂", 2000)
	message := wire.Message{Role: wire.RoleTool, Content: []wire.ContentBlock{{Kind: wire.ContentToolResult, ToolResult: &wire.ToolResult{CallID: "fixture", Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: raw}}}}}}
	request := abi.ModelRequest{Messages: wire.CloneMessages([]wire.Message{message})}
	if err := (referenceProjection{sessionID: plugin.sessionID, backend: plugin.backend}).BeforeModel(context.Background(), testRuntime(t, plugin.sessionID), &request); err != nil {
		t.Fatal(err)
	}
	projected := request.Messages[0].Content[0].ToolResult.Content[0]
	if projected.Kind != wire.ContentReference || projected.Reference == nil || projected.Reference.Bytes != int64(len(raw)) || !strings.Contains(strings.ToLower(projected.Reference.Processing), "volatile") {
		t.Fatalf("missing truthful reference: %#v", projected)
	}
	if message.Content[0].ToolResult.Content[0].Text != raw {
		t.Fatal("projection mutated raw facts")
	}
	arguments, _ := json.Marshal(map[string]any{"uri": projected.Reference.URI, "offset": 0})
	call := wire.ToolCall{ID: "read", Name: "context_read", Arguments: arguments}
	tool := agentcontext.NewReferenceReadTool(plugin.backend, plugin.sessionID)
	if _, err := tool.Execute(context.Background(), abi.ToolExecutionContext{SessionID: "foreign"}, call); err == nil {
		t.Fatal("foreign session read granted")
	}
	result, err := tool.Execute(context.Background(), abi.ToolExecutionContext{SessionID: "owned-session"}, call)
	if err != nil {
		t.Fatal(err)
	}
	var page struct {
		Text string `json:"text"`
		Next int    `json:"next_offset"`
	}
	if err := json.Unmarshal([]byte(result.Content[0].Text), &page); err != nil {
		t.Fatal(err)
	}
	if len(page.Text) > 2048 || page.Next != len(page.Text) || !strings.HasPrefix(raw, page.Text) {
		t.Fatalf("read is not bounded/correct: %+v", page)
	}
	call.Arguments, _ = json.Marshal(map[string]any{"uri": projected.Reference.URI, "offset": 1})
	if _, err := tool.Execute(context.Background(), abi.ToolExecutionContext{SessionID: "owned-session"}, call); err == nil {
		t.Fatal("mid-rune offset accepted")
	}
}

func TestReferenceBudgetFailsInsteadOfFeedingRawBody(t *testing.T) {
	store := memoryContextPlugin(t, "fixture").backend
	if _, err := store.StoreText(context.Background(), strings.Repeat("x", referenceStoreLimit+1)); err == nil {
		t.Fatal("oversized body accepted")
	}
	if _, err := store.StoreText(context.Background(), "small after overflow"); err != nil {
		t.Fatal("overflow poisoned backend", err)
	}
}

func TestReferenceThresholdAggregatesBlocksAndEscapedReadsStayInline(t *testing.T) {
	plugin := memoryContextPlugin(t, "fixture")
	content := []wire.ContentBlock{{Kind: wire.ContentText, Text: strings.Repeat("a", 3000)}, {Kind: wire.ContentText, Text: strings.Repeat("b", 3000)}}
	request := abi.ModelRequest{Messages: []wire.Message{{Role: wire.RoleTool, Content: []wire.ContentBlock{{Kind: wire.ContentToolResult, ToolResult: &wire.ToolResult{CallID: "call", Content: content}}}}}}
	if err := (referenceProjection{sessionID: plugin.sessionID, backend: plugin.backend}).BeforeModel(context.Background(), testRuntime(t, plugin.sessionID), &request); err != nil {
		t.Fatal(err)
	}
	for _, block := range request.Messages[0].Content[0].ToolResult.Content {
		if block.Kind != wire.ContentReference {
			t.Fatal("split blocks bypassed aggregate threshold")
		}
	}
	reference, err := plugin.backend.StoreText(context.Background(), strings.Repeat("\x01", 6000))
	if err != nil {
		t.Fatal(err)
	}
	arguments, _ := json.Marshal(map[string]any{"uri": reference.URI})
	result, err := (agentcontext.NewReferenceReadTool(plugin.backend, plugin.sessionID)).Execute(context.Background(), abi.ToolExecutionContext{SessionID: "fixture"}, wire.ToolCall{ID: "read", Arguments: arguments})
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Content[0].Text) > resultInlineLimit {
		t.Fatal("escaped page was recursively reprojectable")
	}
}

func TestSecurityDefaultRequiresExplicitApprovalForWrites(t *testing.T) {
	model := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "write", Name: "large_fixture", Arguments: []byte("{}")}}); err != nil {
			return err
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	agent, err := NewBuilder(model, core.NewMemorySession("approval")).Tool(largeFixtureTool{write: true}).Build(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if err = agent.Run(context.Background(), []wire.Message{wire.TextMessage(wire.RoleUser, "write")}, func(abi.Event) error { return nil }); !errors.Is(err, security.ErrApprovalRequired) {
		t.Fatalf("write did not fail closed: %v", err)
	}
}

type modelFunc func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error

func testRuntime(t *testing.T, sessionID string) abi.RuntimeContext {
	t.Helper()
	state, err := abi.NewAgentState(demoModel{}, core.NewMemorySession(sessionID), nil)
	if err != nil {
		t.Fatal(err)
	}
	return abi.RuntimeContext{State: state}
}

func TestReferenceProjectionRejectsCopiedParentPolicy(t *testing.T) {
	plugin := memoryContextPlugin(t, "parent")
	request := abi.ModelRequest{}
	if err := (referenceProjection{sessionID: "parent", backend: plugin.backend}).BeforeModel(context.Background(), testRuntime(t, "child"), &request); err == nil {
		t.Fatal("parent reference policy accepted foreign actual runtime")
	}
}

func (f modelFunc) Stream(ctx context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	return f(ctx, request, emit)
}

type largeFixtureTool struct {
	body  string
	write bool
}

func (largeFixtureTool) Name() string        { return "large_fixture" }
func (largeFixtureTool) Description() string { return "local reference projection fixture" }
func (largeFixtureTool) ParametersSchema() json.RawMessage {
	return json.RawMessage(`{"type":"object"}`)
}
func (t largeFixtureTool) Execute(_ context.Context, _ abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: t.body}}}, nil
}
func (largeFixtureTool) Scheduling(wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{ConcurrentSafe: true}
}
func (t largeFixtureTool) DescribeEffect(wire.ToolCall) abi.Effect {
	if t.write {
		return abi.Effect{Kind: abi.EffectWrite}
	}
	return abi.Effect{Kind: abi.EffectRead}
}
func (t largeFixtureTool) IsReadOnly(wire.ToolCall) bool { return !t.write }

func TestRealLoopProjectsLargeResultAndReadsOnlySelectedPage(t *testing.T) {
	body := strings.Repeat("private fixture log 日志🙂\n", 2000)
	session := core.NewMemorySession("owned")
	requests := 0
	model := modelFunc(func(ctx context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		requests++
		last := request.Messages[len(request.Messages)-1]
		if last.Role == wire.RoleUser {
			if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "large", Name: "large_fixture", Arguments: []byte(`{}`)}}); err != nil {
				return err
			}
		} else {
			result := last.Content[0].ToolResult
			if result.CallID == "large" {
				if len(result.Content) != 1 || result.Content[0].Kind != wire.ContentReference {
					return errors.New("raw large result entered model request")
				}
				arguments, _ := json.Marshal(map[string]any{"uri": result.Content[0].Reference.URI})
				if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "page", Name: "context_read", Arguments: arguments}}); err != nil {
					return err
				}
			} else {
				if len(result.Content[0].Text) >= len(body) || !strings.Contains(result.Content[0].Text, "private fixture log") {
					return errors.New("explicit page was not bounded/correct")
				}
				if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: "Read the selected log page only."}); err != nil {
					return err
				}
			}
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish, FinishReason: "stop"})
	})
	agent, err := NewBuilder(model, session).Tool(largeFixtureTool{body: body}).Build(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if err := agent.Run(context.Background(), []wire.Message{wire.TextMessage(wire.RoleUser, "inspect log")}, func(abi.Event) error { return nil }); err != nil {
		t.Fatal(err)
	}
	if requests != 3 {
		t.Fatalf("expected tool/reference/read model rounds: %d", requests)
	}
	records, err := session.Records(context.Background(), 0)
	if err != nil {
		t.Fatal(err)
	}
	found := false
	for _, record := range records {
		if record.ToolOutcome != nil && record.ToolOutcome.CallID == "large" {
			found = true
			if record.ToolOutcome.Result.Content[0].Text != body {
				t.Fatal("raw durable fact was replaced by context projection")
			}
		}
	}
	if !found {
		t.Fatal("tool fact missing")
	}
}

func memoryContextPlugin(t *testing.T, id string) *ContextRetrievalPlugin {
	t.Helper()
	backend, err := agentcontext.NewMemoryReferences(id, referenceStoreLimit)
	if err != nil {
		t.Fatal(err)
	}
	return NewContextRetrievalPlugin(id, backend)
}
