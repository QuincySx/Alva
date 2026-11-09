// INPUT: Actual native file tools, security Guard and explicit invocation identity
// OUTPUT: Untrusted-tool approval, unresolved-tool feedback without approval and same-name replacement trust regressions
// POS: [L4 extension] Exercises the unchanged security middleware with real filesystem Tool instances.
package securityplugin

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"testing"

	filetools "github.com/QuincySx/Alva/extension/files"
	workspacefs "github.com/QuincySx/Alva/platform/host/workspace"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func fileFixture(t *testing.T) (*workspacefs.Workspace, *security.Guard, agentcontext.ReferenceBackend, map[string]abi.Tool) {
	t.Helper()
	workspace, err := workspacefs.OpenWorkspace(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { workspace.Close() })
	guard, err := security.NewGuard(workspace.Directory(), nil, workspace, security.NewPermissionManager(security.AcceptEdits, nil))
	if err != nil {
		t.Fatal(err)
	}
	backend, err := agentcontext.NewMemoryReferences("native", 8<<20)
	if err != nil {
		t.Fatal(err)
	}
	tools := make(map[string]abi.Tool)
	for _, tool := range filetools.FileTools(workspace, guard, backend, "native") {
		tools[tool.Name()] = tool
	}
	return workspace, guard, backend, tools
}
func TestSecurityWrapperUnknownToolsRequireApproval(t *testing.T) {
	w, guard, _, tools := fileFixture(t)
	call := wire.ToolCall{ID: "call", Name: "write_file", Arguments: json.RawMessage(`{"path":"x","content":"new"}`)}
	execution := abi.ToolExecutionContext{SessionID: "native", RunID: "run", CallID: "call", Workspace: w.Directory(), ActualTool: tools["write_file"]}
	called := false
	next := func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
		called = true
		return wire.ToolResult{CallID: "call"}, nil
	}
	unknown := securityMiddleware{guard: guard, known: map[string]abi.Tool{}}
	if _, err := unknown.WrapTool(context.Background(), execution, call, next); !errors.Is(err, security.ErrApprovalRequired) || called {
		t.Fatalf("unknown granted: %v", err)
	}
	known := securityMiddleware{guard: guard, known: map[string]abi.Tool{"write_file": tools["write_file"]}}
	if _, err := known.WrapTool(context.Background(), execution, call, next); err != nil || !called {
		t.Fatalf("explicit accept-edits failed: %v", err)
	}
}

func TestSameNameRuntimeReplacementDoesNotBorrowReadOnlyTrust(t *testing.T) {
	w, guard, _, tools := fileFixture(t)
	if err := os.WriteFile(filepath.Join(w.Directory(), "x"), []byte("fixture"), 0600); err != nil {
		t.Fatal(err)
	}
	call := wire.ToolCall{ID: "call", Name: "read_file", Arguments: json.RawMessage(`{"path":"x"}`)}
	execution := abi.ToolExecutionContext{SessionID: "native", RunID: "run", CallID: "call", Workspace: w.Directory(), ActualTool: tools["write_file"]}
	known := securityMiddleware{guard: guard, known: map[string]abi.Tool{"read_file": tools["read_file"]}}
	called := false
	next := func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
		called = true
		return wire.ToolResult{CallID: call.ID}, nil
	}
	if _, err := known.WrapTool(context.Background(), execution, call, next); !errors.Is(err, security.ErrApprovalRequired) || called {
		t.Fatalf("replacement borrowed old trust: %v", err)
	}
	execution.ActualTool = tools["read_file"]
	if _, err := known.WrapTool(context.Background(), execution, call, next); err != nil || !called {
		t.Fatalf("actual trusted read failed: %v", err)
	}
}

// A call with no resolved implementation executes nothing: it gets the
// kernel's unknown-tool result, never reaches next and never asks a human.
func TestUnresolvedToolGetsUnknownResultWithoutApproval(t *testing.T) {
	w, _, _, tools := fileFixture(t)
	asked := false
	approval := security.ApprovalFunc(func(context.Context, security.ApprovalRequest) (security.ApprovalDecision, error) {
		asked = true
		return security.ApprovalDecision{Allowed: true, Scope: security.Once}, nil
	})
	guard, err := security.NewGuard(w.Directory(), nil, w, security.NewPermissionManager(security.Ask, approval))
	if err != nil {
		t.Fatal(err)
	}
	call := wire.ToolCall{ID: "call", Name: "ghost_tool", Arguments: json.RawMessage(`{}`)}
	called := false
	next := func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
		called = true
		return wire.ToolResult{CallID: "call"}, nil
	}
	middleware := securityMiddleware{guard: guard, known: map[string]abi.Tool{"write_file": tools["write_file"]}}
	result, err := middleware.WrapTool(context.Background(), abi.ToolExecutionContext{SessionID: "native", RunID: "run", CallID: "call", Workspace: w.Directory()}, call, next)
	if err != nil || called || asked {
		t.Fatalf("unresolved tool: err=%v next=%v asked=%v", err, called, asked)
	}
	if !result.IsError || result.CallID != "call" || len(result.Content) != 1 || result.Content[0].Text != `unknown tool "ghost_tool"` {
		t.Fatalf("unresolved tool result %+v", result)
	}
}

// A declined approval, plan mode and an out-of-workspace path are decisions the
// model can act on; cancellation joined with a denial is still an abort. A fresh
// call after a one-time decline asks again.
func TestDenialsReachTheModelAsRecoverableVetoes(t *testing.T) {
	w, _, _, tools := fileFixture(t)
	asks := 0
	decline := security.ApprovalFunc(func(context.Context, security.ApprovalRequest) (security.ApprovalDecision, error) {
		asks++
		return security.ApprovalDecision{Allowed: false, Scope: security.Once}, nil
	})
	manager := security.NewPermissionManager(security.Ask, decline)
	guard, err := security.NewGuard(w.Directory(), nil, w, manager)
	if err != nil {
		t.Fatal(err)
	}
	middleware := securityMiddleware{guard: guard, known: map[string]abi.Tool{"write_file": tools["write_file"]}}
	next := func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
		t.Fatal("denied call executed")
		return wire.ToolResult{}, nil
	}
	run := func(ctx context.Context, id, args string) error {
		call := wire.ToolCall{ID: id, Name: "write_file", Arguments: json.RawMessage(args)}
		_, err := middleware.WrapTool(ctx, abi.ToolExecutionContext{SessionID: "native", RunID: "run", CallID: id, Workspace: w.Directory(), ActualTool: tools["write_file"]}, call, next)
		return err
	}
	blocked := func(err error, reason string) {
		t.Helper()
		var veto *abi.ToolBlockedError
		if !errors.As(err, &veto) || veto.Reason != reason || !errors.Is(err, security.ErrPermissionDenied) {
			t.Fatalf("want veto %q, got %v", reason, err)
		}
	}
	blocked(run(context.Background(), "c1", `{"path":"x","content":"a"}`), security.ReasonUserDeclined)
	blocked(run(context.Background(), "c2", `{"path":"x","content":"a"}`), security.ReasonUserDeclined)
	if asks != 2 {
		t.Fatalf("one-time decline must not carry over: asked %d times", asks)
	}
	blocked(run(context.Background(), "c3", `{"path":"../outside","content":"a"}`), security.ReasonPathDenied)
	canceled, cancel := context.WithCancel(context.Background())
	cancel()
	if err := run(canceled, "c4", `{"path":"x","content":"a"}`); err == nil || errors.As(err, new(*abi.ToolBlockedError)) {
		t.Fatalf("cancellation must stay an abort: %v", err)
	}
	if err := manager.SetMode(security.Plan); err != nil {
		t.Fatal(err)
	}
	blocked(run(context.Background(), "c5", `{"path":"x","content":"a"}`), security.ReasonPlanMode)
}

type billableTool struct{ ran *int }

func (billableTool) Name() string                      { return "spend" }
func (billableTool) Description() string               { return "metered action" }
func (billableTool) ParametersSchema() json.RawMessage { return json.RawMessage(`{"type":"object"}`) }
func (t billableTool) Execute(_ context.Context, _ abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	*t.ran++
	return wire.ToolResult{CallID: call.ID}, nil
}
func (billableTool) Scheduling(wire.ToolCall) abi.Scheduling { return abi.Scheduling{} }
func (billableTool) DescribeEffect(wire.ToolCall) abi.Effect {
	return abi.Effect{Kind: abi.EffectUnknown, Description: "creates metered resources", Billable: true}
}
func (billableTool) IsReadOnly(wire.ToolCall) bool { return false }

// A billable call is put to the user even when the policy would not ask; a
// decline stops the tool, reaches the model as a recoverable veto, and the same
// call retried is asked again (the decline is not remembered).
func TestBillableDeclineIsVisibleToTheModelAndAskedAgain(t *testing.T) {
	w, _, _, _ := fileFixture(t)
	asks, ran := 0, 0
	handler := security.ApprovalFunc(func(_ context.Context, request security.ApprovalRequest) (security.ApprovalDecision, error) {
		asks++
		if !request.Effect.Billable {
			t.Error("the approval request lost the billable flag")
		}
		return security.ApprovalDecision{Allowed: false, Scope: security.Once}, nil
	})
	guard, err := security.NewGuard(w.Directory(), nil, w, security.NewPermissionManager(security.AcceptEdits, handler))
	if err != nil {
		t.Fatal(err)
	}
	tool := billableTool{ran: &ran}
	middleware := securityMiddleware{guard: guard, known: map[string]abi.Tool{"spend": tool}}
	next := func(ctx context.Context, e abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
		return tool.Execute(ctx, e, call)
	}
	for _, id := range []string{"c1", "c2"} {
		call := wire.ToolCall{ID: id, Name: "spend", Arguments: json.RawMessage(`{}`)}
		_, err := middleware.WrapTool(context.Background(), abi.ToolExecutionContext{SessionID: "native", RunID: "run", CallID: id, Workspace: w.Directory(), ActualTool: tool}, call, next)
		var veto *abi.ToolBlockedError
		if !errors.As(err, &veto) || veto.Reason != security.ReasonUserDeclined {
			t.Fatalf("%s: want recoverable decline, got %v", id, err)
		}
	}
	if asks != 2 || ran != 0 {
		t.Fatalf("asked %d times, tool ran %d times; want 2 and 0", asks, ran)
	}
}
