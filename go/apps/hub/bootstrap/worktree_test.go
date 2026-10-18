// INPUT: Explicit local Git repository, actual HTTP/SDK write_file Worker and private product/native stores
// OUTPUT: True isolated execution, cold durable evidence, explicit no-commit adoption and reopen/no-replay acceptance
// POS: [L7 Hub product] Fixture credentials/paid models/global config/user repositories are never accessed
package bootstrap

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"os/exec"
	"path/filepath"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/go/apps/hub"
	"github.com/QuincySx/Alva/go/apps/hub/product"
	"github.com/QuincySx/Alva/go/integration/llm/provider"
)

func worktreeRepo(t *testing.T) string {
	t.Helper()
	repo := t.TempDir()
	home := t.TempDir()
	run := func(args ...string) {
		cmd := exec.CommandContext(t.Context(), "/usr/bin/git", args...)
		cmd.Dir = repo
		cmd.Env = []string{"PATH=/usr/bin:/bin", "HOME=" + home, "GIT_CONFIG_NOSYSTEM=1", "GIT_CONFIG_GLOBAL=/dev/null", "GIT_CONFIG_SYSTEM=/dev/null", "GIT_TERMINAL_PROMPT=0"}
		if data, err := cmd.CombinedOutput(); err != nil {
			t.Fatal("temporary fixture Git", err, string(data))
		}
	}
	run("init", "--initial-branch=main")
	if err := os.WriteFile(filepath.Join(repo, "tracked.txt"), []byte("base\n"), 0600); err != nil {
		t.Fatal(err)
	}
	run("add", "--", "tracked.txt")
	run("-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false", "commit", "-m", "fixture base")
	return repo
}
func TestManagedWorktreeActualWorkerWriteAndExplicitDurableAdoption(t *testing.T) {
	ctx := context.Background()
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/event-stream")
		var delta map[string]any
		finish := "stop"
		if calls.Add(1) == 1 {
			args, _ := json.Marshal(map[string]string{"path": "tracked.txt", "content": "actual isolated edit\n"})
			delta = map[string]any{"tool_calls": []any{map[string]any{"index": 0, "id": "actual-write", "type": "function", "function": map[string]any{"name": "write_file", "arguments": string(args)}}}}
			finish = "tool_calls"
		} else {
			delta = map[string]any{"content": "Worker wrote only its isolated workspace"}
		}
		data, _ := json.Marshal(map[string]any{"choices": []any{map[string]any{"index": 0, "delta": delta, "finish_reason": finish}}})
		fmt.Fprintf(w, "data: %s\n\ndata: [DONE]\n\n", data)
	}))
	defer server.Close()
	config := Config{DataDirectory: filepath.Join(t.TempDir(), "private"), ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: server.URL, Model: "worktree-fixture"}, GitExecutable: "/usr/bin/git", GitHostAccessApproved: true}
	a, err := Open(ctx, config)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { closeApp(t, a) }()
	repo := worktreeRepo(t)
	lead, err := a.Service.Create(ctx, "actual Lead", repo)
	if err != nil {
		t.Fatal(err)
	}
	team, err := a.Store.CreateTeam(ctx, lead.ID)
	if err != nil {
		t.Fatal(err)
	}
	worker, err := a.Service.AddWorker(ctx, lead.ID, team.Team.ID, "isolated", a.Service.DefaultTarget, true)
	if err != nil {
		t.Fatal(err)
	}
	if a.Runtime.HasActive() || calls.Load() != 0 {
		t.Fatal("startup/config/Team creation ran model")
	}
	if _, err = a.Service.ProvisionWorkerWorktree(ctx, lead.ID, team.Team.ID, worker.ID, false); err == nil {
		t.Fatal("isolation consent was inferred")
	}
	worker, err = a.Service.ProvisionWorkerWorktree(ctx, lead.ID, team.Team.ID, worker.ID, true)
	if err != nil {
		t.Fatal(err)
	}
	binding := worker.Worktree.Binding
	if calls.Load() != 0 || binding.Path == repo {
		t.Fatal("isolation ran model or retained shared cwd")
	}
	if _, err = a.Store.EnqueueWorker(ctx, lead.ID, team.Team.ID, worker.ID, "write tracked.txt in your actual workspace", nil); err != nil {
		t.Fatal(err)
	}
	if err = a.Service.Teams.Resume(ctx, lead.ID, team.Team.ID, worker.ID); err != nil {
		t.Fatal(err)
	}
	deadline := time.Now().Add(5 * time.Second)
	for calls.Load() == 0 && time.Now().Before(deadline) {
		time.Sleep(time.Millisecond)
	}
	if calls.Load() == 0 {
		t.Fatal("explicit Worker owner never consumed queue", a.Service.Teams.Status(worker.ID))
	}
	detail := awaitTerminal(t, a, worker.SessionID)
	branch, err := product.ActiveBranch(detail)
	if err != nil || detail.Session.Workspace != binding.Path || branch.NativeSessionID == "" {
		t.Fatal("actual Worker native/cwd mismatch", err)
	}
	data, err := os.ReadFile(filepath.Join(repo, "tracked.txt"))
	if err != nil || string(data) != "base\n" {
		t.Fatal("Worker polluted unadopted shared workspace", err)
	}
	data, err = os.ReadFile(filepath.Join(binding.Path, "tracked.txt"))
	if err != nil || string(data) != "actual isolated edit\n" {
		t.Fatal("real SDK did not write isolated file", string(data), err)
	}
	worker, err = a.Service.PreviewWorkerChanges(ctx, lead.ID, team.Team.ID, worker.ID)
	if err != nil {
		t.Fatal(err)
	}
	e := worker.Worktree.Evidence
	if e == nil || len(e.Changes) != 1 {
		t.Fatal("missing exact durable evidence")
	}
	page, err := a.Service.WorkerPatch(ctx, lead.ID, team.Team.ID, worker.ID, e.ID, 0)
	if err != nil || page.Text == "" {
		t.Fatal("cold raw page missing", err)
	}
	if _, err = a.Service.AdoptWorkerChanges(ctx, lead.ID, team.Team.ID, worker.ID, e.ID, worker.Worktree.Revision, false); err == nil {
		t.Fatal("adoption consent inferred")
	}
	worker, err = a.Service.AdoptWorkerChanges(ctx, lead.ID, team.Team.ID, worker.ID, e.ID, worker.Worktree.Revision, true)
	if err != nil || worker.Worktree.Adoption != "applied" {
		t.Fatal("real explicit adoption not settled", err)
	}
	data, err = os.ReadFile(filepath.Join(repo, "tracked.txt"))
	if err != nil || string(data) != "actual isolated edit\n" {
		t.Fatal("actual shared bytes not adopted", string(data), err)
	}
	closeApp(t, a)
	a, err = Open(ctx, config)
	if err != nil {
		t.Fatal(err)
	}
	team, err = a.Store.Team(ctx, lead.ID, team.Team.ID)
	if err != nil {
		t.Fatal(err)
	}
	for _, w := range team.Workers {
		if w.ID == worker.ID {
			worker = w
		}
	}
	if worker.Worktree.Adoption != "applied" || worker.Worktree.Evidence.ID != e.ID || calls.Load() != 2 || a.Runtime.HasActive() {
		t.Fatal("reopen lost evidence or replayed")
	}
	if _, err = a.Service.WorkerPatch(ctx, lead.ID, team.Team.ID, worker.ID, e.ID, 0); err != nil {
		t.Fatal("cold evidence failed after reopen", err)
	}
	if _, err = a.Service.AdoptWorkerChanges(ctx, lead.ID, team.Team.ID, worker.ID, e.ID, worker.Worktree.Revision, true); err == nil {
		t.Fatal("applied changes replayed")
	}
	if worker.Worktree.Binding.WorkerID != worker.ID {
		t.Fatal(hub.WorktreeBinding{})
	}
}
