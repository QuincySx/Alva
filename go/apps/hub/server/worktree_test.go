// INPUT: Actual temporary Git repository, authenticated HTTP and private worktree/product stores
// OUTPUT: Explicit isolated provisioning, cold diff pages, exact reviewed adoption and ownership/staleness rejection
// POS: [L7 Hub product] Worktree HTTP acceptance applies literal bytes without model execution or commits.
package server

import (
	"context"
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/go/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/go/apps/hub/product"
)

func TestHTTPManagedWorktreeUsesExactOwnedEvidenceAndConsent(t *testing.T) {
	if _, err := os.Stat("/usr/bin/git"); err != nil {
		t.Skip("explicit fixture Git unavailable")
	}
	ctx := context.Background()
	repo, home := t.TempDir(), t.TempDir()
	git := func(args ...string) {
		t.Helper()
		cmd := exec.CommandContext(t.Context(), "/usr/bin/git", args...)
		cmd.Dir = repo
		cmd.Env = []string{"PATH=/usr/bin:/bin", "HOME=" + home, "GIT_CONFIG_NOSYSTEM=1", "GIT_CONFIG_GLOBAL=/dev/null", "GIT_CONFIG_SYSTEM=/dev/null", "GIT_TERMINAL_PROMPT=0"}
		if data, err := cmd.CombinedOutput(); err != nil {
			t.Fatal(err, string(data))
		}
	}
	git("init", "--initial-branch=main")
	if err := os.WriteFile(filepath.Join(repo, "tracked.txt"), []byte("original\n"), 0600); err != nil {
		t.Fatal(err)
	}
	git("add", "--", "tracked.txt")
	git("-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false", "commit", "-m", "fixture")
	f := newFixture(t, bootstrap.Config{Demo: true, GitExecutable: "/usr/bin/git", GitHostAccessApproved: true})
	lead, err := f.app.Service.Create(ctx, "Lead", repo)
	if err != nil {
		t.Fatal(err)
	}
	other := create(t, f, "other Lead")
	team, err := f.app.Store.CreateTeam(ctx, lead.ID)
	if err != nil {
		t.Fatal(err)
	}
	w, err := f.app.Service.AddWorker(ctx, lead.ID, team.Team.ID, "isolated", f.app.Service.DefaultTarget, true)
	if err != nil {
		t.Fatal(err)
	}
	base := "/v1/sessions/" + lead.ID + "/teams/" + team.Team.ID + "/workers/" + w.ID + "/worktree/"
	body(t, f.request(t, "POST", base+"provision", []byte(`{"approved":false}`), nil), 409)
	body(t, f.request(t, "POST", strings.Replace(base, lead.ID, other.ID, 1)+"provision", []byte(`{"approved":true}`), nil), 404)
	if json.Unmarshal(body(t, f.request(t, "POST", base+"provision", []byte(`{"approved":true}`), nil), 200), &w) != nil || w.Worktree == nil || w.Worktree.State != "ready" {
		t.Fatal("actual isolated worktree absent")
	}
	if err = os.WriteFile(filepath.Join(w.Worktree.Binding.Path, "tracked.txt"), []byte("explicit reviewed proposal\n"), 0600); err != nil {
		t.Fatal(err)
	}
	if json.Unmarshal(body(t, f.request(t, "POST", base+"preview", []byte(`{}`), nil), 200), &w) != nil || w.Worktree.Evidence == nil {
		t.Fatal("saved exact evidence absent")
	}
	evidence := w.Worktree.Evidence
	var patch product.WorktreePatchPage
	if json.Unmarshal(body(t, f.request(t, "GET", base+"patch?evidence="+evidence.ID+"&offset=0", nil, nil), 200), &patch) != nil || !strings.Contains(patch.Text, "explicit reviewed proposal") {
		t.Fatal("real patch page absent")
	}
	confirm := map[string]any{"evidence": evidence.ID, "revision": w.Worktree.Revision, "approved": false}
	raw, _ := json.Marshal(confirm)
	body(t, f.request(t, "POST", base+"adopt", raw, nil), 409)
	before, _ := os.ReadFile(filepath.Join(repo, "tracked.txt"))
	if string(before) != "original\n" {
		t.Fatal("preview/adopt without consent changed Lead")
	}
	confirm["approved"] = true
	confirm["revision"] = w.Worktree.Revision + 1
	raw, _ = json.Marshal(confirm)
	body(t, f.request(t, "POST", base+"adopt", raw, nil), 409)
	confirm["revision"] = w.Worktree.Revision
	raw, _ = json.Marshal(confirm)
	if json.Unmarshal(body(t, f.request(t, "POST", base+"adopt", raw, nil), 200), &w) != nil || w.Worktree.Adoption != "applied" {
		t.Fatal("actual adoption not proven")
	}
	after, _ := os.ReadFile(filepath.Join(repo, "tracked.txt"))
	if string(after) != "explicit reviewed proposal\n" {
		t.Fatal("Lead bytes not adopted")
	}
	body(t, f.request(t, "POST", base+"adopt", raw, nil), 409)
	if json.Unmarshal(body(t, f.request(t, "POST", base+"review-removal", []byte(`{}`), nil), 200), &w) != nil || w.Worktree.Removal == nil {
		t.Fatal("saved removal review absent")
	}
	removal := map[string]any{"revision": w.Worktree.Revision, "snapshot": w.Worktree.Removal.WorkerSHA256, "approved": false}
	raw, _ = json.Marshal(removal)
	body(t, f.request(t, "POST", base+"remove", raw, nil), 409)
	if _, err := os.Stat(w.Worktree.Binding.Path); err != nil {
		t.Fatal("missing consent removed files", err)
	}
	removal["approved"] = true
	raw, _ = json.Marshal(removal)
	body(t, f.request(t, "POST", strings.Replace(base, lead.ID, other.ID, 1)+"remove", raw, nil), 404)
	if json.Unmarshal(body(t, f.request(t, "POST", base+"remove", raw, nil), 200), &w) != nil || w.Worktree.State != "removed" {
		t.Fatal("actual removal not proven", w)
	}
	body(t, f.request(t, "POST", base+"remove", raw, nil), 409)
	// Inspector evidence and the shared adopted files survive Worker retirement.
	body(t, f.request(t, "GET", base+"patch?evidence="+evidence.ID+"&offset=0", nil, nil), 200)
	after, err = os.ReadFile(filepath.Join(repo, "tracked.txt"))
	if err != nil || string(after) != "explicit reviewed proposal\n" {
		t.Fatal("removal changed shared adopted files", err)
	}
	git("show-ref", "--verify", "refs/heads/"+w.Worktree.Binding.Branch)
	if f.app.Runtime.HasActive() {
		t.Fatal("cold operations left execution active")
	}
}
