// INPUT: context, errors and sync.
// OUTPUT: Native child setup/resource cleanup with independent physical completion evidence.
// POS: [L5 harness] Retains each exact unfinished resource without repeating successfully completed cleanup.
package harness

import (
	"context"
	"errors"
	"sync"
)

type childResource struct {
	close    func(context.Context) error
	complete func() bool
	done     bool
}
type childResources struct {
	mu    sync.Mutex
	items []childResource
}

func (r *childResources) add(close func(context.Context) error, complete func() bool) {
	r.items = append(r.items, childResource{close: close, complete: complete})
}
func (r *childResources) Close(ctx context.Context) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	var err error
	for i := len(r.items) - 1; i >= 0; i-- {
		item := &r.items[i]
		if item.done {
			continue
		}
		next := item.close(ctx)
		if item.complete != nil {
			item.done = item.complete()
		} else {
			item.done = next == nil
		}
		if !item.done && next == nil {
			next = errors.New("native child resource cleanup remains incomplete")
		}
		err = errors.Join(err, next)
	}
	return err
}
func (r *childResources) CleanupComplete() bool {
	r.mu.Lock()
	defer r.mu.Unlock()
	for _, item := range r.items {
		if !item.done {
			return false
		}
	}
	return true
}
