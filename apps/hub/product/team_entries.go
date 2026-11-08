// INPUT: context, hub TeamStore owned by the TeamCoordinator
// OUTPUT: TeamCoordinator entry methods for Team reads, queue edits and lifecycle changes
// POS: [L7 Hub product] The one Team entry desktop, HTTP and CLI share, so no entry reaches the Team store directly
package product

import (
	"context"

	"github.com/QuincySx/Alva/apps/hub"
)

func (t *TeamCoordinator) TeamForLead(ctx context.Context, lead string) (hub.TeamDetail, error) {
	return t.Store.TeamForLead(ctx, lead)
}

func (t *TeamCoordinator) Team(ctx context.Context, lead, team string) (hub.TeamDetail, error) {
	return t.Store.Team(ctx, lead, team)
}

func (t *TeamCoordinator) CreateTeam(ctx context.Context, lead string) (hub.TeamDetail, error) {
	return t.Store.CreateTeam(ctx, lead)
}

func (t *TeamCoordinator) EndTeam(ctx context.Context, lead, team string) error {
	return t.Store.EndTeam(ctx, lead, team)
}

func (t *TeamCoordinator) SetWorkerLifecycle(ctx context.Context, lead, team, worker string, state hub.WorkerLifecycle) error {
	return t.Store.SetWorkerLifecycle(ctx, lead, team, worker, state)
}

func (t *TeamCoordinator) EnqueueWorker(ctx context.Context, lead, team, worker, scope string, claims []hub.ResourceClaim) (hub.WorkerTask, error) {
	return t.Store.EnqueueWorker(ctx, lead, team, worker, scope, claims)
}

func (t *TeamCoordinator) EditQueuedTask(ctx context.Context, lead, team, worker, task, scope string, claims []hub.ResourceClaim) error {
	return t.Store.EditQueuedTask(ctx, lead, team, worker, task, scope, claims)
}

func (t *TeamCoordinator) CancelQueuedTask(ctx context.Context, lead, team, worker, task string) error {
	return t.Store.CancelQueuedTask(ctx, lead, team, worker, task)
}

func (t *TeamCoordinator) WorkerTasks(ctx context.Context, lead, team, worker string, after uint64, limit int) (hub.WorkerTaskPage, error) {
	return t.Store.WorkerTasks(ctx, lead, team, worker, after, limit)
}
