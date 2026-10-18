// INPUT: context, errors, sort, sync, kernel/abi, llm/wire
// OUTPUT: ToolLockRegistry, NewToolLockRegistry, ToolLockMiddleware
// POS: [L2.5 SDK assembly] SDK-owned cross-Agent resource locks with cancellable fair conflict queues and coordinator bypass.
package core

import (
	"context"
	"errors"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
	"sort"
	"sync"
)

type lockRequest struct {
	id         uint64
	scheduling abi.Scheduling
}
type ToolLockRegistry struct {
	mu              sync.Mutex
	next            uint64
	active, waiting []lockRequest
	changed         chan struct{}
	maxWaiters      int
}

func NewToolLockRegistry(maxWaiters int) (*ToolLockRegistry, error) {
	if maxWaiters < 1 {
		return nil, errors.New("positive lock waiter budget required")
	}
	return &ToolLockRegistry{changed: make(chan struct{}), maxWaiters: maxWaiters}, nil
}
func (registry *ToolLockRegistry) Acquire(ctx context.Context, scheduling abi.Scheduling, workspace string) (func(), error) {
	if err := scheduling.Validate(); err != nil {
		return nil, err
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if scheduling.ExecutionMode == abi.Coordinator {
		return func() {}, nil
	}
	owned := scheduling
	// A shared registry cannot infer reentrancy or resources for an undeclared
	// tool. Serialize it globally; only an explicit Coordinator owns its locks.
	if !owned.ConcurrentSafe {
		owned.ExecutionMode = abi.SerialGlobal
	}
	owned.Resources = append([]abi.ResourceClaim(nil), scheduling.Resources...)
	for i := range owned.Resources {
		owned.Resources[i].Resource = abi.NormalizeResource(owned.Resources[i].Resource, workspace)
	}
	sort.Slice(owned.Resources, func(i, j int) bool { return owned.Resources[i].Resource < owned.Resources[j].Resource })
	registry.mu.Lock()
	if len(registry.waiting) >= registry.maxWaiters {
		registry.mu.Unlock()
		return nil, errors.New("resource lock waiter budget exceeded")
	}
	registry.next++
	request := lockRequest{id: registry.next, scheduling: owned}
	registry.waiting = append(registry.waiting, request)
	for {
		if err := ctx.Err(); err != nil {
			registry.removeWaiting(request.id)
			registry.notify()
			registry.mu.Unlock()
			return nil, err
		}
		if registry.canAcquire(request) {
			registry.removeWaiting(request.id)
			registry.active = append(registry.active, request)
			registry.mu.Unlock()
			var once sync.Once
			return func() {
				once.Do(func() {
					registry.mu.Lock()
					defer registry.mu.Unlock()
					for i, active := range registry.active {
						if active.id == request.id {
							registry.active = append(registry.active[:i], registry.active[i+1:]...)
							break
						}
					}
					registry.notify()
				})
			}, nil
		}
		changed := registry.changed
		registry.mu.Unlock()
		select {
		case <-ctx.Done():
		case <-changed:
		}
		registry.mu.Lock()
	}
}
func (registry *ToolLockRegistry) canAcquire(request lockRequest) bool {
	for _, active := range registry.active {
		if abi.SchedulingConflicts(active.scheduling, request.scheduling, "") {
			return false
		}
	}
	for _, prior := range registry.waiting {
		if prior.id == request.id {
			break
		}
		if abi.SchedulingConflicts(prior.scheduling, request.scheduling, "") {
			return false
		}
	}
	return true
}
func (registry *ToolLockRegistry) removeWaiting(id uint64) {
	for i, request := range registry.waiting {
		if request.id == id {
			registry.waiting = append(registry.waiting[:i], registry.waiting[i+1:]...)
			return
		}
	}
}
func (registry *ToolLockRegistry) notify() {
	close(registry.changed)
	registry.changed = make(chan struct{})
}

type ToolLockMiddleware struct{ Registry *ToolLockRegistry }

func (ToolLockMiddleware) Name() string { return "tool-lock" }
func (middleware ToolLockMiddleware) WrapTool(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall, next abi.ToolFunc) (wire.ToolResult, error) {
	if middleware.Registry == nil {
		return wire.ToolResult{}, errors.New("resource lock registry unavailable")
	}
	release, err := middleware.Registry.Acquire(ctx, execution.Scheduling, execution.Workspace)
	if err != nil {
		return wire.ToolResult{}, err
	}
	defer release()
	return next(ctx, execution, call)
}
