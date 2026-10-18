// INPUT: Canonical saved workspace, trusted engine ToolControl capability and cancellation
// OUTPUT: Fair bounded product workspace admission with conservative external-engine exclusion
// POS: [L7 Hub product] Cross-engine ownership only; actual Alva per-tool resource locks remain the SDK's responsibility
package hub

import (
	"context"
	"errors"
	"path/filepath"
	"sync"
)

type workspaceLease struct {
	id        uint64
	path      string
	exclusive bool
}
type workspaceRegistry struct {
	mu              sync.Mutex
	next            uint64
	active, waiting []workspaceLease
	changed         chan struct{}
}

func newWorkspaceRegistry() *workspaceRegistry {
	return &workspaceRegistry{changed: make(chan struct{})}
}
func overlaps(a, b string) bool {
	relative, err := filepath.Rel(a, b)
	if err == nil && (relative == "." || filepath.IsLocal(relative)) {
		return true
	}
	relative, err = filepath.Rel(b, a)
	return err == nil && (relative == "." || filepath.IsLocal(relative))
}
func conflicts(a, b workspaceLease) bool {
	return (a.exclusive || b.exclusive) && overlaps(a.path, b.path)
}
func (r *workspaceRegistry) notify() { close(r.changed); r.changed = make(chan struct{}) }
func (r *workspaceRegistry) remove(id uint64) {
	for i, item := range r.waiting {
		if item.id == id {
			r.waiting = append(r.waiting[:i], r.waiting[i+1:]...)
			return
		}
	}
}
func (r *workspaceRegistry) acquire(ctx context.Context, path string, exclusive bool) (func(), error) {
	if !filepath.IsAbs(path) {
		return nil, errors.New("canonical absolute workspace required")
	}
	r.mu.Lock()
	if len(r.waiting) >= 128 {
		r.mu.Unlock()
		return nil, errors.New("workspace admission waiter budget exhausted")
	}
	r.next++
	request := workspaceLease{id: r.next, path: filepath.Clean(path), exclusive: exclusive}
	r.waiting = append(r.waiting, request)
	for {
		if err := ctx.Err(); err != nil {
			r.remove(request.id)
			r.notify()
			r.mu.Unlock()
			return nil, err
		}
		allowed := true
		for _, item := range r.active {
			if conflicts(item, request) {
				allowed = false
				break
			}
		}
		if allowed {
			for _, prior := range r.waiting {
				if prior.id == request.id {
					break
				}
				if conflicts(prior, request) {
					allowed = false
					break
				}
			}
		}
		if allowed {
			r.remove(request.id)
			r.active = append(r.active, request)
			r.mu.Unlock()
			var once sync.Once
			return func() {
				once.Do(func() {
					r.mu.Lock()
					defer r.mu.Unlock()
					for i, item := range r.active {
						if item.id == request.id {
							r.active = append(r.active[:i], r.active[i+1:]...)
							r.notify()
							return
						}
					}
				})
			}, nil
		}
		changed := r.changed
		r.mu.Unlock()
		select {
		case <-ctx.Done():
		case <-changed:
		}
		r.mu.Lock()
	}
}

// A running native attempt may extend its already-admitted shared lease. This
// does not enqueue a second reader behind a waiting writer (which would deadlock
// the writer against the still-running attempt). Sealing rejects late retain;
// the underlying lease remains active until every physical resource releases.
func retainableWorkspace(release func()) (seal func(), retain func() (func(), error)) {
	var mu sync.Mutex
	open, refs, count := true, 1, 0
	drop := func() {
		mu.Lock()
		refs--
		last := refs == 0
		mu.Unlock()
		if last {
			release()
		}
	}
	var sealOnce sync.Once
	seal = func() { sealOnce.Do(func() { mu.Lock(); open = false; mu.Unlock(); drop() }) }
	retain = func() (func(), error) {
		mu.Lock()
		if !open || count >= 64 {
			mu.Unlock()
			return nil, errors.New("workspace retention closed or64 resource budget exceeded")
		}
		count++
		refs++
		mu.Unlock()
		var once sync.Once
		return func() { once.Do(drop) }, nil
	}
	return seal, retain
}
