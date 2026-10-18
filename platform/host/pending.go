// INPUT: context, errors, app PendingService, kernel/bus.
// OUTPUT: Controller.Steer and Controller.PendingMessages.
// POS: [platform / native execution host] Explicit root-run admission for optional harness steering without observer-owned execution.
package host

import (
	"context"
	"errors"
	"github.com/QuincySx/Alva/harness"
	"github.com/QuincySx/Alva/sdk/kernel/bus"
)

var ErrPendingUnavailable = errors.New("runner has no pending message plugin")

func (c *Controller) pendingLocked(id string) (*harness.PendingService, error) {
	if c.closed {
		return nil, ErrControllerClosed
	}
	state := c.runs[id]
	if state == nil {
		return nil, ErrRunNotFound
	}
	if c.active != id || state.snapshot.Status != RunRunning {
		return nil, harness.ErrPendingInactive
	}
	provider, ok := c.runner.(interface{ Bus() bus.Handle })
	if !ok {
		return nil, ErrPendingUnavailable
	}
	service, ok := bus.Resolve[*harness.PendingService](provider.Bus())
	if !ok {
		return nil, ErrPendingUnavailable
	}
	return service, nil
}
func (c *Controller) Steer(ctx context.Context, id, text string) (harness.PendingMessage, error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	service, err := c.pendingLocked(id)
	if err != nil {
		return harness.PendingMessage{}, err
	}
	return service.Enqueue(ctx, text)
}
func (c *Controller) PendingMessages(id string) ([]harness.PendingMessage, error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	service, err := c.pendingLocked(id)
	if err != nil {
		return nil, err
	}
	return service.Snapshot(false), nil
}
