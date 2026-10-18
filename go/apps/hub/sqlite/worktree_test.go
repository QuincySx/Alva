// INPUT: Real private product SQLite, full Worker identities and trusted isolated binding/evidence fixtures
// OUTPUT: Atomic cwd admission, revision/foreign rejection and crash unknown-not-replay regressions
// POS: [L7 Hub product] Store tests prove product transitions, not actual Git execution or file adoption
package sqlite

import (
	"context"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/go/apps/hub"
)

func TestWorktreeRemovalRetiresWithoutRetargetingHistoryAndCrashNeverReplays(t *testing.T) {
	for _, crash := range []bool{false, true} {
		t.Run(map[bool]string{false: "completed", true: "interrupted"}[crash], func(t *testing.T) {
			s := newStore(t)
			ctx := context.Background()
			lead, team, w, _ := teamFixture(t, s)
			w, err := s.BeginWorkerWorktree(ctx, lead.ID, team.Team.ID, w.ID)
			if err != nil {
				t.Fatal(err)
			}
			binding := hub.WorktreeBinding{Version: 1, WorkerID: w.ID, Workspace: lead.Workspace, Path: filepath.Join(t.TempDir(), "managed"), Branch: "alva/" + w.ID, BaseCommit: strings.Repeat("a", 40)}
			w, err = s.FinishWorkerWorktree(ctx, lead.ID, team.Team.ID, w.ID, w.Worktree.Revision, binding, "")
			if err != nil {
				t.Fatal(err)
			}
			enqueue(t, s, lead.ID, team.Team.ID, w.ID, "preserve native Worker history", nil)
			claimed, err := s.ClaimWorkerTask(ctx, lead.ID, team.Team.ID, w.ID)
			if err != nil || claimed.Claim == nil {
				t.Fatal(err)
			}
			complete(t, s, *claimed.Claim, "fixture-native")
			before, err := s.History(ctx, w.SessionID, 0, 128)
			if err != nil || len(before) == 0 {
				t.Fatal("missing historical Turn", err)
			}
			review := hub.WorktreeRemovalReview{WorkerSHA256: strings.Repeat("b", 64), IndexSHA256: strings.Repeat("c", 64), Head: binding.BaseCommit, Files: 1, Bytes: 12, CreatedAt: 1}
			if _, err := s.SaveWorkerRemovalReview(ctx, lead.ID, team.Team.ID, w.ID, w.Worktree.Revision+1, review); err == nil {
				t.Fatal("stale review saved")
			}
			w, err = s.SaveWorkerRemovalReview(ctx, lead.ID, team.Team.ID, w.ID, w.Worktree.Revision, review)
			if err != nil {
				t.Fatal(err)
			}
			if _, err := s.BeginWorkerRemoval(ctx, w.SessionID, team.Team.ID, w.ID, w.Worktree.Revision, review.WorkerSHA256); err == nil {
				t.Fatal("foreign Lead granted removal")
			}
			if _, err := s.BeginWorkerRemoval(ctx, lead.ID, team.Team.ID, w.ID, w.Worktree.Revision, strings.Repeat("f", 64)); err == nil {
				t.Fatal("unreviewed snapshot removed")
			}
			w, err = s.BeginWorkerRemoval(ctx, lead.ID, team.Team.ID, w.ID, w.Worktree.Revision, review.WorkerSHA256)
			if err != nil {
				t.Fatal(err)
			}
			if _, err := s.BeginTurn(ctx, w.SessionID, "race removal"); err == nil {
				t.Fatal("removing Worker admitted execution")
			}
			want := "removed"
			if crash {
				if err := s.RecoverWorktrees(ctx); err != nil {
					t.Fatal(err)
				}
				detail, err := s.Team(ctx, lead.ID, team.Team.ID)
				if err != nil {
					t.Fatal(err)
				}
				for _, current := range detail.Workers {
					if current.ID == w.ID {
						w = current
					}
				}
				want = "removal_unknown"
			} else {
				w, err = s.FinishWorkerRemoval(ctx, lead.ID, team.Team.ID, w.ID, w.Worktree.Revision, true)
				if err != nil {
					t.Fatal(err)
				}
			}
			if w.Worktree.State != want || w.Lifecycle != hub.WorkerArchived {
				t.Fatal("retirement state lost", w)
			}
			if err := s.SetWorkerLifecycle(ctx, lead.ID, team.Team.ID, w.ID, hub.WorkerActive); err == nil {
				t.Fatal("removed Worker restored into nonexistent cwd")
			}
			if _, err := s.BeginWorkerRemoval(ctx, lead.ID, team.Team.ID, w.ID, w.Worktree.Revision, review.WorkerSHA256); err == nil {
				t.Fatal("removed/unknown mutation replayed")
			}
			detail, err := s.Detail(ctx, w.SessionID)
			if err != nil || detail.Session.Workspace != binding.Path || detail.Branches[0].NativeSessionID != "fixture-native" {
				t.Fatal("native identity/cwd retargeted", detail, err)
			}
			after, err := s.History(ctx, w.SessionID, 0, 128)
			if err != nil || len(after) != len(before) {
				t.Fatal("Worker historical facts changed", err)
			}
		})
	}
}

func TestWorktreeDurableProvisioningAndAdoptionBlockWorkerAndLeadAdmission(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	lead, team, w, _ := teamFixture(t, s)
	if _, err := s.BeginWorkerWorktree(ctx, w.SessionID, team.Team.ID, w.ID); err == nil {
		t.Fatal("foreign Lead began provision")
	}
	w, err := s.BeginWorkerWorktree(ctx, lead.ID, team.Team.ID, w.ID)
	if err != nil {
		t.Fatal(err)
	}
	enqueue(t, s, lead.ID, team.Team.ID, w.ID, "wait for isolation", nil)
	if _, err = s.ClaimWorkerTask(ctx, lead.ID, team.Team.ID, w.ID); err == nil {
		t.Fatal("provisioning Worker ran on old shared workspace")
	}
	binding := hub.WorktreeBinding{Version: 1, WorkerID: w.ID, Workspace: lead.Workspace, Path: filepath.Join(t.TempDir(), "managed"), Branch: "alva/" + w.ID, BaseCommit: "actual-fixture-base"}
	w, err = s.FinishWorkerWorktree(ctx, lead.ID, team.Team.ID, w.ID, w.Worktree.Revision, binding, "")
	if err != nil {
		t.Fatal(err)
	}
	d, err := s.Detail(ctx, w.SessionID)
	if err != nil || d.Session.Workspace != binding.Path {
		t.Fatal("real Worker cwd not changed atomically", err)
	}
	e := hub.WorktreeEvidence{ID: "fixture-diff", WorkerID: w.ID, BaseCommit: binding.BaseCommit, Changes: []hub.WorktreeChange{{Path: "a.txt", AfterSHA256: "fixture-hash"}}}
	w, err = s.SaveWorkerEvidence(ctx, lead.ID, team.Team.ID, w.ID, w.Worktree.Revision, e)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.BeginWorkerAdoption(ctx, lead.ID, team.Team.ID, w.ID, w.Worktree.Revision-1, e.ID); err == nil {
		t.Fatal("stale revision approved")
	}
	w, err = s.BeginWorkerAdoption(ctx, lead.ID, team.Team.ID, w.ID, w.Worktree.Revision, e.ID)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.BeginTurn(ctx, lead.ID, "race adoption"); err == nil {
		t.Fatal("Lead admitted into adopting shared cwd")
	}
	if _, err = s.ClaimWorkerTask(ctx, lead.ID, team.Team.ID, w.ID); err == nil {
		t.Fatal("Worker admitted during adoption")
	}
	w, err = s.FinishWorkerAdoption(ctx, lead.ID, team.Team.ID, w.ID, w.Worktree.Revision, "actual application failure")
	if err != nil || w.Worktree.Adoption != "unknown" {
		t.Fatal("failed apply fabricated success", w, err)
	}
	if _, err = s.BeginWorkerAdoption(ctx, lead.ID, team.Team.ID, w.ID, w.Worktree.Revision, e.ID); err == nil {
		t.Fatal("unknown application replayed")
	}
	if _, err = s.ClaimWorkerTask(ctx, lead.ID, team.Team.ID, w.ID); err == nil {
		t.Fatal("unknown Worker state automatically ran")
	}
	if entries, err := s.History(ctx, w.SessionID, 0, 32); err != nil || len(entries) != 0 {
		t.Fatal("worktree operations fabricated Worker Turn", err)
	}
}
func TestWorktreeRecoveryPreservesPrivateTargetAndUnknownWithoutReplay(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "private")
	ctx := context.Background()
	s, err := Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	lead, team, w, _ := teamFixture(t, s)
	w, err = s.BeginWorkerWorktree(ctx, lead.ID, team.Team.ID, w.ID)
	if err != nil {
		t.Fatal(err)
	}
	if err = s.Close(); err != nil {
		t.Fatal(err)
	}
	s, err = Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	defer s.Close()
	d, err := s.Team(ctx, lead.ID, team.Team.ID)
	if err != nil {
		t.Fatal(err)
	}
	for _, actual := range d.Workers {
		if actual.ID == w.ID {
			if actual.Worktree.State != "provision_unknown" || actual.Worktree.Revision <= w.Worktree.Revision {
				t.Fatal("crash relabeled ready/replayed", actual)
			}
		}
	}
	if _, err = s.BeginWorkerWorktree(ctx, lead.ID, team.Team.ID, w.ID); err == nil {
		t.Fatal("interrupted provision automatically replayed")
	}
}
