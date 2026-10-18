// INPUT: Explicit local Git and temporary repositories with tracked files/hooks/filter traps
// OUTPUT: Real no-checkout isolation, no host hook/filter execution and ownership/budget refusal regressions
// POS: [L7 Hub product] Fixtures change only their own temporary repositories and never a user worktree or global Git config
package worktree

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func managerFixture(t *testing.T) (*Manager, string) {
	t.Helper()
	const git = "/usr/bin/git"
	if _, err := os.Stat(git); err != nil {
		t.Fatal("explicit fixture Git required; no executable discovery/install", err)
	}
	m, err := New(Config{GitExecutable: git, Root: filepath.Join(t.TempDir(), "private"), ApprovedHostAccess: true})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := m.Close(context.Background()); err != nil {
			t.Error(err)
		}
	})
	repo := t.TempDir()
	gitRun(t, m, repo, "init", "--initial-branch=main")
	if err = os.WriteFile(filepath.Join(repo, "tracked.txt"), []byte("base original\n"), 0600); err != nil {
		t.Fatal(err)
	}
	gitRun(t, m, repo, "add", "--", "tracked.txt")
	gitRun(t, m, repo, "-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false", "commit", "-m", "fixture base")
	return m, repo
}
func gitRun(t *testing.T, m *Manager, cwd string, args ...string) []byte {
	t.Helper()
	data, err := m.run(t.Context(), cwd, args, nil, nil, 1<<20)
	if err != nil {
		t.Fatal("fixture Git", args, err)
	}
	return data
}
func TestManagedWorktreeCopiesActualCommitWithoutRunningHooksOrSmudge(t *testing.T) {
	m, repo := managerFixture(t)
	if err := os.WriteFile(filepath.Join(repo, ".gitattributes"), []byte("tracked.txt filter=fixture\n"), 0600); err != nil {
		t.Fatal(err)
	}
	gitRun(t, m, repo, "add", "--", ".gitattributes")
	gitRun(t, m, repo, "-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false", "commit", "-m", "attributes fixture")
	marker := filepath.Join(t.TempDir(), "must-not-execute")
	hooks := filepath.Join(t.TempDir(), "hooks")
	if err := os.Mkdir(hooks, 0700); err != nil {
		t.Fatal(err)
	}
	script := "#!/bin/sh\nprintf compromised > '" + marker + "'\n"
	if err := os.WriteFile(filepath.Join(hooks, "post-checkout"), []byte(script), 0700); err != nil {
		t.Fatal(err)
	}
	gitRun(t, m, repo, "config", "core.hooksPath", hooks)
	gitRun(t, m, repo, "config", "filter.fixture.smudge", "/bin/sh -c 'printf compromised > "+marker+"; cat'")
	before := gitRun(t, m, repo, "status", "--porcelain=v1", "-z")
	handle, err := m.Create(t.Context(), repo, "worker-0123456789abcdef")
	if err != nil {
		t.Fatal(err)
	}
	if handle.Version != 1 || handle.BaseCommit == "" || strings.HasPrefix(handle.Path+"/", repo+"/") {
		t.Fatal("bad actual managed identity", handle)
	}
	data, err := os.ReadFile(filepath.Join(handle.Path, "tracked.txt"))
	if err != nil || string(data) != "base original\n" {
		t.Fatal("actual commit blob not copied", err)
	}
	if _, err = os.Stat(marker); !os.IsNotExist(err) {
		t.Fatal("repository hook or smudge executed", err)
	}
	if after := gitRun(t, m, repo, "status", "--porcelain=v1", "-z"); string(after) != string(before) {
		t.Fatal("worktree creation modified source workspace/index")
	}
	if status := gitRun(t, m, handle.Path, "status", "--porcelain=v1", "-z"); len(status) != 0 {
		t.Fatal("direct blobs/index are not a clean real Git worktree", string(status))
	}
	if err = os.WriteFile(filepath.Join(handle.Path, "tracked.txt"), []byte("isolated worker edit\n"), 0600); err != nil {
		t.Fatal(err)
	}
	data, err = os.ReadFile(filepath.Join(repo, "tracked.txt"))
	if err != nil || string(data) != "base original\n" {
		t.Fatal("unadopted Worker edit reached shared workspace", err)
	}
	if _, err = m.Create(t.Context(), repo, handle.WorkerID); err == nil {
		t.Fatal("existing managed target overwritten")
	}
}
func TestManagedWorktreeRefusesSubdirectorySymlinkSubmoduleAndMissingConsent(t *testing.T) {
	m, repo := managerFixture(t)
	if _, err := New(Config{GitExecutable: m.config.GitExecutable, Root: filepath.Join(t.TempDir(), "not-created")}); err == nil {
		t.Fatal("missing explicit host approval accepted")
	}
	sub := filepath.Join(repo, "sub")
	if err := os.Mkdir(sub, 0700); err != nil {
		t.Fatal(err)
	}
	if _, err := m.Create(t.Context(), sub, "worker-subdir"); err == nil {
		t.Fatal("repository subdirectory isolated")
	}
	if err := os.Symlink("tracked.txt", filepath.Join(repo, "link")); err != nil {
		t.Fatal(err)
	}
	gitRun(t, m, repo, "add", "--", "link")
	gitRun(t, m, repo, "-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false", "commit", "-m", "symlink fixture")
	if _, err := m.Create(t.Context(), repo, "worker-link"); err == nil {
		t.Fatal("source symlink accepted")
	}
	if _, err := os.Stat(filepath.Join(m.config.Root, "trees", "worker-link")); !os.IsNotExist(err) {
		t.Fatal("unsupported tree created partial target", err)
	}
	if _, err := parseTree([]byte("160000 commit 0123456789abcdef0123456789abcdef01234567\tmodule\x00")); err == nil {
		t.Fatal("submodule accepted")
	}
	if _, err := m.Create(t.Context(), repo, "../user-worktree"); err == nil {
		t.Fatal("arbitrary target path accepted")
	}
}
