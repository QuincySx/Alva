// INPUT: context, encoding/json, errors, filepath, strings, testing, time, agent/security/core, extension/subagent/builtin, storage/session.
// OUTPUT: Real recursive native child records, exact grants, isolated references, HITL and setup-failure regressions.
// POS: [L5 harness] Local model fixtures execute actual SDK loops and SQLite, never paid providers or inherited history.
package harness

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/go/extension/builtin"
	"github.com/QuincySx/Alva/go/extension/subagent"
	"github.com/QuincySx/Alva/go/platform/storage/session"
	agentabi "github.com/QuincySx/Alva/go/sdk/agent/abi"
	"github.com/QuincySx/Alva/go/sdk/agent/context/scope"
	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/agent/security"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type childAllow struct{ requests []security.ApprovalRequest }

func (handler *childAllow) Approve(_ context.Context, request security.ApprovalRequest) (security.ApprovalDecision, error) {
	handler.requests = append(handler.requests, request)
	return security.ApprovalDecision{Allowed: true, Scope: security.Once}, nil
}

type childModelFunc func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error

func (fn childModelFunc) Stream(ctx context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	return fn(ctx, request, emit)
}
func childToolReply(emit func(wire.StreamChunk) error, id, name, args string) error {
	if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: id, Name: name, Arguments: json.RawMessage(args)}}); err != nil {
		return err
	}
	return emit(wire.StreamChunk{Kind: wire.ChunkFinish, FinishReason: "tool_calls"})
}
func childTextReply(emit func(wire.StreamChunk) error, text string) error {
	if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: text}); err != nil {
		return err
	}
	return emit(wire.StreamChunk{Kind: wire.ChunkFinish, FinishReason: "stop"})
}

func TestNativeSubagentRecursiveFactsReferencesAndDirectGrants(t *testing.T) {
	ctx := context.Background()
	cwd := t.TempDir()
	if err := os.WriteFile(filepath.Join(cwd, "source.txt"), []byte("workspace shared source🙂"), 0600); err != nil {
		t.Fatal(err)
	}
	repo, err := session.Open(ctx, filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	parent, err := repo.Create(ctx, "owner", "root", "")
	if err != nil {
		t.Fatal(err)
	}
	workspace, err := builtin.OpenWorkspace(cwd)
	if err != nil {
		t.Fatal(err)
	}
	defer workspace.Close()
	allow := &childAllow{}
	guard, err := security.NewGuard(cwd, nil, workspace, security.NewPermissionManager(security.Ask, allow))
	if err != nil {
		t.Fatal(err)
	}
	refs, _ := repo.BoundReferences(ctx, "owner", parent.ID())
	parentPrivate, _ := refs.StoreText(ctx, "root-only-secret")
	files := builtin.FileTools(workspace, guard, refs, parent.ID())
	locks, _ := core.NewToolLockRegistry(64)
	factory, err := NewNativeChildFactory(ctx, NativeChildConfig{Repository: repo, Owner: "owner", ParentID: parent.ID(), Cwd: cwd, Guard: guard, Bindings: NativeFileBindings(files, workspace), Locks: locks})
	if err != nil {
		t.Fatal(err)
	}
	model := childModelFunc(func(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		var user string
		var results []string
		for _, message := range request.Messages {
			if message.Role == wire.RoleUser {
				user = message.Text()
			}
			for _, block := range message.Content {
				if block.ToolResult != nil {
					results = append(results, block.ToolResult.CallID)
				}
			}
		}
		isChild := user == "child" || user == "grandchild"
		if isChild {
			data, _ := json.Marshal(request.Messages)
			if strings.Contains(string(data), "root-only-secret") || strings.Contains(string(data), parentPrivate.URI) {
				return errors.New("child inherited parent history")
			}
			for _, tool := range request.Tools {
				if tool.Name == "write_file" {
					return errors.New("child regained ungranted write capability")
				}
			}
		}
		if strings.HasPrefix(user, "root") {
			if len(results) == 0 {
				return childToolReply(emit, "spawn", "agent", `{"task":"child","tools":["read_file"]}`)
			}
			return childTextReply(emit, "root done")
		}
		if len(results) == 0 {
			return childToolReply(emit, "read", "read_file", `{"path":"source.txt"}`)
		}
		if user == "child" && len(results) == 1 {
			return childToolReply(emit, "spawn", "agent", `{"task":"grandchild","tools":["read_file"]}`)
		}
		return childTextReply(emit, user+" done🙂")
	})
	spawn, err := subagent.New(subagent.Config{SessionID: parent.ID(), Factory: factory, Guard: guard, References: refs, Timeout: 30 * time.Second, MaxIterations: 10, MaxDepth: 2})
	if err != nil {
		t.Fatal(err)
	}
	plugins, err := NativeSessionPlugins(ctx, repo, "owner", parent.ID())
	if err != nil {
		t.Fatal(err)
	}
	builder := NewBuilder(model, parent).Workspace(cwd).Plugin(spawn).Plugin(NewSecurityPlugin(guard, append(files, spawn.Tool()), plugins...)).Middleware(core.ToolLockMiddleware{Registry: locks})
	for _, plugin := range plugins {
		builder.Plugin(plugin)
	}
	for _, file := range files {
		builder.Tool(file)
	}
	agent, err := builder.Build(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if err = agent.Run(ctx, []wire.Message{wire.TextMessage(wire.RoleUser, "root root-only-secret")}, func(event abi.Event) error {
		if event.Record != nil && event.Record.SessionID != "root" {
			return errors.New("child event relabelled or leaked into root stream")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	list, err := repo.List(ctx, "owner", "", 10)
	if err != nil || len(list.Sessions) != 3 {
		t.Fatalf("actual recursive sessions: %+v %v", list, err)
	}
	var childID, grandchildID string
	for _, info := range list.Sessions {
		if info.ParentID == "root" {
			childID = info.ID
		}
	}
	for _, info := range list.Sessions {
		if info.ParentID == childID {
			grandchildID = info.ID
		}
	}
	if childID == "" || grandchildID == "" {
		t.Fatal("native direct ancestry lost")
	}
	child, _ := repo.OpenSession(ctx, "owner", childID)
	childRecords, err := child.Records(ctx, 0)
	if err != nil {
		t.Fatal(err)
	}
	var childRef wire.Reference
	var childSelected wire.Reference
	for _, record := range childRecords {
		if record.ToolOutcome == nil {
			continue
		}
		var payload struct {
			Reference wire.Reference `json:"reference"`
			Selected  wire.Reference `json:"selected_output"`
		}
		json.Unmarshal([]byte(record.ToolOutcome.Result.Content[0].Text), &payload)
		if record.ToolOutcome.CallID == "read" {
			if ref := record.ToolOutcome.Result.Content[0].Reference; ref != nil {
				childRef = *ref
			}
		}
		if record.ToolOutcome.CallID == "spawn" {
			childSelected = payload.Selected
		}
	}
	if childSelected.URI == "" {
		t.Fatal("grandchild selected output not retained in child")
	}
	// The root can inspect only the selected child output that was copied into
	// its own reference authority, never the child's original file/worker logs.
	if _, err = refs.ReadText(ctx, childSelected.URI, 0, 1024); !errors.Is(err, session.ErrNotFound) {
		t.Fatal("ancestor read child selected-output artifact", err)
	}
	if childRef.URI == "" {
		t.Fatal("child did not actually read and retain own file reference")
	}
	if _, err = refs.ReadText(ctx, childRef.URI, 0, 1024); !errors.Is(err, session.ErrNotFound) {
		t.Fatal("parent read child file artifact", err)
	}
	rootRecords, _ := parent.Records(ctx, 0)
	var rootSelected wire.Reference
	for _, record := range rootRecords {
		if record.ToolOutcome != nil && record.ToolOutcome.CallID == "spawn" {
			var payload struct {
				Selected wire.Reference `json:"selected_output"`
			}
			json.Unmarshal([]byte(record.ToolOutcome.Result.Content[0].Text), &payload)
			rootSelected = payload.Selected
		}
	}
	page, err := refs.ReadText(ctx, rootSelected.URI, 0, 2048)
	if err != nil || page.Text != "child done🙂" {
		t.Fatalf("selected child output incorrect: %+v %v", page, err)
	}
	if len(allow.requests) != 2 || allow.requests[0].SessionID != "root" || allow.requests[1].SessionID != childID {
		t.Fatalf("HITL reused ancestor approval or doubled invocation: %+v", allow.requests)
	}
}

func TestNativeChildFactoryRejectsUnboundActualToolBeforeCreatingSession(t *testing.T) {
	ctx := context.Background()
	cwd := t.TempDir()
	repo, err := session.Open(ctx, filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	parent, _ := repo.Create(ctx, "owner", "root", "")
	guard, _ := security.NewGuard(cwd, nil, nil, security.NewPermissionManager(security.Ask, nil))
	factory, err := NewNativeChildFactory(ctx, NativeChildConfig{Repository: repo, Owner: "owner", ParentID: "root", Cwd: cwd, Guard: guard})
	if err != nil {
		t.Fatal(err)
	}
	tool := echoTool{}
	model := demoModel{}
	root, _ := scope.NewRoot(model, []abi.Tool{tool}, scope.Config{SessionID: "root", Timeout: time.Second, MaxIterations: 3, MaxDepth: 2})
	child, _ := root.SpawnChild(ctx, agentabi.ChildScopeConfig{Role: "child"}, []string{"echo"})
	budget, _ := subagent.NewBudget(2, 1)
	_, err = factory.Prepare(ctx, subagent.ChildRequest{Parent: parent, ParentRunID: "run", SpawnCallID: "call", Workspace: cwd, Scope: child, Model: model, Grants: []abi.Tool{tool}, Budget: budget})
	if err == nil {
		t.Fatal("copied contributed tool without explicit binder")
	}
	list, _ := repo.List(ctx, "owner", "", 10)
	if len(list.Sessions) != 1 {
		t.Fatal("failed admission created child")
	}
	foreign := core.NewMemorySession("root")
	_, err = factory.Prepare(ctx, subagent.ChildRequest{Parent: foreign, ParentRunID: "run", SpawnCallID: "call", Workspace: cwd, Scope: child, Model: model, Budget: budget})
	if err == nil {
		t.Fatal("same-ID fake parent authority accepted")
	}
}

func TestNativeChildSetupFailureKeepsTruthfulDurableChildWithoutExecution(t *testing.T) {
	ctx := context.Background()
	cwd := t.TempDir()
	repo, err := session.Open(ctx, filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	parent, _ := repo.Create(ctx, "owner", "root", "")
	guard, _ := security.NewGuard(cwd, nil, nil, security.NewPermissionManager(security.Ask, nil))
	tool := echoTool{}
	failure := errors.New("explicit binder setup failed")
	closed := false
	factory, err := NewNativeChildFactory(ctx, NativeChildConfig{Repository: repo, Owner: "owner", ParentID: "root", Cwd: cwd, Guard: guard, Bindings: []ChildToolBinding{{Parent: tool, Bind: func(context.Context, ChildBinding) (BoundChildTool, error) {
		return BoundChildTool{Close: func(ctx context.Context) error { closed = true; return ctx.Err() }}, failure
	}}}})
	if err != nil {
		t.Fatal(err)
	}
	model := demoModel{}
	root, _ := scope.NewRoot(model, []abi.Tool{tool}, scope.Config{SessionID: "root", Timeout: time.Second, MaxIterations: 3, MaxDepth: 2})
	child, _ := root.SpawnChild(ctx, agentabi.ChildScopeConfig{Role: "child"}, []string{"echo"})
	budget, _ := subagent.NewBudget(2, 1)
	_, err = parent.AppendRecords(ctx, []abi.SessionRecord{abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordToolIntent, ToolIntent: &abi.ToolIntent{RunID: "run", Call: wire.ToolCall{ID: "call", Name: "agent", Arguments: json.RawMessage(`{"task":"independent","tools":["echo"]}`)}}})})
	if err != nil {
		t.Fatal(err)
	}
	_, err = factory.Prepare(ctx, subagent.ChildRequest{Parent: parent, ParentRunID: "run", SpawnCallID: "call", Workspace: cwd, Scope: child, Model: model, Grants: []abi.Tool{tool}, Budget: budget})
	if !errors.Is(err, failure) || !closed {
		t.Fatal("setup failure/cleanup lost", err)
	}
	actual, err := repo.OpenSession(ctx, "owner", child.SessionID())
	if err != nil {
		t.Fatal("durable child/link silently discarded", err)
	}
	records, err := actual.Records(ctx, 0)
	if err != nil || len(records) != 1 || records[0].Type != "child_lifecycle" || !strings.Contains(string(records[0].Payload), "prepare_failed") {
		t.Fatalf("incorrect child setup facts: %+v %v", records, err)
	}
	if messages, err := actual.Messages(ctx); err != nil || len(messages) != 0 {
		t.Fatal("failed setup executed model/user turn", messages, err)
	}
}
