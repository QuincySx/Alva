// INPUT: context, encoding/json, errors, filepath, strings, testing, Task/core/security and native facts; portable capability contracts
// OUTPUT: Durable real Task/Plan recovery, DAG/transition/metadata/output/CAS and exact identity regressions.
// POS: [L4 extension] Tests prove actual bookkeeping and persistence without claiming execution or permission-mode changes.
package native

import (
	"context"
	"encoding/json"
	"errors"
	"path/filepath"
	"strings"
	"testing"

	planning "github.com/QuincySx/Alva/extension/planning"
	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/platform/storage/session"
	agentabi "github.com/QuincySx/Alva/sdk/agent/abi"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type modelFunc func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error

func (f modelFunc) Stream(c context.Context, r abi.ModelRequest, e func(wire.StreamChunk) error) error {
	return f(c, r, e)
}

type fixture struct {
	repo storage.Repository
	log  storage.Session
	c    Config
	p    *Plugin
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
	guard, err := security.NewGuard(cwd, nil, nil, security.NewPermissionManager(security.Ask, security.ApprovalFunc(func(context.Context, security.ApprovalRequest) (security.ApprovalDecision, error) {
		return security.ApprovalDecision{Allowed: true, Scope: security.Once}, nil
	})))
	if err != nil {
		t.Fatal(err)
	}
	c := Config{Repository: repo, Owner: "owner", SessionID: "native", Cwd: cwd, Guard: guard}
	p, err := New(ctx, c)
	if err != nil {
		t.Fatal(err)
	}
	f := &fixture{repo: repo, log: log, c: c, p: p}
	t.Cleanup(func() {
		if err := repo.Close(); err != nil {
			t.Error(err)
		}
	})
	return f
}
func TestRealDurableWorkflowDAGNotesAndColdRecovery(t *testing.T) {
	f := newFixture(t)
	s := f.p.Service()
	ctx := context.Background()
	a, err := s.Create(ctx, planning.CreateRequest{Title: "Actual A", Description: "full description", Metadata: json.RawMessage(`{"safe":"kept","api_key":"fixture-secret"}`)})
	if err != nil {
		t.Fatal(err)
	}
	b, err := s.Create(ctx, planning.CreateRequest{Title: "Actual B", BlockedBy: []string{a.Task.ID}})
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.Update(ctx, planning.UpdateRequest{ID: b.Task.ID, Status: agentabi.TaskRunning}); err == nil {
		t.Fatal("unsatisfied dependency started")
	}
	if _, err = s.Update(ctx, planning.UpdateRequest{ID: a.Task.ID, BlockedBy: []string{b.Task.ID}}); err == nil {
		t.Fatal("dependency cycle accepted")
	}
	if _, err = s.Update(ctx, planning.UpdateRequest{ID: a.Task.ID, Status: agentabi.TaskRunning}); err != nil {
		t.Fatal(err)
	}
	if _, err = s.Update(ctx, planning.UpdateRequest{ID: a.Task.ID, Status: agentabi.TaskSucceeded}); err != nil {
		t.Fatal(err)
	}
	output := "actual manual note🙂"
	if _, err = s.Update(ctx, planning.UpdateRequest{ID: b.Task.ID, Status: agentabi.TaskRunning, AppendOutput: &output}); err != nil {
		t.Fatal(err)
	}
	note, err := s.SetNotes(ctx, "real plan🙂\nNo workspace write")
	if err != nil {
		t.Fatal(err)
	}
	restored, err := New(ctx, f.c)
	if err != nil {
		t.Fatal(err)
	}
	rs := restored.Service()
	got, found, err := rs.Get(ctx, b.Task.ID)
	if err != nil || !found || got.Task.Status != agentabi.TaskRunning || len(got.Output) != 1 {
		t.Fatalf("not actual restored task: %+v %v", got, err)
	}
	if got.Task.Kind != agentabi.TaskLocalWorkflow || got.Task.OutputFile != "" {
		t.Fatal("fake execution/path")
	}
	_, bytes, err := f.repo.ReadArtifactFull(ctx, "owner", "native", got.Output[0].URI, 1<<20)
	if err != nil || string(bytes) != output {
		t.Fatalf("wrong source note: %s %v", bytes, err)
	}
	plan, err := rs.Notes(ctx)
	if err != nil || plan == nil || plan.URI != note.URI {
		t.Fatal("plan note not durable")
	}
	first, _, _ := rs.Get(ctx, a.Task.ID)
	if strings.Contains(string(first.Metadata), "fixture-secret") || !strings.Contains(string(first.Metadata), "kept") {
		t.Fatal("cache diverged from canonical redaction")
	}
	if _, err = rs.Update(ctx, planning.UpdateRequest{ID: a.Task.ID, Status: agentabi.TaskRunning}); err == nil {
		t.Fatal("terminal reopened")
	}
	if _, _, err = f.repo.LatestRecord(ctx, "foreign", "native", "alva.planning", "state_snapshot"); err == nil {
		t.Fatal("foreign metadata grant")
	}
}
func TestStalePlanningCacheCannotOverwriteNewerFacts(t *testing.T) {
	f := newFixture(t)
	other, err := New(context.Background(), f.c)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = f.p.Service().Create(context.Background(), planning.CreateRequest{Title: "committed"}); err != nil {
		t.Fatal(err)
	}
	if _, err = other.Service().Create(context.Background(), planning.CreateRequest{Title: "must not overwrite"}); !errors.Is(err, storage.ErrRevisionConflict) {
		t.Fatal(err)
	}
	fresh, err := New(context.Background(), f.c)
	if err != nil {
		t.Fatal(err)
	}
	page, err := fresh.Service().List(context.Background(), "", 0, 20)
	if err != nil || len(page.Tasks) != 1 || page.Tasks[0].Task.Title != "committed" {
		t.Fatalf("lost true state: %+v %v", page, err)
	}
}
func TestToolsActuallyConsumeServiceAndNoFakePermissionTools(t *testing.T) {
	f := newFixture(t)
	m := modelFunc(func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error { return nil })
	state, err := abi.NewAgentState(m, f.log, f.p.Tools())
	if err != nil {
		t.Fatal(err)
	}
	if err = f.p.OnRunStart(context.Background(), abi.RuntimeContext{RunID: "run", State: state, Workspace: f.c.Cwd}); err != nil {
		t.Fatal(err)
	}
	var create *planningTool
	for _, tool := range f.p.tools {
		if strings.Contains(tool.name, "plan_mode") {
			t.Fatal("registered fake permission tool")
		}
		if tool.name == "task_create" {
			create = tool
		}
	}
	e := abi.ToolExecutionContext{ActualTool: create, Session: f.log, SessionID: "native", RunID: "run", CallID: "call", Workspace: f.c.Cwd}
	call := wire.ToolCall{ID: "call", Name: create.name, Arguments: json.RawMessage(`{"subject":"real through tool","description":"stored, not merely echoed"}`)}
	result, err := create.Execute(context.Background(), e, call)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(result.Content[0].Text, "stored, not merely echoed") {
		t.Fatal("full task detail inline")
	}
	page, err := f.p.Service().List(context.Background(), "", 0, 20)
	if err != nil || len(page.Tasks) != 1 || page.Tasks[0].Description != "stored, not merely echoed" {
		t.Fatal("DTO without actual service write")
	}
	e.ActualTool = &planningTool{plugin: f.p, name: create.name}
	if _, err = create.Execute(context.Background(), e, call); err == nil {
		t.Fatal("forged tool granted write")
	}
	if _, err = create.Execute(context.Background(), abi.ToolExecutionContext{ActualTool: create, Session: f.log, SessionID: "native", RunID: "run", CallID: "bad", Workspace: f.c.Cwd}, wire.ToolCall{ID: "bad", Name: create.Name(), Arguments: json.RawMessage(`{"subject":"x","invented":true}`)}); err == nil {
		t.Fatal("ignored unsupported field")
	}
}
func TestMalformedDurablePlanDoesNotBecomeEmptySuccess(t *testing.T) {
	f := newFixture(t)
	if _, err := f.log.AppendExtensionSnapshot(context.Background(), "alva.planning", "state_snapshot", 0, json.RawMessage(`{"tasks":[{"task":{"kind":"local_bash","id":"b1"}}]}`)); err != nil {
		t.Fatal(err)
	}
	if _, err := New(context.Background(), f.c); err == nil {
		t.Fatal("bad stored state silently empty")
	}
}

func TestNativeBundleKeepsPlanReferenceInTaskList(t *testing.T) {
	f := newFixture(t)
	ref, err := f.p.Service().SetNotes(t.Context(), "native plan stays available")
	if err != nil {
		t.Fatal(err)
	}
	state, err := abi.NewAgentState(modelFunc(func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error { return nil }), f.log, f.p.Tools())
	if err != nil {
		t.Fatal(err)
	}
	if err := f.p.OnRunStart(t.Context(), abi.RuntimeContext{RunID: "run", State: state, Workspace: f.c.Cwd}); err != nil {
		t.Fatal(err)
	}
	var list abi.Tool
	for _, tool := range f.p.Tools() {
		if tool.Name() == "task_list" {
			list = tool
		}
	}
	if list == nil {
		t.Fatal("no task list")
	}
	result, err := list.Execute(t.Context(), abi.ToolExecutionContext{ActualTool: list, Session: f.log, SessionID: f.c.SessionID, RunID: "run", CallID: "list", Workspace: f.c.Cwd}, wire.ToolCall{ID: "list", Name: list.Name(), Arguments: json.RawMessage(`{}`)})
	var body struct {
		Notes *wire.Reference `json:"plan_notes"`
	}
	if err != nil || json.Unmarshal([]byte(result.Content[0].Text), &body) != nil || body.Notes == nil || body.Notes.URI != ref.URI || strings.Contains(result.Content[0].Text, "native plan stays available") {
		t.Fatal("native plan retrieval changed or leaked inline", result, err)
	}
}
