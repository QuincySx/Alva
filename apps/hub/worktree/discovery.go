// INPUT: Explicit approved Git Manager, canonical repository/worktree paths and fixed read-only Git commands.
// OUTPUT: Canonical common-root repository identity and bounded live worktree listing for explicit imports.
// POS: [L7 Hub product] Discovery never scans ambient directories, reads Worker bindings or mutates Git state.
package worktree

import (
	"bytes"
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"

	"github.com/QuincySx/Alva/apps/hub"
)

type rawWorktree struct {
	path, head, branch string
	detached           bool
	prunable           bool
}

func canonicalDirectory(path string) (string, error) {
	if !filepath.IsAbs(path) || strings.ContainsAny(path, "\x00\r\n") || filepath.Clean(path) != path {
		return "", errors.New("explicit clean absolute directory required")
	}
	canonical, err := filepath.EvalSymlinks(path)
	if err != nil {
		return "", err
	}
	info, err := os.Stat(canonical)
	if err != nil || !info.IsDir() {
		return "", errors.New("developer directory must be an existing directory")
	}
	return canonical, nil
}

func (m *Manager) repositoryToplevel(ctx context.Context, path string) (string, error) {
	canonical, err := canonicalDirectory(path)
	if err != nil {
		return "", err
	}
	data, err := m.run(ctx, canonical, []string{"rev-parse", "--show-toplevel"}, nil, nil, 4096)
	if err != nil {
		// The friendly reason must not replace the real cause: cancellation,
		// deadline and process failures stay inspectable by callers.
		return "", errors.Join(errors.New("explicit import requires an existing Git worktree"), err)
	}
	top, err := canonicalDirectory(strings.TrimSuffix(string(data), "\n"))
	if err != nil || top != canonical {
		return "", errors.New("explicit import accepts only the repository/worktree toplevel, not a subdirectory")
	}
	return canonical, nil
}

func (m *Manager) commonDirectory(ctx context.Context, path string) (string, error) {
	data, err := m.run(ctx, path, []string{"rev-parse", "--path-format=absolute", "--git-common-dir"}, nil, nil, 4096)
	if err != nil {
		return "", err
	}
	return canonicalDirectory(strings.TrimSuffix(string(data), "\n"))
}

func parseWorktreeList(data []byte) ([]rawWorktree, error) {
	var result []rawWorktree
	var current *rawWorktree
	flush := func() {
		if current != nil {
			result = append(result, *current)
			current = nil
		}
	}
	for _, raw := range bytes.Split(data, []byte{0}) {
		field := string(raw)
		if field == "" {
			flush()
			continue
		}
		key, value, _ := strings.Cut(field, " ")
		switch key {
		case "worktree":
			flush()
			current = &rawWorktree{path: value}
		case "HEAD":
			if current == nil {
				return nil, errors.New("Git worktree listing is corrupt")
			}
			current.head = value
		case "branch":
			if current == nil {
				return nil, errors.New("Git worktree listing is corrupt")
			}
			if !strings.HasPrefix(value, "refs/heads/") {
				return nil, errors.New("Git worktree listing contains a non-local branch")
			}
			current.branch = strings.TrimPrefix(value, "refs/heads/")
		case "detached":
			if current == nil {
				return nil, errors.New("Git worktree listing is corrupt")
			}
			current.detached = true
		case "prunable":
			if current == nil {
				return nil, errors.New("Git worktree listing is corrupt")
			}
			current.prunable = true
		case "bare", "locked":
			if current == nil {
				return nil, errors.New("Git worktree listing is corrupt")
			}
		}
		if len(result) > hub.MaxConversationTabs {
			return nil, errors.New("Git worktree listing exceeds its bounded metadata budget")
		}
	}
	flush()
	return result, nil
}

func (m *Manager) InspectDeveloperRepository(ctx context.Context, path string) (value hub.DeveloperRepositoryIdentity, result error) {
	owned, end, err := m.operation(ctx)
	if err != nil {
		return value, err
	}
	defer end()
	requested, err := m.repositoryToplevel(owned, path)
	if err != nil {
		return value, err
	}
	common, err := m.commonDirectory(owned, requested)
	if err != nil {
		return value, err
	}
	data, err := m.run(owned, requested, []string{"worktree", "list", "--porcelain", "-z"}, nil, nil, 1<<20)
	if err != nil {
		return value, err
	}
	worktrees, err := parseWorktreeList(data)
	if err != nil || len(worktrees) == 0 {
		return value, errors.Join(errors.New("Git repository has no worktree listing"), err)
	}
	found := false
	for _, worktree := range worktrees {
		canonical, canonicalErr := canonicalDirectory(worktree.path)
		found = found || canonicalErr == nil && canonical == requested
	}
	if !found || worktrees[0].path == "" || worktrees[0].prunable {
		return value, errors.New("explicit import path is not a live worktree of this Git common root")
	}
	main, err := canonicalDirectory(worktrees[0].path)
	if err != nil {
		return value, err
	}
	mainCommon, err := m.commonDirectory(owned, main)
	if err != nil || mainCommon != common {
		return value, errors.Join(errors.New("Git worktree common-root identity changed"), err)
	}
	return hub.DeveloperRepositoryIdentity{Path: main, CommonDir: common}, nil
}

func (m *Manager) ListDeveloperWorktrees(ctx context.Context, repositoryID, repositoryPath string) (items []hub.DeveloperWorktree, result error) {
	owned, end, err := m.operation(ctx)
	if err != nil {
		return nil, err
	}
	defer end()
	root, err := m.repositoryToplevel(owned, repositoryPath)
	if err != nil || root != repositoryPath {
		return nil, errors.Join(errors.New("saved developer repository root changed"), err)
	}
	data, err := m.run(owned, root, []string{"worktree", "list", "--porcelain", "-z"}, nil, nil, 1<<20)
	if err != nil {
		return nil, err
	}
	worktrees, err := parseWorktreeList(data)
	if err != nil {
		return nil, err
	}
	for index, worktree := range worktrees {
		if worktree.prunable || worktree.path == "" || !validOID(worktree.head) || worktree.branch == "" && !worktree.detached {
			if worktree.prunable {
				continue
			}
			return nil, errors.New("Git worktree listing contains an invalid live identity")
		}
		path, canonicalErr := canonicalDirectory(worktree.path)
		if canonicalErr != nil {
			return nil, canonicalErr
		}
		items = append(items, hub.DeveloperWorktree{
			RepositoryID: repositoryID, Path: path, Name: filepath.Base(path), Head: worktree.head,
			Branch: worktree.branch, Main: index == 0, Detached: worktree.detached,
		})
		if len(items) > hub.MaxConversationTabs {
			return nil, errors.New("Git worktree listing exceeds its bounded metadata budget")
		}
	}
	return items, nil
}

var _ hub.DeveloperWorktreeSource = (*Manager)(nil)
