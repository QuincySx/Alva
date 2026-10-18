// INPUT: MyGo actual input Tester, explicit fixture Git and durable product worktree service
// OUTPUT: No-start isolation/evidence controls, explicit raw page and two-step real file adoption regression
// POS: [L7 Hub product] Headless native controls do not claim OS window QA, autonomous Worker edits or model verification
//go:build desktop

package hubdesktop

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"testing"

	"github.com/QuincySx/Alva/go/apps/hub/worktree"
	"github.com/egoist/mygo/ui"
)

func TestNativeWorktreeIsolationEvidenceAndTwoStepAdoption(t *testing.T) {
	s, requests := fixture(t)
	ctx := context.Background()
	repo := t.TempDir()
	home := t.TempDir()
	git := func(args ...string) {
		cmd := exec.CommandContext(t.Context(), "/usr/bin/git", args...)
		cmd.Dir = repo
		cmd.Env = []string{"PATH=/usr/bin:/bin", "HOME=" + home, "GIT_CONFIG_NOSYSTEM=1", "GIT_CONFIG_GLOBAL=/dev/null", "GIT_CONFIG_SYSTEM=/dev/null"}
		if data, err := cmd.CombinedOutput(); err != nil {
			t.Fatal(err, string(data))
		}
	}
	git("init", "--initial-branch=main")
	if err := os.WriteFile(filepath.Join(repo, "tracked.txt"), []byte("original\n"), 0600); err != nil {
		t.Fatal(err)
	}
	git("add", "--", "tracked.txt")
	git("-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false", "commit", "-m", "temporary fixture")
	var err error
	s.Worktrees, err = worktree.New(worktree.Config{GitExecutable: "/usr/bin/git", Root: filepath.Join(t.TempDir(), "private"), ApprovedHostAccess: true})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = s.Worktrees.Close(context.Background()) })
	lead, err := s.Create(ctx, "Lead", repo)
	if err != nil {
		t.Fatal(err)
	}
	team, err := s.Teams.Store.CreateTeam(ctx, lead.ID)
	if err != nil {
		t.Fatal(err)
	}
	worker, err := s.AddWorker(ctx, lead.ID, team.Team.ID, "ui-worker", s.DefaultTarget, true)
	if err != nil {
		t.Fatal(err)
	}
	page := NewNativePage(s)
	page.selected = lead.ID
	page.detail, err = s.Store.Detail(ctx, lead.ID)
	if err != nil {
		t.Fatal(err)
	}
	page.applyDetail()
	tester := ui.NewTester(page.Build, 1700, 4200)
	if err = tester.Click("Collaboration"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if err = tester.Click("ui-worker · active"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if len(requests) != 0 || s.Runtime.HasActive() {
		t.Fatal("viewing Team executed task")
	}
	if err = tester.Click("Approve Git and isolate new Worker"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if page.problem != "" {
		t.Fatal(page.problem)
	}
	for _, actual := range page.team.Workers {
		if actual.ID == worker.ID {
			worker = actual
		}
	}
	if worker.Worktree == nil || worker.Worktree.State != "ready" {
		t.Fatal("explicit isolate did not create real target")
	}
	if err = os.WriteFile(filepath.Join(worker.Worktree.Binding.Path, "tracked.txt"), []byte("fixture isolated proposal\n"), 0600); err != nil {
		t.Fatal(err)
	}
	if err = tester.Click("Generate exact Worker diff evidence"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if page.problem != "" || page.worktreePatch.Text != "" {
		t.Fatal("metadata preview failed or hydrated raw patch", page.problem)
	}
	if err = tester.Click("Read raw diff page (may contain project secrets)"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if page.problem != "" || page.worktreePatch.Text == "" {
		t.Fatal("explicit raw page missing", page.problem)
	}
	if err = tester.Click("Review shared-file adoption confirmation"); err != nil {
		t.Fatal(err)
	}
	// A metadata refresh from another window cannot transfer this approval to
	// a new evidence revision, even if the same Worker remains selected.
	for i := range page.team.Workers {
		if page.team.Workers[i].ID == worker.ID {
			page.team.Workers[i].Worktree.Revision++
		}
	}
	tester.Frame()
	if tester.HasText("Confirm apply reviewed paths to shared workspace") {
		t.Fatal("old confirmation authorized a different evidence revision")
	}
	for i := range page.team.Workers {
		if page.team.Workers[i].ID == worker.ID {
			page.team.Workers[i].Worktree.Revision--
		}
	}
	tester.Frame()
	data, err := os.ReadFile(filepath.Join(repo, "tracked.txt"))
	if err != nil || string(data) != "original\n" {
		t.Fatal("review automatically adopted", err)
	}
	if err = tester.Click("Confirm apply reviewed paths to shared workspace"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if page.problem != "" {
		t.Fatal(page.problem)
	}
	data, err = os.ReadFile(filepath.Join(repo, "tracked.txt"))
	if err != nil || string(data) != "fixture isolated proposal\n" {
		t.Fatal("actual confirmed bytes not adopted", err)
	}
	if err = tester.Click("Review managed worktree removal"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if page.problem != "" {
		t.Fatal(page.problem)
	}
	if _, err := os.Stat(worker.Worktree.Binding.Path); err != nil {
		t.Fatal("review deleted worktree", err)
	}
	if err = tester.Click("Review deletion confirmation"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if err = tester.Click("Confirm delete reviewed worktree and retire Worker"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if page.problem != "" {
		t.Fatal(page.problem)
	}
	if _, err := os.Stat(worker.Worktree.Binding.Path); !os.IsNotExist(err) {
		t.Fatal("confirmed managed removal did not remove target", err)
	}
	data, err = os.ReadFile(filepath.Join(repo, "tracked.txt"))
	if err != nil || string(data) != "fixture isolated proposal\n" {
		t.Fatal("removal changed shared adopted bytes", err)
	}
	if err = tester.Click("Chat"); err != nil {
		t.Fatal(err)
	}
	if page.worktreePatch.Text != "" || page.worktreeConfirm != "" {
		t.Fatal("hidden pane retained raw patch/confirmation")
	}
	if len(requests) != 0 || s.Runtime.HasActive() {
		t.Fatal("worktree controls executed model/Worker queue")
	}
}
