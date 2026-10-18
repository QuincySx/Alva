// INPUT: testing, isolated workspace, actual Guard and bounded reference backend
// OUTPUT: Reference-first file, backup, authorization, path and search regressions
// POS: [L4 extension] Native file tool acceptance without modifying user workspaces
package builtin

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	agentcontext "github.com/QuincySx/Alva/go/sdk/agent/context"
	"github.com/QuincySx/Alva/go/sdk/agent/security"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

func fileFixture(t *testing.T) (*Workspace, *security.Guard, agentcontext.ReferenceBackend, map[string]abi.Tool) {
	t.Helper()
	workspace, err := OpenWorkspace(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { workspace.Close() })
	guard, err := security.NewGuard(workspace.cwd, nil, workspace, security.NewPermissionManager(security.AcceptEdits, nil))
	if err != nil {
		t.Fatal(err)
	}
	backend, err := agentcontext.NewMemoryReferences("native", 8<<20)
	if err != nil {
		t.Fatal(err)
	}
	tools := make(map[string]abi.Tool)
	for _, tool := range FileTools(workspace, guard, backend, "native") {
		tools[tool.Name()] = tool
	}
	return workspace, guard, backend, tools
}
func executeFile(t *testing.T, w *Workspace, tools map[string]abi.Tool, name, args string) (wire.ToolResult, error) {
	t.Helper()
	return tools[name].Execute(context.Background(), abi.ToolExecutionContext{SessionID: "native", RunID: "run", CallID: "call", Workspace: w.cwd}, wire.ToolCall{ID: "call", Name: name, Arguments: json.RawMessage(args)})
}
func TestFileReadsUseReferencesAndEditsRetainBackup(t *testing.T) {
	w, _, backend, tools := fileFixture(t)
	original := strings.Repeat("old 中文\n", 1000)
	if err := os.WriteFile(filepath.Join(w.cwd, "log.txt"), []byte(original), 0600); err != nil {
		t.Fatal(err)
	}
	result, err := executeFile(t, w, tools, "read_file", `{"path":"log.txt"}`)
	if err != nil || result.IsError || len(result.Content) != 1 || result.Content[0].Reference == nil {
		t.Fatalf("read: %+v %v", result, err)
	}
	ref := result.Content[0].Reference
	page, err := backend.ReadText(context.Background(), ref.URI, 0, 128)
	if err != nil || !strings.HasPrefix(page.Text, "old 中文") {
		t.Fatalf("reference: %+v %v", page, err)
	}
	result, err = executeFile(t, w, tools, "edit_file", `{"path":"log.txt","old":"old","new":"new"}`)
	if err != nil || !result.IsError {
		t.Fatal("ambiguous edit did not stay business failure")
	}
	unchanged, _ := os.ReadFile(filepath.Join(w.cwd, "log.txt"))
	if string(unchanged) != original {
		t.Fatal("ambiguous edit mutated file")
	}
	result, err = executeFile(t, w, tools, "edit_file", `{"path":"log.txt","old":"old","new":"new","all":true}`)
	if err != nil || result.IsError {
		t.Fatalf("edit: %+v %v", result, err)
	}
	var value struct {
		Backup *wire.Reference `json:"backup"`
	}
	if err = json.Unmarshal([]byte(result.Content[0].Text), &value); err != nil || value.Backup == nil {
		t.Fatal("missing backup")
	}
	backup, err := backend.ReadText(context.Background(), value.Backup.URI, 0, 128)
	if err != nil || !strings.HasPrefix(backup.Text, "old 中文") {
		t.Fatal("backup did not retain prior bytes")
	}
	changed, _ := os.ReadFile(filepath.Join(w.cwd, "log.txt"))
	if !strings.HasPrefix(string(changed), "new 中文") {
		t.Fatal("edit not applied")
	}
}
func TestFileSafetyAndActualSessionBinding(t *testing.T) {
	w, _, _, tools := fileFixture(t)
	outside := filepath.Join(t.TempDir(), "secret")
	os.WriteFile(outside, []byte("outside secret"), 0600)
	os.Symlink(outside, filepath.Join(w.cwd, "alias"))
	os.WriteFile(filepath.Join(w.cwd, ".env"), []byte("private"), 0600)
	for _, path := range []string{"../escape", ".env", "alias", outside} {
		data, _ := json.Marshal(map[string]string{"path": path})
		if _, err := executeFile(t, w, tools, "read_file", string(data)); err == nil {
			t.Errorf("unsafe read accepted: %q", path)
		}
	}
	_, err := tools["write_file"].Execute(context.Background(), abi.ToolExecutionContext{SessionID: "foreign", Workspace: w.cwd}, wire.ToolCall{ID: "call", Name: "write_file", Arguments: json.RawMessage(`{"path":"x","content":"bad"}`)})
	if err == nil {
		t.Fatal("foreign session borrowed file tool")
	}
	for _, args := range []string{`{"path":"x"}`, `{"path":"x","content":"x","old":"not allowed"}`} {
		result, err := executeFile(t, w, tools, "write_file", args)
		if err != nil || !result.IsError {
			t.Fatal("invalid file args must be explicit business failure")
		}
	}
	os.WriteFile(filepath.Join(w.cwd, "binary"), []byte{255, 0}, 0600)
	result, err := executeFile(t, w, tools, "write_file", `{"path":"binary","content":"replace"}`)
	if err != nil || !result.IsError {
		t.Fatal("binary overwrite accepted without backup")
	}
}
func TestBoundedLiteralSearchDoesNotReadSensitiveText(t *testing.T) {
	w, _, _, tools := fileFixture(t)
	os.WriteFile(filepath.Join(w.cwd, "a.txt"), []byte("needle 中文 text"), 0600)
	os.WriteFile(filepath.Join(w.cwd, ".env"), []byte("needle private"), 0600)
	result, err := executeFile(t, w, tools, "search_files", `{"path":".","query":"needle"}`)
	if err != nil || result.IsError {
		t.Fatal(err)
	}
	if strings.Contains(result.Content[0].Text, "private") || !strings.Contains(result.Content[0].Text, "a.txt") {
		t.Fatalf("bad search: %s", result.Content[0].Text)
	}
}
func TestSecurityWrapperUnknownToolsRequireApproval(t *testing.T) {
	w, guard, _, tools := fileFixture(t)
	call := wire.ToolCall{ID: "call", Name: "write_file", Arguments: json.RawMessage(`{"path":"x","content":"new"}`)}
	execution := abi.ToolExecutionContext{SessionID: "native", RunID: "run", CallID: "call", Workspace: w.cwd}
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
	execution.ActualTool = tools["write_file"]
	if _, err := known.WrapTool(context.Background(), execution, call, next); err != nil || !called {
		t.Fatalf("explicit accept-edits failed: %v", err)
	}
}

func TestSameNameRuntimeReplacementDoesNotBorrowReadOnlyTrust(t *testing.T) {
	w, guard, _, tools := fileFixture(t)
	if err := os.WriteFile(filepath.Join(w.cwd, "x"), []byte("fixture"), 0600); err != nil {
		t.Fatal(err)
	}
	call := wire.ToolCall{ID: "call", Name: "read_file", Arguments: json.RawMessage(`{"path":"x"}`)}
	execution := abi.ToolExecutionContext{SessionID: "native", RunID: "run", CallID: "call", Workspace: w.cwd, ActualTool: tools["write_file"]}
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
func TestMakeDirectoryIsConfinedAndNonRecursive(t *testing.T) {
	w, guard, _, _ := fileFixture(t)
	if err := w.MakeDirectory(context.Background(), "memory", guard); err != nil {
		t.Fatal(err)
	}
	if err := w.MakeDirectory(context.Background(), "memory", guard); err != nil {
		t.Fatal(err)
	}
	if err := w.MakeDirectory(context.Background(), "missing/child", guard); err == nil {
		t.Fatal("unexpected recursive create")
	}
	if err := w.MakeDirectory(context.Background(), "../outside", guard); err == nil {
		t.Fatal("directory escaped workspace")
	}
	if err := os.Symlink(t.TempDir(), filepath.Join(w.cwd, "alias-dir")); err != nil {
		t.Fatal(err)
	}
	if err := w.MakeDirectory(context.Background(), "alias-dir/child", guard); err == nil {
		t.Fatal("symlink directory mutation accepted")
	}
}
