// INPUT: Real temporary Git repository, linked/detached worktrees and the explicit approved Manager.
// OUTPUT: Canonical common-root import identity, live multi-worktree listing and cancellation regressions.
// POS: [L7 Hub product tests] Discovery is Git state, not Worker bindings or ambient filesystem scanning.
package worktree

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"
)

func TestDeveloperWorktreeDiscoveryUsesLiveGitCommonRoot(t *testing.T) {
	m, repository := managerFixture(t)
	linked := filepath.Join(t.TempDir(), "linked with space")
	gitRun(t, m, repository, "worktree", "add", "-b", "feature/navigation", "--", linked)
	detached := filepath.Join(t.TempDir(), "detached")
	gitRun(t, m, repository, "worktree", "add", "--detach", "--", detached, "HEAD")

	mainIdentity, err := m.InspectDeveloperRepository(t.Context(), repository)
	if err != nil {
		t.Fatal(err)
	}
	canonicalRepository, err := filepath.EvalSymlinks(repository)
	if err != nil {
		t.Fatal(err)
	}
	linkedIdentity, err := m.InspectDeveloperRepository(t.Context(), linked)
	if err != nil {
		t.Fatal(err)
	}
	if mainIdentity != linkedIdentity || mainIdentity.Path != canonicalRepository || !filepath.IsAbs(mainIdentity.CommonDir) {
		t.Fatal("linked worktree did not normalize to the same canonical main/common-root identity", mainIdentity, linkedIdentity)
	}
	items, err := m.ListDeveloperWorktrees(t.Context(), "repository-fixture", mainIdentity.Path)
	if err != nil {
		t.Fatal(err)
	}
	canonicalLinked, err := filepath.EvalSymlinks(linked)
	if err != nil {
		t.Fatal(err)
	}
	canonicalDetached, err := filepath.EvalSymlinks(detached)
	if err != nil {
		t.Fatal(err)
	}
	if len(items) != 3 || !items[0].Main || items[0].Path != canonicalRepository || items[0].Branch != "main" ||
		items[1].Main || items[1].Path != canonicalLinked || items[1].Branch != "feature/navigation" ||
		items[2].Main || items[2].Path != canonicalDetached || !items[2].Detached || items[2].Branch != "" {
		t.Fatal("actual Git worktree listing lost main/linked/detached identities", items)
	}
	for _, item := range items {
		if item.RepositoryID != "repository-fixture" || len(item.Name) == 0 || len(item.Head) != 40 && len(item.Head) != 64 {
			t.Fatal("worktree listing lacks live product/Git identity", item)
		}
	}
	subdirectory := filepath.Join(repository, "nested")
	if err = os.Mkdir(subdirectory, 0o700); err != nil {
		t.Fatal(err)
	}
	if _, err = m.InspectDeveloperRepository(t.Context(), subdirectory); err == nil {
		t.Fatal("subdirectory was accepted as a repository/worktree toplevel")
	}
	cancelled, cancel := context.WithCancel(t.Context())
	cancel()
	if _, err = m.ListDeveloperWorktrees(cancelled, "repository-fixture", repository); !errors.Is(err, context.Canceled) {
		t.Fatal("cancelled Git discovery did not propagate cancellation", err)
	}
}
