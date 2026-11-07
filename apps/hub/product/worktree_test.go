// INPUT: Real private Git fixture, durable product facts and a blocked provisioning admission
// OUTPUT: Actual shared lease serialization and successful physical provisioning settlement with a newly admitted Lead
// POS: [L7 Hub product] Uses only temporary Git repositories and local demo harness; no user repository, network or accounts
package product

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/worktree"
)

type worktreeProvisionBarrier struct {
	ServiceStore
	hub.WorktreeStore
	entered chan struct{}
	release chan struct{}
}

func (b *worktreeProvisionBarrier) BeginWorkerWorktree(ctx context.Context, lead, team, worker string) (hub.Worker, error) {
	w, err := b.WorktreeStore.BeginWorkerWorktree(ctx, lead, team, worker)
	if err != nil {
		return w, err
	}
	close(b.entered)
	select {
	case <-b.release:
	case <-ctx.Done():
		return w, ctx.Err()
	}
	return w, nil
}

func TestWorktreeProvisionSettlementPrecedesAdmittedLeadWorkspaceExecution(t *testing.T) {
	s, requests := fixture(t)
	ctx := t.Context()
	repo, home := t.TempDir(), t.TempDir()
	git := func(args ...string) {
		t.Helper()
		cmd := exec.CommandContext(ctx, "/usr/bin/git", args...)
		cmd.Dir = repo
		cmd.Env = []string{"PATH=/usr/bin:/bin", "HOME=" + home, "GIT_CONFIG_NOSYSTEM=1", "GIT_CONFIG_GLOBAL=/dev/null", "GIT_CONFIG_SYSTEM=/dev/null", "GIT_TERMINAL_PROMPT=0"}
		if data, err := cmd.CombinedOutput(); err != nil {
			t.Fatal(err, string(data))
		}
	}
	git("init", "--initial-branch=main")
	if err := os.WriteFile(filepath.Join(repo, "tracked.txt"), []byte("base\n"), 0600); err != nil {
		t.Fatal(err)
	}
	git("add", "--", "tracked.txt")
	git("-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false", "commit", "-m", "temporary fixture")
	var err error
	s.Worktrees, err = worktree.New(worktree.Config{GitExecutable: "/usr/bin/git", Root: filepath.Join(t.TempDir(), "managed"), ApprovedHostAccess: true})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = s.Worktrees.Close(context.Background()) })
	lead, err := s.Create(ctx, "Lead admitted during provision", repo)
	if err != nil {
		t.Fatal(err)
	}
	team, err := s.Teams.Store.CreateTeam(ctx, lead.ID)
	if err != nil {
		t.Fatal(err)
	}
	worker, err := s.AddWorker(ctx, lead.ID, team.Team.ID, "isolated", s.DefaultTarget, true)
	if err != nil {
		t.Fatal(err)
	}
	barrier := &worktreeProvisionBarrier{ServiceStore: s.Store, WorktreeStore: s.Store.(hub.WorktreeStore), entered: make(chan struct{}), release: make(chan struct{})}
	s.Store = barrier
	provisioned := make(chan error, 1)
	go func() {
		_, err := s.ProvisionWorkerWorktree(ctx, lead.ID, team.Team.ID, worker.ID, true)
		provisioned <- err
	}()
	select {
	case <-barrier.entered:
	case <-time.After(3 * time.Second):
		t.Fatal("provisioning did not acquire its workspace and admit")
	}
	_, done, err := s.Runtime.Start(ctx, lead.ID, "run after actual provisioning releases the workspace")
	if err != nil {
		close(barrier.release)
		t.Fatal(err)
	}
	detail, err := s.Store.Detail(ctx, lead.ID)
	if err != nil || detail.Session.Status != hub.Running || len(requests) != 0 {
		close(barrier.release)
		t.Fatal("Lead did not durably admit behind the exclusive provision lease", detail, err)
	}
	close(barrier.release)
	if err = <-provisioned; err != nil {
		t.Fatal("successful Git owner could not publish ready before releasing its lease", err)
	}
	if err = <-done; err != nil {
		t.Fatal(err)
	}
	actual, err := s.Teams.Store.Team(ctx, lead.ID, team.Team.ID)
	if err != nil {
		t.Fatal(err)
	}
	for _, current := range actual.Workers {
		if current.ID != worker.ID {
			continue
		}
		if current.Worktree == nil || current.Worktree.State != "ready" {
			t.Fatal("actual target remained unavailable", current)
		}
		data, err := os.ReadFile(filepath.Join(current.Worktree.Binding.Path, "tracked.txt"))
		child, childErr := s.Store.Detail(ctx, current.SessionID)
		if err != nil || string(data) != "base\n" || childErr != nil || child.Session.Workspace != current.Worktree.Binding.Path {
			t.Fatal("ready receipt lost actual isolated files or native cwd", err, childErr)
		}
	}
}
