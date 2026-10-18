// INPUT: Trusted actual workspace paths, explicit product callback and Runtime/application shutdown
// OUTPUT: Bounded exclusive multi-workspace activity reusing execution leases and owned Close joining
// POS: [L7 Hub product] Product mutation coordination only; not an SDK tool grant or an OS filesystem transaction
package hub

import (
	"context"
	"errors"
	"path/filepath"
	"slices"
)

type workspaceAction struct {
	cancel context.CancelFunc
	exited chan struct{}
}

func (r *Runtime) WithExclusiveWorkspaces(ctx context.Context, paths []string, apply func(context.Context) error) error {
	if len(paths) < 1 || len(paths) > 2 || apply == nil {
		return errors.New("one or two actual workspaces and explicit callback required")
	}
	paths = append([]string(nil), paths...)
	slices.Sort(paths)
	paths = slices.Compact(paths)
	for _, path := range paths {
		if !filepath.IsAbs(path) || filepath.Clean(path) != path {
			return errors.New("canonical absolute workspace required")
		}
	}
	filtered := paths[:0]
	for _, path := range paths {
		if len(filtered) > 0 && overlaps(filtered[len(filtered)-1], path) {
			continue
		}
		filtered = append(filtered, path)
	}
	paths = filtered
	for _, path := range paths {
		if !filepath.IsAbs(path) || filepath.Clean(path) != path {
			return errors.New("canonical absolute workspace required")
		}
	}
	r.mu.Lock()
	if r.closed || len(r.actions) >= 8 {
		r.mu.Unlock()
		return errors.New("Runtime mutation owner closed or activity budget exceeded")
	}
	owner, cancel := context.WithCancel(ctx)
	action := &workspaceAction{cancel: cancel, exited: make(chan struct{})}
	if r.actions == nil {
		r.actions = map[*workspaceAction]struct{}{}
	}
	r.actions[action] = struct{}{}
	r.mu.Unlock()
	defer func() { cancel(); r.mu.Lock(); delete(r.actions, action); close(action.exited); r.mu.Unlock() }()
	for _, path := range paths {
		release, err := r.workspaces.acquire(owner, path, true)
		if err != nil {
			return err
		}
		defer release()
	}
	if err := owner.Err(); err != nil {
		return err
	}
	return apply(owner)
}
