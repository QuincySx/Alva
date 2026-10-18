// INPUT: Exact product-private Worker binding, explicit whole-tree removal review and fixed approved Git executor
// OUTPUT: Bounded removal review and actual managed worktree removal with identity/content revalidation
// POS: [L7 Hub product] Owner must hold both workspace leases and durable removing state; branches/history/evidence are retained.
package worktree

import (
	"bytes"
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/QuincySx/Alva/go/apps/hub"
)

// Removal is deliberately independent of the saved base commit still being
// HEAD. A human can retire an old Worker after the shared branch has advanced.
// The exact registered path/branch/common repository must still match.
func (m *Manager) removalIdentity(ctx context.Context, h hub.WorktreeBinding) (string, error) {
	if h.Version != 1 || !validWorker(h.WorkerID) || h.Path != filepath.Join(m.config.Root, "trees", h.WorkerID) || h.Branch != "alva/"+h.WorkerID || !validOID(h.BaseCommit) {
		return "", errors.New("exact managed Worker binding required for removal")
	}
	for _, name := range []string{h.Path, h.Workspace} {
		info, err := os.Lstat(name)
		canonical, pathErr := filepath.EvalSymlinks(name)
		if err != nil || pathErr != nil || !info.IsDir() || info.Mode()&os.ModeSymlink != 0 || canonical != name {
			return "", errors.New("managed/shared workspace identity changed")
		}
	}
	var common string
	for _, name := range []string{h.Workspace, h.Path} {
		top, err := m.run(ctx, name, []string{"rev-parse", "--show-toplevel"}, nil, nil, 4096)
		if err != nil || strings.TrimSuffix(string(top), "\n") != name {
			return "", errors.New("removal requires the actual shared and managed repository roots")
		}
		data, err := m.run(ctx, name, []string{"rev-parse", "--path-format=absolute", "--git-common-dir"}, nil, nil, 4096)
		if err != nil {
			return "", err
		}
		actual, err := filepath.EvalSymlinks(strings.TrimSuffix(string(data), "\n"))
		if err != nil || common != "" && common != actual {
			return "", errors.New("managed Worker belongs to a different Git repository")
		}
		common = actual
	}
	data, err := m.run(ctx, h.Workspace, []string{"worktree", "list", "--porcelain", "-z"}, nil, nil, 1<<20)
	if err != nil {
		return "", err
	}
	found, selected := false, false
	head, branch := "", ""
	for _, field := range bytes.Split(data, []byte{0}) {
		value := string(field)
		if strings.HasPrefix(value, "worktree ") {
			selected = strings.TrimPrefix(value, "worktree ") == h.Path
			if selected {
				if found {
					return "", errors.New("duplicate managed Git registration")
				}
				found = true
			}
		} else if selected && strings.HasPrefix(value, "HEAD ") {
			head = strings.TrimPrefix(value, "HEAD ")
		} else if selected && strings.HasPrefix(value, "branch ") {
			branch = strings.TrimPrefix(value, "branch ")
		}
	}
	if !found || branch != "refs/heads/"+h.Branch || !validOID(head) {
		return "", errors.New("exact managed Git registration/branch missing or changed")
	}
	return head, nil
}

func (m *Manager) removalReview(ctx context.Context, h hub.WorktreeBinding) (hub.WorktreeRemovalReview, error) {
	var review hub.WorktreeRemovalReview
	head, err := m.removalIdentity(ctx, h)
	if err != nil {
		return review, err
	}
	snapshot, err := m.currentSnapshot(ctx, h.Path, nil)
	if err != nil {
		return review, err
	}
	index, err := m.run(ctx, h.Path, []string{"ls-files", "--stage", "-z"}, nil, nil, 1<<20)
	if err != nil {
		return review, err
	}
	review = hub.WorktreeRemovalReview{WorkerSHA256: snapshot.digest(), IndexSHA256: sha(index), Head: head, Files: len(snapshot), CreatedAt: time.Now().UTC().UnixMilli()}
	for _, state := range snapshot {
		review.Bytes += len(state.Data)
	}
	return review, nil
}

func (m *Manager) ReviewRemoval(ctx context.Context, h hub.WorktreeBinding) (hub.WorktreeRemovalReview, error) {
	owned, end, err := m.operation(ctx)
	if err != nil {
		return hub.WorktreeRemovalReview{}, err
	}
	defer end()
	return m.removalReview(owned, h)
}

// Remove never deletes the branch or private evidence. A failed Git command or
// postcondition is unknown; callers must not retry it automatically.
func (m *Manager) Remove(ctx context.Context, h hub.WorktreeBinding, reviewed hub.WorktreeRemovalReview) error {
	owned, end, err := m.operation(ctx)
	if err != nil {
		return err
	}
	defer end()
	actual, err := m.removalReview(owned, h)
	if err != nil {
		return err
	}
	if reviewed.CreatedAt <= 0 || actual.WorkerSHA256 != reviewed.WorkerSHA256 || actual.IndexSHA256 != reviewed.IndexSHA256 || actual.Head != reviewed.Head || actual.Files != reviewed.Files || actual.Bytes != reviewed.Bytes {
		return errors.New("managed Worker changed after removal review; generate a new review")
	}
	if _, err = m.run(owned, h.Workspace, []string{"worktree", "remove", "--force", "--", h.Path}, nil, nil, 64<<10); err != nil {
		return err
	}
	if _, err = os.Lstat(h.Path); !errors.Is(err, os.ErrNotExist) {
		return errors.New("managed worktree removal did not prove target absence")
	}
	data, err := m.run(owned, h.Workspace, []string{"worktree", "list", "--porcelain", "-z"}, nil, nil, 1<<20)
	if err != nil || bytes.Contains(data, []byte("worktree "+h.Path+"\x00")) {
		return errors.Join(errors.New("managed Git registration removal was not proven"), err)
	}
	return nil
}
