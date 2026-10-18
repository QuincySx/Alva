// INPUT: Explicit product schedule actions, pinned configured routes and the existing Hub runtime
// OUTPUT: Owned local interval timer with bounded due indices and no replay, fallback or detached daemon
// POS: [L7 Hub product] Shared desktop/headless product coordinator; constructing/observing it never starts execution
package product

import (
	"context"
	"errors"
	"sync"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
)

type AutomationCoordinator struct {
	Store   hub.AutomationStore
	service *HubService
	mu      sync.Mutex
	gate    chan struct{}
	owner   context.Context
	cancel  context.CancelFunc
	exited  chan struct{}
	closed  bool
	problem bool
}

func newAutomationCoordinator(service *HubService, store hub.AutomationStore) *AutomationCoordinator {
	owner, cancel := context.WithCancel(context.Background())
	return &AutomationCoordinator{Store: store, service: service, gate: make(chan struct{}, 1), owner: owner, cancel: cancel}
}

// Start is an explicit application-owner action, not a side effect of Open,
// settings reads or HTTP observers. A restart first moves idle due times a full
// interval forward in the store; this timer never catches up missed intervals.
func (a *AutomationCoordinator) Start() error {
	a.mu.Lock()
	defer a.mu.Unlock()
	if a.closed {
		return errors.New("automation owner is closed")
	}
	if a.exited != nil {
		return nil
	}
	ctx := a.owner
	a.exited = make(chan struct{})
	go func() {
		defer close(a.exited)
		timer := time.NewTicker(15 * time.Second)
		defer timer.Stop()
		for {
			select {
			case <-ctx.Done():
				return
			case <-timer.C:
				query, stop := context.WithTimeout(ctx, 10*time.Second)
				err := a.Tick(query, time.Now().UTC().UnixMilli())
				stop()
				if err != nil && ctx.Err() == nil {
					a.mu.Lock()
					a.problem = true
					a.mu.Unlock()
				}
			}
		}
	}()
	return nil
}

func (a *AutomationCoordinator) HasError() bool { a.mu.Lock(); defer a.mu.Unlock(); return a.problem }

// Tick consumes only a bounded read-only candidate index. Admission is still
// atomic with the ordinary Turn in the store, and a caller cannot supply the
// prompt, native ID, route or a fabricated delivery outcome.
func (a *AutomationCoordinator) Tick(ctx context.Context, at int64) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	// The application owner's cancellation propagates synchronously to the
	// actual DB claim, not via an asynchronously scheduled AfterFunc window.
	admission, cancel := context.WithCancel(a.owner)
	stop := context.AfterFunc(ctx, cancel)
	defer stop()
	defer cancel()
	ctx = admission
	select {
	case a.gate <- struct{}{}:
		defer func() { <-a.gate }()
	case <-ctx.Done():
		return ctx.Err()
	}
	a.mu.Lock()
	closed := a.closed
	a.mu.Unlock()
	if closed {
		return errors.New("automation owner is closed")
	}
	ticks, err := a.Store.DueAutomations(ctx, at)
	if err != nil {
		return err
	}
	for _, tick := range ticks {
		if err := ctx.Err(); err != nil {
			return err
		}
		config, err := a.Store.Automation(ctx, tick.SessionID, tick.ID)
		if err != nil {
			return err
		}
		if !a.service.configuredAutomationRoute(config.Target) {
			if err := a.Store.PauseInvalidAutomation(ctx, tick); err != nil && !errors.Is(err, hub.ErrAutomationStale) {
				return err
			}
			continue
		}
		// A request timeout/detach is not the execution owner. Runtime.Close
		// remains responsible for admitted ordinary Turns on app shutdown.
		if err := ctx.Err(); err != nil {
			return err
		}
		_, _, err = a.service.Runtime.StartAutomation(context.Background(), ctx, tick, at)
		if errors.Is(err, hub.ErrSessionBusy) {
			err = a.Store.SkipBusyAutomation(ctx, tick, at)
		}
		if err == nil || errors.Is(err, hub.ErrAutomationStale) || errors.Is(err, hub.ErrAutomationSkipped) || errors.Is(err, hub.ErrAutomationRouteChanged) {
			continue
		}
		pauseErr := a.Store.PauseInvalidAutomation(ctx, tick)
		if errors.Is(pauseErr, hub.ErrAutomationStale) {
			pauseErr = nil
		}
		if pauseErr != nil {
			return errors.Join(err, pauseErr)
		}
	}
	return nil
}

// Close stops only the timer/admission seam; it does not synthesize Stop on a
// current Turn. The application closes this owner before its normal Runtime.
func (a *AutomationCoordinator) Close(ctx context.Context) error {
	a.mu.Lock()
	a.closed = true
	if a.cancel != nil {
		a.cancel()
	}
	exited := a.exited
	a.mu.Unlock()
	if exited != nil {
		select {
		case <-exited:
		case <-ctx.Done():
			return ctx.Err()
		}
	}
	select {
	case a.gate <- struct{}{}:
		<-a.gate
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}

func (s *HubService) configuredAutomationRoute(target hub.Target) bool {
	for _, configured := range s.Targets {
		if hub.RouteKey(target) == hub.RouteKey(configured) {
			return true
		}
	}
	return false
}

func (s *HubService) SetAutomationEnabled(ctx context.Context, owner, id string, revision uint64, enabled, approved bool) (hub.Automation, error) {
	if s.Automations == nil {
		return hub.Automation{}, errors.New("durable automation service unavailable")
	}
	if enabled {
		if !approved {
			return hub.Automation{}, errors.New("automation account/billing route requires explicit approval")
		}
		config, err := s.Automations.Store.Automation(ctx, owner, id)
		if err != nil {
			return hub.Automation{}, err
		}
		if !s.configuredAutomationRoute(config.Target) {
			return hub.Automation{}, errors.New("automation target is not an actually configured model/account route")
		}
	}
	return s.Automations.Store.SetAutomationEnabled(ctx, owner, id, revision, enabled, approved)
}
