// INPUT: Actual Hub runtime owner, TeamStore queue and persisted Worker membership
// OUTPUT: StartWorkerTask atomically consuming one FIFO task as a normal owned Hub Turn
// POS: [L7 Hub product] Reuses the existing engine supervisor, fallback, cancellation and workspace safety runtime
package hub

import (
	"context"
	"errors"
)

var ErrWorkerDormant = errors.New("Worker is dormant or Team has ended")
var ErrWorkerQueueEmpty = errors.New("Worker queue is empty")
var ErrWorkerResourcesBlocked = errors.New("Worker head is waiting for older or running resource claims")

func (r *Runtime) StartWorkerTask(owner context.Context, lead, team, workerID string) (WorkerTask, <-chan error, error) {
	store, ok := r.store.(TeamStore)
	if !ok {
		return WorkerTask{}, nil, errors.New("product store has no durable teams")
	}
	detail, err := store.Team(owner, lead, team)
	if err != nil {
		return WorkerTask{}, nil, err
	}
	var sessionID string
	for _, w := range detail.Workers {
		if w.ID == workerID {
			sessionID = w.SessionID
			break
		}
	}
	if sessionID == "" {
		return WorkerTask{}, nil, errors.New("Worker is not owned by this Team")
	}
	var task WorkerTask
	_, done, err := r.start(owner, sessionID, func(ctx context.Context) (Claim, error) {
		claimed, err := store.ClaimWorkerTask(ctx, lead, team, workerID)
		if err != nil {
			return Claim{}, err
		}
		task = claimed.Task
		if claimed.Dormant {
			return Claim{}, ErrWorkerDormant
		}
		if len(claimed.BlockedBy) > 0 {
			return Claim{}, ErrWorkerResourcesBlocked
		}
		if claimed.Claim == nil {
			return Claim{}, ErrWorkerQueueEmpty
		}
		return *claimed.Claim, nil
	})
	return task, done, err
}
