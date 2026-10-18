// INPUT: context, encoding/json, errors, os/filepath, strings, testing, SDK/security, native Workspace and facts.
// OUTPUT: Actual notebook edits/original references/mode/lease/risk/identity and strict cell-format regression coverage.
// POS: [L4 extension] Temporary real files prove cell mutations and nonexecution without pretending OS-level hash-CAS.
package notebook

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/extension/builtin"
	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

const original = `{"nbformat":4,"nbformat_minor":5,"metadata":{"custom":"preserved"},"future_extension":{"untouched":true},"cells":[{"id":"code1","cell_type":"code","metadata":{"execution":{"old":"stale"},"custom":"kept"},"source":["old\n","no final newline"],"outputs":[{"output_type":"stream","name":"stdout","text":"old output"}],"execution_count":3},{"id":"md1","cell_type":"markdown","metadata":{},"source":"original markdown","attachments":{"a.png":{"image/png":"fixture-opaque-bytes"}}}]}`

type model struct{}

func (model) Stream(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error {
	return nil
}

type fixture struct {
	repo      *session.Repository
	log       *session.Session
	cwd, path string
	workspace *builtin.Workspace
	p         *Plugin
	tool      abi.Tool
}

func newFixture(t *testing.T) *fixture {
	t.Helper()
	ctx := context.Background()
	repo, err := session.Open(ctx, filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	log, err := repo.Create(ctx, "owner", "native", "")
	if err != nil {
		t.Fatal(err)
	}
	cwd := t.TempDir()
	path := filepath.Join(cwd, "notes.ipynb")
	if err = os.WriteFile(path, []byte(original), 0640); err != nil {
		t.Fatal(err)
	}
	os.Chmod(path, 0640)
	workspace, err := builtin.OpenWorkspace(cwd)
	if err != nil {
		t.Fatal(err)
	}
	guard, err := security.NewGuard(cwd, nil, workspace, security.NewPermissionManager(security.Ask, security.ApprovalFunc(func(context.Context, security.ApprovalRequest) (security.ApprovalDecision, error) {
		return security.ApprovalDecision{Allowed: true, Scope: security.Once}, nil
	})))
	if err != nil {
		t.Fatal(err)
	}
	locks, _ := core.NewToolLockRegistry(16)
	p, err := New(ctx, Config{Repository: repo, Owner: "owner", SessionID: "native", Cwd: cwd, Workspace: workspace, Guard: guard, Locks: locks, Report: func(context.Context, Activity) error { return nil }})
	if err != nil {
		t.Fatal(err)
	}
	state, _ := abi.NewAgentState(model{}, log, p.Tools())
	if err = p.OnRunStart(ctx, abi.RuntimeContext{RunID: "run", State: state, Workspace: cwd}); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		workspace.Close()
		if err := repo.Close(); err != nil {
			t.Error(err)
		}
	})
	return &fixture{repo: repo, log: log, cwd: cwd, path: path, workspace: workspace, p: p, tool: p.Tools()[0]}
}
func (f *fixture) call(args any) (wire.ToolResult, error) {
	raw, _ := json.Marshal(args)
	return f.tool.Execute(context.Background(), abi.ToolExecutionContext{ActualTool: f.tool, Session: f.log, SessionID: "native", RunID: "run", CallID: "call", Workspace: f.cwd}, wire.ToolCall{ID: "call", Name: "notebook_edit", Arguments: raw})
}
func TestActualNotebookEditFullOriginalRefsAndNoExecution(t *testing.T) {
	f := newFixture(t)
	source := "raise Exception('must not execute')\n🙂 no trailing newline"
	result, err := f.call(map[string]any{"notebook_path": "notes.ipynb", "cell_id": "code1", "edit_mode": "replace", "new_source": source})
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(result.Content[0].Text, "raise Exception") {
		t.Fatal("raw notebook source in model result")
	}
	data, err := os.ReadFile(f.path)
	if err != nil {
		t.Fatal(err)
	}
	var doc struct {
		Cells  []map[string]json.RawMessage
		Future json.RawMessage `json:"future_extension"`
	}
	if err = json.Unmarshal(data, &doc); err != nil {
		t.Fatal(err)
	}
	var actual string
	json.Unmarshal(doc.Cells[0]["source"], &actual)
	if actual != source {
		t.Fatal("source newline transformed")
	}
	if string(doc.Cells[0]["outputs"]) != "[]" || string(doc.Cells[0]["execution_count"]) != "null" || strings.Contains(string(doc.Cells[0]["metadata"]), "stale") {
		t.Fatal("stale code execution evidence retained")
	}
	if !strings.Contains(string(doc.Future), "untouched") || !strings.Contains(string(doc.Cells[1]["attachments"]), "fixture-opaque-bytes") {
		t.Fatal("unselected data lost")
	}
	info, _ := os.Stat(f.path)
	if info.Mode().Perm() != 0640 {
		t.Fatal("file mode changed")
	}
	var receipt struct {
		Original, Prepared wire.Reference
		Written, Executed  bool
	}
	if err = json.Unmarshal([]byte(result.Content[0].Text), &receipt); err != nil {
		t.Fatal(err)
	}
	if !receipt.Written || receipt.Executed {
		t.Fatal("edit falsely executed or not actually written")
	}
	_, backup, err := f.repo.ReadArtifactFull(context.Background(), "owner", "native", receipt.Original.URI, 20<<20)
	if err != nil || string(backup) != original {
		t.Fatal("original bytes not retained exactly", err)
	}
	_, prepared, err := f.repo.ReadArtifactFull(context.Background(), "owner", "native", receipt.Prepared.URI, 20<<20)
	if err != nil || string(prepared) != string(data) {
		t.Fatal("actual replacement differs from prepared evidence", err)
	}
}
func TestNotebookInsertDeleteAndProtectedCellActualSemantics(t *testing.T) {
	source := "new markdown"
	a := editArguments{ID: "inserted", Mode: "insert", Source: &source, CellType: "markdown", Before: "md1"}
	result, err := transform([]byte(original), a)
	if err != nil || result.Index != 1 {
		t.Fatal("insert position not honored", err)
	}
	result, err = transform(result.Data, editArguments{ID: "inserted", Mode: "delete"})
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(result.Data), "inserted") {
		t.Fatal("not actually deleted")
	}
	protected := strings.Replace(original, `"custom":"kept"`, `"custom":"kept","editable":false`, 1)
	if _, err = transform([]byte(protected), editArguments{ID: "code1", Mode: "replace", Source: &source}); err == nil {
		t.Fatal("protected cell edited")
	}
	duplicate := strings.Replace(original, `"id":"md1"`, `"id":"code1"`, 1)
	if _, err = transform([]byte(duplicate), editArguments{ID: "code1", Mode: "delete"}); err == nil {
		t.Fatal("duplicate cell ID accepted")
	}
	if _, err = transform([]byte(strings.Replace(original, `"nbformat_minor":5`, `"nbformat_minor":4`, 1)), a); err == nil {
		t.Fatal("silent format upgrade")
	}
}
func TestNotebookRiskFailureDoesNotWriteAndForeignToolRejected(t *testing.T) {
	f := newFixture(t)
	f.p.config.Report = func(context.Context, Activity) error { return errors.New("risk report unavailable") }
	if _, err := f.call(map[string]any{"notebook_path": "notes.ipynb", "cell_id": "code1", "edit_mode": "delete"}); err == nil {
		t.Fatal("risk failure ignored")
	}
	data, _ := os.ReadFile(f.path)
	if string(data) != original {
		t.Fatal("file write preceded durable risk")
	}
	forged := &notebookTool{plugin: f.p}
	raw := json.RawMessage(`{"notebook_path":"notes.ipynb","cell_id":"code1","edit_mode":"delete"}`)
	if _, err := f.tool.Execute(context.Background(), abi.ToolExecutionContext{ActualTool: forged, Session: f.log, SessionID: "native", RunID: "run", CallID: "call", Workspace: f.cwd}, wire.ToolCall{ID: "call", Name: "notebook_edit", Arguments: raw}); err == nil {
		t.Fatal("forged tool identity granted write")
	}
}

func TestMalformedNullSourceRejectsWithoutAnyFileMutation(t *testing.T) {
	for _, source := range []string{`null`, `["x",null]`} {
		t.Run(source, func(t *testing.T) {
			f := newFixture(t)
			var doc map[string]json.RawMessage
			json.Unmarshal([]byte(original), &doc)
			var cells []map[string]json.RawMessage
			json.Unmarshal(doc["cells"], &cells)
			cells[0]["source"] = json.RawMessage(source)
			doc["cells"] = encoded(cells)
			raw := encoded(doc)
			if err := os.WriteFile(f.path, raw, 0640); err != nil {
				t.Fatal(err)
			}
			if _, err := f.call(map[string]any{"notebook_path": "notes.ipynb", "cell_id": "md1", "edit_mode": "delete"}); err == nil {
				t.Fatal("malformed source entered actual edit")
			}
			actual, err := os.ReadFile(f.path)
			if err != nil || string(actual) != string(raw) {
				t.Fatal("invalid source notebook was mutated", err)
			}
		})
	}
}
