// INPUT: Actual temporary Git repositories, unadopted managed edits and reviewed removal snapshots
// OUTPUT: Real scoped deletion, stale/foreign rejection and retained shared files/branch/evidence regressions
// POS: [L7 Hub product] Tests remove only freshly created fixture worktrees; no user workspace or historical cache is touched.
package worktree

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"
)

func TestRemovalDeletesOnlyReviewedManagedTreeAndRetainsBranchEvidenceAndSharedChanges(t *testing.T) {
	m, repo := managerFixture(t)
	h, err := m.Create(t.Context(), repo, "worker-remove")
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(h.Path, "tracked.txt"), []byte("unadopted proposal\n"), 0600); err != nil {
		t.Fatal(err)
	}
	evidence, err := m.Preview(t.Context(), h)
	if err != nil {
		t.Fatal(err)
	}
	// Moving the Lead HEAD must not strand an old managed worktree forever.
	if err := os.WriteFile(filepath.Join(repo, "lead.txt"), []byte("human shared change\n"), 0600); err != nil {
		t.Fatal(err)
	}
	gitRun(t, m, repo, "add", "--", "lead.txt")
	gitRun(t, m, repo, "-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false", "commit", "-m", "advance Lead independently")
	head := string(gitRun(t, m, repo, "rev-parse", "HEAD"))
	review, err := m.ReviewRemoval(t.Context(), h)
	if err != nil || review.Files != 1 || review.Bytes != len("unadopted proposal\n") {
		t.Fatal("actual review missing", review, err)
	}
	if err := m.Remove(t.Context(), h, review); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Lstat(h.Path); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("target still present", err)
	}
	if string(gitRun(t, m, repo, "rev-parse", "HEAD")) != head {
		t.Fatal("Lead HEAD changed")
	}
	gitRun(t, m, repo, "show-ref", "--verify", "refs/heads/"+h.Branch)
	if data, err := os.ReadFile(filepath.Join(repo, "lead.txt")); err != nil || string(data) != "human shared change\n" {
		t.Fatal("shared human file changed", err)
	}
	if _, _, err := m.ReadPatch(t.Context(), evidence, 0, 4096); err != nil {
		t.Fatal("saved recovery evidence lost", err)
	}
	if err := m.Remove(t.Context(), h, review); err == nil {
		t.Fatal("missing target removal reported as a new success")
	}
}

func TestRemovalRejectsStaleIndexOrBytesAndUnmanagedPath(t *testing.T) {
	for _, indexOnly := range []bool{false, true} {
		t.Run(map[bool]string{false: "changed files", true: "changed index"}[indexOnly], func(t *testing.T) {
			m, repo := managerFixture(t)
			h, err := m.Create(t.Context(), repo, "worker-stale-removal")
			if err != nil {
				t.Fatal(err)
			}
			review, err := m.ReviewRemoval(t.Context(), h)
			if err != nil {
				t.Fatal(err)
			}
			if indexOnly {
				gitRun(t, m, h.Path, "read-tree", "--empty")
			} else if err := os.WriteFile(filepath.Join(h.Path, "unreviewed.txt"), []byte("new work"), 0600); err != nil {
				t.Fatal(err)
			}
			if err := m.Remove(t.Context(), h, review); err == nil {
				t.Fatal("stale reviewed tree was deleted")
			}
			if _, err := os.Stat(h.Path); err != nil {
				t.Fatal("refusal deleted the managed target", err)
			}
			foreign := h
			foreign.Path = repo
			if _, err := m.ReviewRemoval(t.Context(), foreign); err == nil {
				t.Fatal("unmanaged shared path accepted")
			}
			ctx, cancel := context.WithCancel(context.Background())
			cancel()
			if err := m.Remove(ctx, h, review); err == nil {
				t.Fatal("cancelled removal executed")
			}
		})
	}
}
