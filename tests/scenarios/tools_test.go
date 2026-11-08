// INPUT: hubFixture real Alva assembly, fakeModel tool-call scripts, a temporary project workspace and /bin/sh
// OUTPUT: Scenarios where the model drives every built-in file tool and the shell through the real loop
// POS: [scenario] Checks the file system, the Message Stream tool blocks and the execution trace agree on each call.
package scenarios

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/llm/provider"
)

// enableShell saves an explicit /bin/sh shell and its separate host consent.
func enableShell(t *testing.T, h *hubFixture, id string) {
	t.Helper()
	ctx := context.Background()
	if err := h.service().SaveToolsDraft(ctx, id, `{"shell":{"executable":"/bin/sh","enabled":false,"host_access_approved":false},"web":{"enabled":false,"network_access_approved":false}}`); err != nil {
		t.Fatal("save shell draft:", err)
	}
	if err := h.service().SetToolEnabled(ctx, id, "shell", true, true); err != nil {
		t.Fatal("enable shell:", err)
	}
}

func TestModelDrivesFileToolsAndShellAndViewsAgree(t *testing.T) {
	for _, kind := range []provider.Kind{provider.OpenAIChatKind, provider.AnthropicKind} {
		t.Run(string(kind), func(t *testing.T) { modelDrivesFileToolsAndShell(t, kind) })
	}
}

func modelDrivesFileToolsAndShell(t *testing.T, kind provider.Kind) {
	workspace := t.TempDir()
	if err := os.WriteFile(filepath.Join(workspace, "notes.txt"), []byte("project notes\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	model := newFakeModel(t, kind,
		call("c-list", "list_directory", `{"path":"."}`),
		call("c-read", "read_file", `{"path":"notes.txt"}`),
		call("c-write", "write_file", `{"path":"out.txt","content":"alpha\nbeta\n"}`),
		call("c-edit", "edit_file", `{"path":"out.txt","old":"beta","new":"gamma"}`),
		call("c-search", "search_files", `{"path":".","query":"gamma"}`),
		call("c-shell", "execute_shell", `{"command":"printf shell-ok > shell.txt"}`),
		call("c-missing", "edit_file", `{"path":"out.txt","old":"not present","new":"x"}`),
		reply{Text: []string{"all ", "done"}, Input: 30, Usage: 4},
	)
	h := openHub(t, liveConfig(t, model))
	s := h.create("tools", workspace)
	if err := h.service().SetPermissionPolicy(context.Background(), s.ID, hub.PermissionFull, true); err != nil {
		t.Fatal(err)
	}
	enableShell(t, h, s.ID)
	h.send(s.ID, "use every tool")
	h.expect(s.ID, engine.Succeeded)

	// 1. The file system holds the real effects.
	if data, err := os.ReadFile(filepath.Join(workspace, "out.txt")); err != nil || string(data) != "alpha\ngamma\n" {
		t.Fatalf("write+edit result %q %v", data, err)
	}
	if data, err := os.ReadFile(filepath.Join(workspace, "shell.txt")); err != nil || string(data) != "shell-ok" {
		t.Fatalf("shell effect %q %v", data, err)
	}
	// 2. Each result went back to the model in the next request, the failed
	// read as an error the model could react to.
	if model.calls() != 8 {
		t.Fatalf("model requests %d, want 8", model.calls())
	}
	ids := []string{"c-list", "c-read", "c-write", "c-edit", "c-search", "c-shell", "c-missing"}
	for i, id := range ids {
		results := model.request(i + 2).toolResults()
		if _, ok := results[id]; !ok {
			t.Fatalf("request %d lacks the result of %s: %v", i+2, id, results)
		}
	}
	if r := model.request(2).toolResults()["c-list"]; !strings.Contains(r, "notes.txt") {
		t.Fatalf("directory listing did not reach the model: %s", r)
	}
	if r := model.request(6).toolResults()["c-search"]; !strings.Contains(r, "out.txt") {
		t.Fatalf("search result did not reach the model: %s", r)
	}
	if missing := model.request(8).toolResults()["c-missing"]; !strings.Contains(missing, "Old text is missing") {
		t.Fatalf("failed edit did not reach the model as an error: %s", missing)
	}
	// 3. The Message Stream shows one typed block per call.
	blocks := h.blocks(s.ID)
	want := map[string]engine.ToolKind{"c-read": engine.ToolRead, "c-write": engine.ToolEdit, "c-edit": engine.ToolEdit, "c-search": engine.ToolSearch, "c-shell": engine.ToolCommand, "c-list": engine.ToolOther, "c-missing": engine.ToolEdit}
	for id, kind := range want {
		b := toolBlock(t, blocks, id)
		if b.Tool == nil || b.Tool.Kind != kind {
			t.Fatalf("%s tool view %+v, want kind %s", id, b.Tool, kind)
		}
		wantStatus := product.BlockStatusCompleted
		if id == "c-missing" {
			wantStatus = product.BlockStatusFailed
		}
		if b.Status != wantStatus {
			t.Fatalf("%s block status %s, want %s", id, b.Status, wantStatus)
		}
	}
	if b := toolBlock(t, blocks, "c-write"); b.Tool.Path != "out.txt" {
		t.Fatalf("write block path %q", b.Tool.Path)
	}
	if b := toolBlock(t, blocks, "c-shell"); b.Tool.Command != "printf shell-ok > shell.txt" || b.Tool.ExitCode == nil || *b.Tool.ExitCode != 0 {
		t.Fatalf("shell block %+v", b.Tool)
	}
	replies := blocksOf(blocks, product.BlockKindReply)
	if len(replies) == 0 || replies[len(replies)-1].Text != "all done" {
		t.Fatalf("final reply %+v", replies)
	}
	// 4. The trace has a tool step for each call, with the same outcome.
	steps := map[string]product.TraceStep{}
	for _, step := range h.trace(s.ID) {
		if step.Kind == product.TraceStepTool {
			steps[step.CallID] = step
		}
	}
	for _, id := range ids {
		step, ok := steps[id]
		if !ok {
			t.Fatalf("trace has no tool step for %s (steps %v)", id, steps)
		}
		if step.Open {
			t.Fatalf("trace step %s still open", id)
		}
	}
	if steps["c-missing"].Status == steps["c-write"].Status {
		t.Fatalf("trace does not tell the failed call from the successful one: %q", steps["c-missing"].Status)
	}
}

// Stop during a long shell command ends the Turn promptly and kills the
// command before it reaches its later side effect.
func TestStopDuringALongShellCommandKillsIt(t *testing.T) {
	workspace := t.TempDir()
	model := newFakeModel(t, provider.OpenAIChatKind, call("c-sleep", "execute_shell", `{"command":"printf started > started.txt; sleep 20; printf late > late.txt"}`))
	h := openHub(t, liveConfig(t, model))
	s := h.create("stop shell", workspace)
	if err := h.service().SetPermissionPolicy(context.Background(), s.ID, hub.PermissionFull, true); err != nil {
		t.Fatal(err)
	}
	enableShell(t, h, s.ID)
	h.send(s.ID, "run the slow thing")
	waitFor(t, "the command to start", func() bool {
		_, err := os.Stat(filepath.Join(workspace, "started.txt"))
		return err == nil
	})
	execution, _ := h.app.Runtime.ActiveExecution(s.ID)
	started := time.Now()
	if err := h.app.Runtime.CancelExecution(context.Background(), s.ID, execution); err != nil {
		t.Fatal(err)
	}
	h.expect(s.ID, engine.Cancelled)
	if elapsed := time.Since(started); elapsed > 10*time.Second {
		t.Fatalf("Stop took %s", elapsed)
	}
	if b := toolBlock(t, h.blocks(s.ID), "c-sleep"); b.Status == product.BlockStatusRunning || b.Status == product.BlockStatusCompleted {
		t.Fatalf("stopped command block %+v", b)
	}
	time.Sleep(300 * time.Millisecond)
	if _, err := os.Stat(filepath.Join(workspace, "late.txt")); !os.IsNotExist(err) {
		t.Fatal("the stopped command kept running", err)
	}
}

// Under the documented file-tool rule (extension/files AGENTS.md: real IO
// failures are execution errors; only argument/binary/ambiguity problems are
// model-recoverable) a read of a missing path ends the whole Turn and the
// model never sees the error. The same holds for a directory or a symlink.
// Whether ordinary "not found" should instead reach the model is an open
// product decision; this scenario pins today's behavior and its recovery.
func TestReadingAMissingFileEndsTheTurnAndTheUserCanRetry(t *testing.T) {
	workspace := t.TempDir()
	model := newFakeModel(t, provider.OpenAIChatKind,
		call("c-missing", "read_file", `{"path":"missing.txt"}`),
		text("ok"),
	)
	h := openHub(t, liveConfig(t, model))
	s := h.create("missing file", workspace)
	h.send(s.ID, "read missing.txt")
	h.expect(s.ID, engine.Failed)
	if model.calls() != 1 {
		t.Fatalf("model requests %d: the error reached the model after all", model.calls())
	}
	errs := errorEvents(h.history(s.ID))
	if len(errs) != 1 || errs[0].Failure != engine.FailureExecution || !strings.Contains(errs[0].Text, "no such file") {
		t.Fatalf("missing file error %+v", errs)
	}
	if b := toolBlock(t, h.blocks(s.ID), "c-missing"); b.Status != product.BlockStatusFailed {
		t.Fatalf("missing file block %+v", b)
	}
	h.send(s.ID, "never mind")
	h.expect(s.ID, engine.Succeeded)
}
