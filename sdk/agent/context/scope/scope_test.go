// INPUT: context, encoding/json, errors, testing, time, agent/abi, agent/context, kernel/abi, kernel/bus, llm/wire
// OUTPUT: Real spawn tree, direct-parent grants, board isolation and communication lifecycle regressions
// POS: [L3 capability library] Exercises scoped capabilities without native sessions, providers or parent history inheritance.
package scope

import (
	"context"
	"encoding/json"
	"errors"
	agentabi "github.com/QuincySx/Alva/sdk/agent/abi"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/kernel/bus"
	"github.com/QuincySx/Alva/sdk/llm/wire"
	"testing"
	"time"
)

type fixtureModel struct{}

func (fixtureModel) Stream(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error {
	return nil
}

type namedTool string

func (tool namedTool) Name() string                 { return string(tool) }
func (namedTool) Description() string               { return "fixture" }
func (namedTool) ParametersSchema() json.RawMessage { return json.RawMessage(`{"type":"object"}`) }
func (namedTool) Execute(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
	return wire.ToolResult{}, nil
}
func (namedTool) Scheduling(wire.ToolCall) abi.Scheduling { return abi.Scheduling{} }
func (namedTool) DescribeEffect(wire.ToolCall) abi.Effect { return abi.Effect{Kind: abi.EffectRead} }
func (namedTool) IsReadOnly(wire.ToolCall) bool           { return true }
func limits() BoardLimits {
	return BoardLimits{MaxProfiles: 10, MaxMessages: 100, MaxBytes: 1 << 20, MaxMessageBytes: 4096, NotificationCapacity: 2}
}
func TestSpawnScopeDirectParentGrantsAndDepth(t *testing.T) {
	root, err := NewRoot(fixtureModel{}, []abi.Tool{namedTool("read"), namedTool("write"), namedTool("agent")}, Config{Timeout: time.Minute, MaxIterations: 10, MaxDepth: 2})
	if err != nil {
		t.Fatal(err)
	}
	child, err := root.SpawnChild(context.Background(), agentabi.ChildScopeConfig{Role: "child", MaxIterations: 5}, []string{"read", "agent", "missing"})
	if err != nil {
		t.Fatal(err)
	}
	grandchild, err := child.SpawnChild(context.Background(), agentabi.ChildScopeConfig{Role: "grandchild"}, []string{"read", "write", "agent"})
	if err != nil {
		t.Fatal(err)
	}
	if names := grandchild.ParentToolNames(); len(names) != 1 || names[0] != "read" {
		t.Fatal("ancestor tool authority leaked", names)
	}
	if child.MaxIterations() != 5 || grandchild.Timeout() != time.Minute || root.Snapshot().ChildrenCount != 1 || child.SessionID() == root.SessionID() {
		t.Fatal("scope identity/config incorrect")
	}
	if _, err := grandchild.SpawnChild(context.Background(), agentabi.ChildScopeConfig{Role: "too-deep"}, nil); err == nil {
		t.Fatal("depth bypass")
	}
	child.Complete()
	if _, err := child.SpawnChild(context.Background(), agentabi.ChildScopeConfig{Role: "late"}, nil); err == nil {
		t.Fatal("completed scope spawned")
	}
}
func TestBoardRegistryIsolationReadOnlyParentAndLag(t *testing.T) {
	registry, err := NewBoardRegistry(10, limits())
	if err != nil {
		t.Fatal(err)
	}
	first, _ := registry.GetOrCreate("parent", "team")
	same, _ := registry.GetOrCreate("parent", "team")
	other, _ := registry.GetOrCreate("unrelated", "team")
	if first != same || first == other {
		t.Fatal("scope board identity")
	}
	if err := registry.SetParent("child", "parent"); err != nil {
		t.Fatal(err)
	}
	if err := registry.SetParent("parent", "child"); err == nil {
		t.Fatal("cyclic scope link")
	}
	view, found := registry.ParentBoardFor("child", "team")
	if !found {
		t.Fatal("no upward board")
	}
	if _, writable := view.(interface{ Post(BoardMessage) error }); writable {
		t.Fatal("reader restored parent write authority")
	}
	subscription := view.Subscribe()
	defer subscription.Close()
	for i := 0; i < 3; i++ {
		message, _ := NewBoardMessage("parent", "message")
		message.Attachments = []json.RawMessage{json.RawMessage(`{"value":true}`)}
		if err := first.Post(message); err != nil {
			t.Fatal(err)
		}
		message.Attachments[0][0] = 'x'
	}
	if view.MessageCount() != 3 || other.MessageCount() != 0 {
		t.Fatal("board isolation")
	}
	if _, err := subscription.Recv(context.Background()); err == nil {
		t.Fatal("lag not reported")
	} else {
		var lag bus.LaggedError
		if !errors.As(err, &lag) || lag.Skipped != 1 {
			t.Fatal(err)
		}
	}
	messages := view.MessagesSince(0)
	messages[0].Attachments[0][0] = 'x'
	if view.MessagesSince(0)[0].Attachments[0][0] != '{' {
		t.Fatal("shared board bytes")
	}
}
func TestBoardRelevanceBudgetsAndRealCommunicationHooks(t *testing.T) {
	registry, _ := NewBoardRegistry(10, limits())
	board, _ := registry.GetOrCreate("parent", "team")
	_ = board.Register(AgentProfile{ID: "child", Role: "implementation", DependsOn: []string{"planner"}})
	for _, fixture := range []struct {
		from     string
		mentions []string
	}{{"planner", []string{"someone"}}, {"other", []string{"someone"}}, {"other", []string{"child"}}} {
		message, _ := NewBoardMessage(fixture.from, "text")
		message.Mentions = fixture.mentions
		if err := board.Post(message); err != nil {
			t.Fatal(err)
		}
	}
	if len(board.MessagesFor("child")) != 2 {
		t.Fatal("relevance routing")
	}
	communication := BlackboardCommunication{Registry: registry, Config: BlackboardHooksConfig{MaxMessagesInContext: 10, AutoPostOutput: true}}
	handle, err := communication.Attach(context.Background(), agentabi.SpawnCommContext{ParentScopeID: "parent", ParentSessionID: "ps", ChildScopeID: "child", ChildSessionID: "cs", Role: "developer"}, json.RawMessage(`{"board_id":"team"}`))
	if err != nil || len(handle.Hooks) != 1 || handle.OnComplete == nil {
		t.Fatal(handle, err)
	}
	store := agentcontext.NewStore(4096, 8192, nil, nil)
	hook := handle.Hooks[0].(*BlackboardHooks)
	if err := hook.Bootstrap(context.Background(), store, "cs"); err != nil {
		t.Fatal(err)
	}
	_, err = hook.OnMessage(context.Background(), store, "cs", wire.TextMessage(wire.RoleAssistant, "implemented"))
	if err != nil {
		t.Fatal(err)
	}
	entries, err := hook.Assemble(context.Background(), store, "cs", nil, 1000)
	if err != nil || len(entries) != 1 {
		t.Fatal(entries, err)
	}
	if err := hook.AfterTurn(context.Background(), store, "cs"); err != nil {
		t.Fatal(err)
	}
	if err := handle.OnComplete.Call(context.Background(), agentabi.SpawnResult{Text: "done"}); err != nil {
		t.Fatal(err)
	}
	if board.MessagesSince(board.MessageCount() - 1)[0].Kind != Artifact {
		t.Fatal("completion callback did not post")
	}
	commRegistry, _ := agentabi.NewCommunicationRegistry(1)
	if err := commRegistry.Register(communication); err != nil {
		t.Fatal(err)
	}
	if err := commRegistry.Register(communication); err == nil {
		t.Fatal("duplicate comm replaced")
	}
}
func TestSessionTrackerDoesNotClaimHistoryAuthority(t *testing.T) {
	tracker, _ := NewSessionTracker(3)
	if err := tracker.Register(SessionLink{SessionID: "root", ScopeID: "r"}); err != nil {
		t.Fatal(err)
	}
	if err := tracker.Register(SessionLink{SessionID: "child", ParentSessionID: "root", ScopeID: "c"}); err != nil {
		t.Fatal(err)
	}
	links, err := tracker.Ancestors("child")
	if err != nil || len(links) != 2 || links[1].SessionID != "root" {
		t.Fatal(links, err)
	}
	if err := tracker.Register(SessionLink{SessionID: "bad", ParentSessionID: "missing", ScopeID: "b"}); err == nil {
		t.Fatal("orphan accepted")
	}
}

func TestRootUsesActualSessionAndChildCannotExpandParentBudget(t *testing.T) {
	root, err := NewRoot(fixtureModel{}, nil, Config{SessionID: "actual-native-session", Timeout: time.Second, MaxIterations: 5, MaxDepth: 2})
	if err != nil || root.SessionID() != "actual-native-session" {
		t.Fatal(root, err)
	}
	for _, config := range []agentabi.ChildScopeConfig{{Role: "too-long", Timeout: 2 * time.Second}, {Role: "too-many", MaxIterations: 6}} {
		if _, err := root.SpawnChild(context.Background(), config, nil); err == nil {
			t.Fatal("child enlarged execution ceiling")
		}
	}
	child, err := root.SpawnChild(context.Background(), agentabi.ChildScopeConfig{Role: "child", Timeout: time.Millisecond, MaxIterations: 2}, nil)
	if err != nil || child.SessionID() == root.SessionID() || child.Timeout() != time.Millisecond || child.MaxIterations() != 2 {
		t.Fatal(child, err)
	}
}
