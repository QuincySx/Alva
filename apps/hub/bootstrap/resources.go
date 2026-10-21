// INPUT: Actual per-attempt native resources, bounded explicit Close and independent physical-cleanup evidence
// OUTPUT: OwnedRunner-compatible lifecycle preserving strong errors and safe application Close retries
// POS: [L7 Hub product] Native resource ownership only; never executes tasks, fabricates terminal status or replays a failed attempt
package bootstrap

import (
	"context"
	"errors"
	workspacefs "github.com/QuincySx/Alva/platform/host/workspace"
	"sync"
	"time"

	"github.com/QuincySx/Alva/harness"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/protocol/skill"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

// Close errors can retain journal failures after a process is reaped. They do
// not substitute for physical worker/group/snapshot cleanup evidence.
type attemptResource interface {
	Close(context.Context) error
	CleanupComplete() bool
}

type ownedRunner struct {
	release func()
	harness.Runner
	workspace         *workspacefs.Workspace
	skills            *skill.FS
	nativeID, ownerID string
	resources         []attemptResource
	closeMu           sync.Mutex
	closed            bool
	closeErr          error
	onClosed          func()
}

func (r *ownedRunner) Run(ctx context.Context, messages []wire.Message, emit func(abi.Event) error) (err error) {
	defer func() { err = errors.Join(err, r.Close(context.WithoutCancel(ctx))) }()
	return r.Runner.Run(ctx, messages, emit)
}
func (r *ownedRunner) Close(ctx context.Context) error {
	r.closeMu.Lock()
	defer r.closeMu.Unlock()
	if r.closed {
		return r.closeErr
	}
	err := closeAttemptResources(ctx, r.nativeID, r.resources)
	if r.closeErr == nil {
		r.closeErr = err
	}
	for _, resource := range r.resources {
		if !resource.CleanupComplete() {
			if r.closeErr == nil {
				r.closeErr = nativeCloseFailure(r.nativeID, "close native resource", errors.New("physical cleanup not complete"))
			}
			return r.closeErr
		}
	}
	if r.skills != nil {
		err = errors.Join(err, nativeCloseFailure(r.nativeID, "close skills", r.skills.Close()))
	}
	if r.workspace != nil {
		err = errors.Join(err, nativeCloseFailure(r.nativeID, "close workspace", r.workspace.Close()))
	}
	if r.release != nil {
		r.release()
		r.release = nil
	}
	r.closed = true
	r.closeErr = errors.Join(r.closeErr, err)
	if r.onClosed != nil {
		r.onClosed()
	}
	return r.closeErr
}
func (r *ownedRunner) CleanupComplete() bool {
	r.closeMu.Lock()
	defer r.closeMu.Unlock()
	return r.closed
}
func closeAttemptResources(ctx context.Context, nativeID string, resources []attemptResource) error {
	if len(resources) == 0 {
		return nil
	}
	cleanup, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	var result error
	for index := len(resources) - 1; index >= 0; index-- {
		result = errors.Join(result, nativeCloseFailure(nativeID, "close native resource", resources[index].Close(cleanup)))
	}
	return result
}
func nativeCloseFailure(nativeID, operation string, err error) error {
	if err == nil {
		return nil
	}
	return &abi.SessionError{Operation: operation, SessionID: nativeID, Err: err}
}

func (r *ownedRunner) WaitCheckpoint() *engine.WaitCheckpoint {
	if source, ok := r.Runner.(engine.WaitingRunner); ok {
		return source.WaitCheckpoint()
	}
	return nil
}
