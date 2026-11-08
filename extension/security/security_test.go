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
