// INPUT: testing, owned temporary files, real SQLite memory/session stores, builtin filesystem, app recipe.
// OUTPUT: Actual memory tools, reference-only search, atomic sync, Markdown extraction and permission regressions.
// POS: [L4 extension] Full local memory business acceptance without external models, embeddings accounts or ambient scans.
package memoryplugin

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"

	"github.com/QuincySx/Alva/go/extension/builtin"
	"github.com/QuincySx/Alva/go/harness"
	memorysqlite "github.com/QuincySx/Alva/go/platform/storage/memory"
	"github.com/QuincySx/Alva/go/platform/storage/session"
	"github.com/QuincySx/Alva/go/sdk/agent/memory"
	"github.com/QuincySx/Alva/go/sdk/agent/security"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/kernel/bus"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type fixture struct {
	plugin    *Plugin
	backend   *memorysqlite.Backend
	repo      *session.Repository
	log       *session.Session
	cwd       string
	calls     int
	approvals atomic.Int32
}

func newFixture(t *testing.T, allow bool) *fixture {
	t.Helper()
	ctx := context.Background()
	private := filepath.Join(t.TempDir(), "private")
	repo, err := session.Open(ctx, private)
	if err != nil {
		t.Fatal(err)
	}
	log, err := repo.Create(ctx, "product", "actual", "")
	if err != nil {
		t.Fatal(err)
	}
	refs, err := repo.BoundReferences(ctx, "product", "actual")
	if err != nil {
		t.Fatal(err)
	}
	store, err := memorysqlite.Open(ctx, private)
	if err != nil {
		t.Fatal(err)
	}
	backend, err := store.Bind("workspace")
	if err != nil {
		t.Fatal(err)
	}
	cwd := t.TempDir()
	workspace, err := builtin.OpenWorkspace(cwd)
	if err != nil {
		t.Fatal(err)
	}
	fixture := &fixture{backend: backend, repo: repo, log: log, cwd: cwd}
	handler := security.ApprovalFunc(func(context.Context, security.ApprovalRequest) (security.ApprovalDecision, error) {
		fixture.approvals.Add(1)
		return security.ApprovalDecision{Allowed: allow, Scope: security.Once}, nil
	})
	guard, err := security.NewGuard(cwd, nil, workspace, security.NewPermissionManager(security.Ask, handler))
	if err != nil {
		t.Fatal(err)
	}
	plugin, err := New(Config{SessionID: "actual", Service: memory.NewService(backend, memory.NoopEmbeddingProvider{}), References: refs, Guard: guard, Workspace: workspace, Cwd: cwd})
	if err != nil {
		t.Fatal(err)
	}
	fixture.plugin = plugin
	t.Cleanup(func() {
		if err := workspace.Close(); err != nil {
			t.Error(err)
		}
		if err := store.Close(); err != nil {
			t.Error(err)
		}
		if err := repo.Close(); err != nil {
			t.Error(err)
		}
	})
	return fixture
}
func (f *fixture) invoke(ctx context.Context, name string, args any) (wire.ToolResult, error) {
	f.calls++
	id := fmt.Sprintf("call-%d", f.calls)
	data, err := json.Marshal(args)
	if err != nil {
		return wire.ToolResult{}, err
	}
	for _, tool := range f.plugin.Tools() {
		if tool.Name() == name {
			return tool.Execute(ctx, abi.ToolExecutionContext{SessionID: "actual", RunID: "run", CallID: id, Workspace: f.cwd, ActualTool: tool}, wire.ToolCall{ID: id, Name: name, Arguments: data})
		}
	}
	return wire.ToolResult{}, errors.New("tool unavailable")
}

func TestRebindOnlyOneExplicitMemoryCapabilityAndOwnReferences(t *testing.T) {
	f := newFixture(t, true)
	ctx := context.Background()
	if _, err := f.invoke(ctx, "memory_store", map[string]any{"key": "explicit-shared-knowledge", "content": "needle child selected🙂", "category": "project"}); err != nil {
		t.Fatal(err)
	}
	if _, err := f.repo.Create(ctx, "product", "child", "actual"); err != nil {
		t.Fatal(err)
	}
	refs, err := f.repo.BoundReferences(ctx, "product", "child")
	if err != nil {
		t.Fatal(err)
	}
	var parent abi.Tool
	for _, tool := range f.plugin.Tools() {
		if tool.Name() == "memory_search" {
			parent = tool
		}
	}
	child, err := f.plugin.RebindTool(parent, "child", refs)
	if err != nil {
		t.Fatal(err)
	}
	if child == parent || child.Name() != "memory_search" {
		t.Fatal("rebound entire tool catalog or reused parent object")
	}
	call := wire.ToolCall{ID: "child-search", Name: child.Name(), Arguments: json.RawMessage(`{"query":"needle"}`)}
	result, err := child.Execute(ctx, abi.ToolExecutionContext{SessionID: "child", RunID: "child-run", CallID: call.ID, Workspace: f.cwd, ActualTool: child}, call)
	if err != nil {
		t.Fatal(err)
	}
	var payload struct {
		Matches []struct {
			Source wire.Reference `json:"source"`
		} `json:"matches"`
	}
	if err = json.Unmarshal([]byte(result.Content[0].Text), &payload); err != nil || len(payload.Matches) != 1 {
		t.Fatal(result, err)
	}
	ref := payload.Matches[0].Source
	page, err := refs.ReadText(ctx, ref.URI, 0, 1024)
	if err != nil || !strings.Contains(page.Text, "child selected") {
		t.Fatal(page, err)
	}
	if _, err = f.repo.ReadArtifact(ctx, "product", "actual", ref.URI, 0, 1024); !errors.Is(err, session.ErrNotFound) {
		t.Fatal("parent received child search reference authority", err)
	}
	if _, err = child.Execute(ctx, abi.ToolExecutionContext{SessionID: "actual", RunID: "run", CallID: call.ID, ActualTool: child}, call); err == nil {
		t.Fatal("child memory capability authorizes parent")
	}
	if _, err = f.plugin.RebindTool(child, "grandchild", refs); err == nil {
		t.Fatal("foreign actual tool accepted by parent recipe")
	}
}
func TestMemoryToolsPersistSearchByReferenceAndDenyForeignScopes(t *testing.T) {
	f := newFixture(t, true)
	text := strings.Repeat("needle 日志🙂\n", 1000)
	result, err := f.invoke(context.Background(), "memory_store", map[string]any{"key": "knowledge", "content": text, "category": "project"})
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(result.Content[0].Text, text) {
		t.Fatal("stored body re-entered context")
	}
	result, err = f.invoke(context.Background(), "memory_search", map[string]any{"query": "needle", "limit": 5})
	if err != nil {
		t.Fatal(err)
	}
	var response struct {
		Matches []struct {
			Source wire.Reference `json:"source"`
		} `json:"matches"`
	}
	if err = json.Unmarshal([]byte(result.Content[0].Text), &response); err != nil || len(response.Matches) != 1 {
		t.Fatalf("actual FTS result missing: %+v %v", response, err)
	}
	if strings.Contains(result.Content[0].Text, "needle 日志") || len(result.Content[0].Text) > 4096 {
		t.Fatal("raw search body fed to model")
	}
	page, err := f.plugin.config.References.ReadText(context.Background(), response.Matches[0].Source.URI, 0, 2048)
	if err != nil || !strings.HasPrefix(text, page.Text) {
		t.Fatal("selected memory source unavailable", err)
	}
	before := f.approvals.Load()
	tool := f.plugin.Tools()[0]
	if _, err = tool.Execute(context.Background(), abi.ToolExecutionContext{SessionID: "child", CallID: "foreign"}, wire.ToolCall{ID: "foreign", Name: tool.Name(), Arguments: []byte(`{"query":"needle"}`)}); err == nil {
		t.Fatal("copied tool granted parent source to child")
	}
	if f.approvals.Load() != before {
		t.Fatal("foreign scope reached permission system")
	}
}
func TestMemorySyncChunksAndUnchangedHashAreRealAndBounded(t *testing.T) {
	f := newFixture(t, true)
	var body strings.Builder
	for index := 1; index <= 120; index++ {
		fmt.Fprintf(&body, "needle line %d\n", index)
	}
	if err := os.WriteFile(filepath.Join(f.cwd, "MEMORY.md"), []byte(body.String()), 0600); err != nil {
		t.Fatal(err)
	}
	for _, directory := range []string{".ignored", "node_modules", "target"} {
		if err := os.Mkdir(filepath.Join(f.cwd, directory), 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(f.cwd, directory, "MEMORY.md"), []byte("not authorized automatic content"), 0600); err != nil {
			t.Fatal(err)
		}
	}
	outside := t.TempDir()
	_ = os.WriteFile(filepath.Join(outside, "MEMORY.md"), []byte("foreign-marker"), 0600)
	if err := os.Symlink(outside, filepath.Join(f.cwd, "symlink")); err != nil {
		t.Fatal(err)
	}
	result, err := f.invoke(context.Background(), "memory_sync", map[string]any{})
	if err != nil {
		t.Fatal(err)
	}
	var report syncReport
	if err = json.Unmarshal([]byte(result.Content[0].Text), &report); err != nil || report.FilesAdded != 1 || report.ChunksCreated != 3 {
		t.Fatalf("scan not real: %+v %v", report, err)
	}
	entries, err := f.backend.FTSSearch(context.Background(), "needle", 10)
	if err != nil || len(entries) != 3 {
		t.Fatal("chunks not indexed", len(entries), err)
	}
	if entries[0].StartLine != 1 || entries[1].StartLine != 51 || entries[2].StartLine != 101 {
		t.Fatal("50-line chunk metadata lost", entries)
	}
	result, err = f.invoke(context.Background(), "memory_sync", map[string]any{})
	if err != nil {
		t.Fatal(err)
	}
	_ = json.Unmarshal([]byte(result.Content[0].Text), &report)
	if report.FilesUnchanged != 1 || report.ChunksCreated != 0 {
		t.Fatal("unchanged source reindexed", report)
	}
	foreign, err := f.backend.FTSSearch(context.Background(), "foreign", 10)
	if err != nil || len(foreign) != 0 {
		t.Fatal("symlink scanned outside workspace", err)
	}
}
func TestMemoryExtractionCreatesTypedMarkdownAndRetainsBackups(t *testing.T) {
	f := newFixture(t, true)
	directory := filepath.Join(f.cwd, ".alva-memory")
	if _, err := os.Stat(directory); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("constructor created/scanned memory files")
	}
	args := map[string]any{"name": "Build constraints", "description": "Safe notes", "type": "project", "content": "needle original structured note"}
	result, err := f.invoke(context.Background(), "memory_extract", args)
	if err != nil {
		t.Fatal(err)
	}
	var receipt struct {
		Path      string         `json:"path"`
		Source    wire.Reference `json:"source"`
		FileWrite struct {
			Backup *wire.Reference `json:"backup"`
		} `json:"file_write"`
		IndexWrite struct {
			Backup *wire.Reference `json:"backup"`
		} `json:"index_write"`
	}
	if err = json.Unmarshal([]byte(result.Content[0].Text), &receipt); err != nil {
		t.Fatal(err)
	}
	original, err := os.ReadFile(filepath.Join(f.cwd, receipt.Path))
	if err != nil || !strings.Contains(string(original), "name: Build constraints") || !strings.Contains(string(original), "needle original") {
		t.Fatal("typed Markdown not written", err)
	}
	index, err := os.ReadFile(filepath.Join(directory, "MEMORY.md"))
	if err != nil || !strings.Contains(string(index), "Safe notes") {
		t.Fatal("actual index missing", err)
	}
	args["content"] = "updated explicit memory"
	result, err = f.invoke(context.Background(), "memory_extract", args)
	if err != nil {
		t.Fatal(err)
	}
	if err = json.Unmarshal([]byte(result.Content[0].Text), &receipt); err != nil {
		t.Fatal(err)
	}
	if receipt.FileWrite.Backup == nil || receipt.IndexWrite.Backup == nil {
		t.Fatal("prior note/index not recoverable")
	}
	page, err := f.plugin.config.References.ReadText(context.Background(), receipt.FileWrite.Backup.URI, 0, 2048)
	if err != nil || !strings.Contains(page.Text, "needle original") {
		t.Fatal("prior bytes not retained", err)
	}
}
func TestMemoryReplacementRecipeAndPermissionDenial(t *testing.T) {
	f := newFixture(t, false)
	if _, err := f.invoke(context.Background(), "memory_store", map[string]any{"key": "denied", "content": "must not save"}); !errors.Is(err, security.ErrPermissionDenied) {
		t.Fatal("write did not require approval", err)
	}
	if _, found, err := f.backend.GetFile(context.Background(), "denied"); err != nil || found {
		t.Fatal("denied write changed memory", err)
	}
	native, err := harness.NativeSessionPlugins(context.Background(), f.repo, "product", f.log.ID())
	if err != nil {
		t.Fatal(err)
	}
	builder := harness.NewDemoBuilder(f.log).Workspace(f.cwd).Plugin(f.plugin)
	for _, plugin := range native {
		builder.Plugin(plugin)
	}
	builder.Plugin(harness.NewSecurityPlugin(f.plugin.config.Guard, f.plugin.Tools(), native...))
	agent, err := builder.Build(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	service, ok := bus.Resolve[*memory.Service](agent.Bus())
	if !ok || service != f.plugin.config.Service {
		t.Fatal("same-name memory service not replaced")
	}
	count := 0
	for _, plugin := range agent.Snapshot().Plugins {
		if plugin.Name == "memory" {
			count++
		}
	}
	if count != 1 {
		t.Fatal("default memory role duplicated")
	}
}
