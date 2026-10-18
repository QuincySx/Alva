// INPUT: Durable TeamStore, existing Hub runtime and explicit product dispatch actions
// OUTPUT: Owned bounded Worker pumps, lifecycle-safe stop and configured-target admission
// POS: [L7 Hub product] Collaboration service reuses full independent Hub execution; UI detach never cancels a Worker
package product

import (
	"context"
	"errors"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/go/apps/hub"
)

type workerPump struct {
	cancel context.CancelFunc
	exited chan struct{}
	kick   chan struct{}
}
type WorkerPumpStatus struct {
	Active bool   `json:"active"`
	Error  string `json:"error,omitempty"`
}
type TeamCoordinator struct {
	Store    hub.TeamStore
	runtime  *hub.Runtime
	mu       sync.Mutex
	pumps    map[string]*workerPump
	problems map[string]string
	stopping map[string]bool
	closed   bool
}

func NewTeamCoordinator(store hub.TeamStore, runtime *hub.Runtime) *TeamCoordinator {
	return &TeamCoordinator{Store: store, runtime: runtime, pumps: make(map[string]*workerPump), problems: make(map[string]string), stopping: make(map[string]bool)}
}
func boundedProblem(err error) string {
	s := err.Error()
	if len(s) > 4096 {
		s = s[:4096]
		for !utf8.ValidString(s) {
			s = s[:len(s)-1]
		}
	}
	return s
}
func (t *TeamCoordinator) Status(worker string) WorkerPumpStatus {
	t.mu.Lock()
	defer t.mu.Unlock()
	return WorkerPumpStatus{Active: t.pumps[worker] != nil, Error: t.problems[worker]}
}

// Resume requires an explicit action after process restart. Queued tasks are
// durable, but merely opening a store or observing a Team never executes them.
func (t *TeamCoordinator) Resume(ctx context.Context, lead, team, worker string) error {
	detail, err := t.Store.Team(ctx, lead, team)
	if err != nil {
		return err
	}
	var found bool
	for _, w := range detail.Workers {
		if w.ID == worker && w.Lifecycle == hub.WorkerActive {
			found = true
		}
	}
	if !found || detail.Team.Status != "active" {
		return hub.ErrWorkerDormant
	}
	t.mu.Lock()
	defer t.mu.Unlock()
	if t.closed {
		return errors.New("Team execution owner is closed")
	}
	if t.stopping[worker] {
		return errors.New("Worker stop is still awaiting its actual owner")
	}
	if p := t.pumps[worker]; p != nil {
		select {
		case p.kick <- struct{}{}:
		default:
		}
		return nil
	}
	if len(t.pumps) >= 32 {
		return errors.New("Team execution pump budget exhausted (32)")
	}
	owner, cancel := context.WithCancel(context.Background())
	pump := &workerPump{cancel: cancel, exited: make(chan struct{}), kick: make(chan struct{}, 1)}
	delete(t.problems, worker)
	t.pumps[worker] = pump
	go func() {
		defer func() {
			cancel()
			t.mu.Lock()
			if t.pumps[worker] == pump {
				delete(t.pumps, worker)
			}
			t.mu.Unlock()
			close(pump.exited)
		}()
		for {
			if owner.Err() != nil {
				return
			}
			_, done, err := t.runtime.StartWorkerTask(owner, lead, team, worker)
			if errors.Is(err, hub.ErrWorkerResourcesBlocked) {
				timer := time.NewTimer(250 * time.Millisecond)
				select {
				case <-owner.Done():
					timer.Stop()
					return
				case <-timer.C:
				}
				continue
			}
			if err != nil {
				// Atomically resolve Resume's kick and remove the drained owner.
				t.mu.Lock()
				select {
				case <-pump.kick:
					t.mu.Unlock()
					continue
				default:
				}
				delete(t.pumps, worker)
				if !errors.Is(err, hub.ErrWorkerQueueEmpty) && !errors.Is(err, hub.ErrWorkerDormant) {
					if len(t.problems) >= 64 {
						for id := range t.problems {
							delete(t.problems, id)
							break
						}
					}
					t.problems[worker] = boundedProblem(err)
				}
				t.mu.Unlock()
				return
			}
			if runErr := <-done; runErr != nil {
				t.mu.Lock()
				if len(t.problems) < 64 {
					t.problems[worker] = boundedProblem(runErr)
				}
				select {
				case <-pump.kick:
					t.mu.Unlock()
					continue
				default:
				}
				delete(t.pumps, worker)
				t.mu.Unlock()
				return
			}
		}
	}()
	return nil
}
func (t *TeamCoordinator) Stop(ctx context.Context, lead, team, worker string) error {
	detail, err := t.Store.Team(ctx, lead, team)
	if err != nil {
		return err
	}
	var sid string
	for _, w := range detail.Workers {
		if w.ID == worker {
			sid = w.SessionID
		}
	}
	if sid == "" {
		return errors.New("Worker is not owned by this Team")
	}
	t.mu.Lock()
	if t.stopping[worker] {
		t.mu.Unlock()
		return errors.New("Worker stop already in progress")
	}
	t.stopping[worker] = true
	defer func() { t.mu.Lock(); delete(t.stopping, worker); t.mu.Unlock() }()
	pump := t.pumps[worker]
	if pump != nil {
		pump.cancel()
	}
	t.mu.Unlock()
	// Cancel only this actual Worker owner. A missing active generation is not
	// a fabricated cancelled outcome; the durable Turn remains authoritative.
	cancelErr := t.runtime.Cancel(ctx, sid)
	if pump != nil {
		select {
		case <-pump.exited:
		case <-ctx.Done():
			return ctx.Err()
		}
	}
	if errors.Is(cancelErr, hub.ErrSessionNotRunning) {
		return nil
	}
	return cancelErr
}
func (t *TeamCoordinator) Close(ctx context.Context) error {
	t.mu.Lock()
	t.closed = true
	pumps := make([]*workerPump, 0, len(t.pumps))
	for _, p := range t.pumps {
		p.cancel()
		pumps = append(pumps, p)
	}
	t.mu.Unlock()
	for _, p := range pumps {
		select {
		case <-p.exited:
		case <-ctx.Done():
			return ctx.Err()
		}
	}
	return nil
}
func (s *HubService) AddWorker(ctx context.Context, lead, team, name string, target hub.Target, accountAndBillingApproved bool) (hub.Worker, error) {
	if s.Teams == nil {
		return hub.Worker{}, errors.New("durable Team service unavailable")
	}
	if !accountAndBillingApproved {
		return hub.Worker{}, errors.New("Worker account/billing route requires explicit approval")
	}
	configured := false
	for _, candidate := range s.Targets {
		if hub.RouteKey(candidate) == hub.RouteKey(target) {
			configured = true
			break
		}
	}
	if !configured {
		return hub.Worker{}, errors.New("Worker target is not one of the actually configured engine routes")
	}
	return s.Teams.Store.AddWorker(ctx, lead, team, name, target)
}
