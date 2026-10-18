// INPUT:  context, errors, time, host.Controller
// OUTPUT: bounded snapshot-reconciling presentation observer
// POS: [L6 application]    Framework-independent subscription lifecycle that never owns execution
package desktop

import (
	"context"
	"errors"
	"time"

	"github.com/QuincySx/Alva/platform/host"
)

// observe owns only its subscription. Cancellation/detachment never calls Stop.
// When ephemeral replay expires or a slow subscriber closes, resnapshot before
// subscribing again; no provider/tool operation is replayed.
func observe(ctx context.Context, controller *host.Controller, id string, update func(host.RunSnapshot), failed func(error)) {
	var after uint64
	for {
		if ctx.Err() != nil {
			return
		}
		stream, unsubscribe, err := controller.Subscribe(id, after)
		if err != nil && !errors.Is(err, host.ErrHistoryExpired) {
			failed(err)
			observeSnapshots(ctx, controller, id, update, failed)
			return
		}
		if err == nil {
			for {
				select {
				case <-ctx.Done():
					unsubscribe()
					return
				case _, ok := <-stream:
					if !ok {
						unsubscribe()
						goto refresh
					}
					snapshot, readErr := controller.Snapshot(id)
					if readErr != nil {
						unsubscribe()
						failed(readErr)
						return
					}
					after = snapshot.HighWater
					update(snapshot)
					if snapshot.Status != host.RunRunning {
						unsubscribe()
						return
					}
				}
			}
		}
	refresh:
		snapshot, readErr := controller.Snapshot(id)
		if readErr != nil {
			failed(readErr)
			return
		}
		after = snapshot.HighWater
		update(snapshot)
		if snapshot.Status != host.RunRunning {
			return
		}
	}
}

// Subscription capacity is observation state, not execution state. Fall back to
// bounded-rate snapshots so an admitted run can still acknowledge Stop or reach
// its real terminal outcome. No extra observer slot or execution is allocated.
func observeSnapshots(ctx context.Context, controller *host.Controller, id string, update func(host.RunSnapshot), failed func(error)) {
	ticker := time.NewTicker(250 * time.Millisecond)
	defer ticker.Stop()
	for {
		if ctx.Err() != nil {
			return
		}
		snapshot, err := controller.Snapshot(id)
		if err != nil {
			failed(err)
			return
		}
		update(snapshot)
		if snapshot.Status != host.RunRunning {
			return
		}
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
		}
	}
}
