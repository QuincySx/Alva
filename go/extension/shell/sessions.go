// INPUT: Trusted per-attempt shell configuration, actual native session authority and explicit application lifetime
// OUTPUT: Sessions registry, per-attempt Lease, scoped job status/Stop and bounded interruption recovery
// POS: [L4 extension] Explicit product resource owner keeps successful background jobs across turns; no daemon/PID adoption or replay.
package shell

import (
	"context"
	"encoding/json"
	"errors"
	"maps"
	"sort"
	"sync"
	"time"

	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
)

type sessionKey struct{ owner, native string }
type Sessions struct {
	mu      sync.Mutex
	closed  bool
	entries map[sessionKey]*Plugin
}

// Lease owns only an attempt's reservation. Its Plugin's background processes
// already belong to Sessions, which MUST be closed before the fact repository.
// Releasing an observer/attempt is therefore not physical process shutdown.
type Lease struct {
	*Plugin
	generation uint64
}
type JobStatus struct {
	RunID  string `json:"origin_run_id"`
	Status Status `json:"status"`
	Frames int    `json:"output_frames"`
}

func detachedStatus(status Status) Status {
	if status.Task.StartedAt != nil {
		started := *status.Task.StartedAt
		status.Task.StartedAt = &started
	}
	if status.Task.EndedAt != nil {
		ended := *status.Task.EndedAt
		status.Task.EndedAt = &ended
	}
	if status.ExitCode != nil {
		code := *status.ExitCode
		status.ExitCode = &code
	}
	return status
}

func NewSessions() *Sessions { return &Sessions{entries: map[sessionKey]*Plugin{}} }

func (l *Lease) Register(_ context.Context, r *core.Registrar) error {
	l.mu.Lock()
	valid := l.generation == l.Plugin.generation && l.reserved
	l.mu.Unlock()
	if !valid {
		return errors.New("shell attempt lease expired before registration")
	}
	return l.Plugin.register(r, l)
}
func (l *Lease) OnRunStart(ctx context.Context, r abi.RuntimeContext) error {
	l.mu.Lock()
	defer l.mu.Unlock()
	if l.generation != l.Plugin.generation || !l.reserved {
		return errors.New("shell attempt lease expired before execution")
	}
	return l.Plugin.startLocked(ctx, r)
}
func (l *Lease) OnRunEnd(ctx context.Context, r abi.RuntimeContext, err error) error {
	return l.Plugin.endRun(ctx, r, err, l.generation)
}

func sameRecipe(p *Plugin, c Config) bool {
	return p.config.Repository == c.Repository && p.config.Owner == c.Owner && p.config.SessionID == c.SessionID && p.config.Cwd == c.Cwd && p.config.Executable == c.Executable && p.config.ApprovedHostAccess == c.ApprovedHostAccess && p.config.Locks == c.Locks && maps.Equal(p.config.Env, c.Env)
}

// Bind freezes executable/env/cwd/authority for the lifetime of live jobs. Guard
// and prior-activity reporter are rebound only while no other attempt owns it.
func (s *Sessions) Bind(ctx context.Context, c Config) (*Lease, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed {
		return nil, errors.New("application shell owner is closed")
	}
	key := sessionKey{c.Owner, c.SessionID}
	p := s.entries[key]
	if p != nil {
		p.mu.Lock()
		if p.reserved || p.runID != "" {
			p.mu.Unlock()
			return nil, errors.New("native shell session already reserved by an actual attempt")
		}
		changed := !sameRecipe(p, c) || p.closed || p.life.Err() != nil
		clean := p.completeLocked()
		p.mu.Unlock()
		if changed {
			if !clean {
				return nil, errors.New("previous shell recipe still owns jobs/cleanup; explicitly Stop those jobs before changing its executable/environment")
			}
			if err := p.Close(ctx); err != nil {
				return nil, err
			}
			delete(s.entries, key)
			p = nil
		}
	}
	if p == nil {
		if len(s.entries) >= 64 {
			return nil, errors.New("64 native shell session owners reached; explicitly reset inactive owners")
		}
		var err error
		p, err = New(ctx, c)
		if err != nil {
			return nil, err
		}
		p.persistent = true
		p.life, p.cancel = context.WithCancel(context.Background())
		if err := p.recoverInterrupted(ctx); err != nil {
			p.cancel()
			return nil, err
		}
		s.entries[key] = p
	}
	p.mu.Lock()
	p.config.Guard, p.config.Report = c.Guard, c.Report
	p.config.RetainWorkspace = c.RetainWorkspace
	p.reserved = true
	p.generation++
	lease := &Lease{Plugin: p, generation: p.generation}
	p.mu.Unlock()
	return lease, nil
}

func (l *Lease) Close(ctx context.Context) error {
	p := l.Plugin
	p.mu.Lock()
	if p.generation != l.generation || !p.reserved {
		p.mu.Unlock()
		return nil
	}
	run := p.runID
	p.mu.Unlock()
	var err error
	if run != "" {
		err = p.endRun(ctx, abi.RuntimeContext{RunID: run}, errors.New("attempt ended before shell lifecycle finalized"), l.generation)
	}
	p.mu.Lock()
	if p.generation == l.generation && p.runID == "" {
		p.reserved = false
	}
	p.mu.Unlock()
	return err
}
func (l *Lease) CleanupComplete() bool {
	l.mu.Lock()
	defer l.mu.Unlock()
	return l.generation != l.Plugin.generation || !l.reserved
}

func (s *Sessions) List(owner, native string) []JobStatus {
	s.mu.Lock()
	p := s.entries[sessionKey{owner, native}]
	s.mu.Unlock()
	result := []JobStatus{}
	if p == nil {
		return result
	}
	p.mu.Lock()
	defer p.mu.Unlock()
	for _, j := range p.jobs {
		j.mu.Lock()
		status := j.status
		status.Task = j.task
		result = append(result, JobStatus{RunID: j.runID, Status: detachedStatus(status), Frames: len(j.frames)})
		j.mu.Unlock()
	}
	sort.Slice(result, func(i, j int) bool { return result[i].Status.Task.ID < result[j].Status.Task.ID })
	return result
}

// Stop resolves only an already-owned opaque job. It never looks up, signals
// or adopts a PID provided by the caller or restored from disk.
func (s *Sessions) Stop(ctx context.Context, owner, native, id string) error {
	s.mu.Lock()
	p := s.entries[sessionKey{owner, native}]
	s.mu.Unlock()
	if p == nil {
		return errors.New("owned shell session unavailable")
	}
	p.mu.Lock()
	j := p.jobs[id]
	p.mu.Unlock()
	if j == nil {
		return errors.New("job is not owned by this native session")
	}
	if err := p.append(ctx, "operator_stop_intent", map[string]any{"job_id": id, "origin_run_id": j.runID, "notice": "Explicit operator Stop of an already-owned job; no PID adoption or replay"}); err != nil {
		return err
	}
	j.cancel()
	j.process.BeginClose()
	select {
	case <-j.done:
	case <-ctx.Done():
		return ctx.Err()
	}
	select {
	case <-j.leaseDone:
	case <-ctx.Done():
		return ctx.Err()
	}
	j.mu.Lock()
	err := j.err
	j.mu.Unlock()
	if !j.process.CleanupComplete() {
		return errors.Join(err, errors.New("owned shell cleanup remains incomplete"))
	}
	if abi.OnlyCancellation(err) {
		return nil
	}
	return err
}

// Reset is explicit Stop-all and retirement of this in-memory owner. Durable
// facts/artifacts remain; the next enabled Turn may construct a fresh recipe.
func (s *Sessions) Reset(ctx context.Context, owner, native string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	key := sessionKey{owner, native}
	p := s.entries[key]
	if p == nil {
		return nil
	}
	p.mu.Lock()
	reserved := p.reserved
	p.mu.Unlock()
	if reserved {
		return errors.New("finish or Stop the actual Turn before resetting its shell owner")
	}
	err := p.Close(ctx)
	if p.CleanupComplete() {
		delete(s.entries, key)
	}
	return err
}
func (s *Sessions) Close(ctx context.Context) error {
	s.mu.Lock()
	s.closed = true
	entries := make([]*Plugin, 0, len(s.entries))
	for _, p := range s.entries {
		entries = append(entries, p)
	}
	s.mu.Unlock()
	// Cancel every owner before waiting for one so a deadline never leaves the
	// remaining live jobs without a shutdown request.
	for _, p := range entries {
		p.mu.Lock()
		if p.cancel != nil {
			p.cancel()
		}
		p.mu.Unlock()
	}
	var err error
	for _, p := range entries {
		err = errors.Join(err, p.Close(ctx))
	}
	return err
}
func (s *Sessions) CleanupComplete() bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	for _, p := range s.entries {
		if !p.CleanupComplete() {
			return false
		}
	}
	return true
}

func (p *Plugin) recoverInterrupted(ctx context.Context) error {
	records, err := p.log.Records(ctx, 0)
	if err != nil {
		return err
	}
	pending := map[string]bool{}
	for _, record := range records {
		if record.Kind != abi.RecordExtension || record.Namespace != "alva.shell" {
			continue
		}
		var value struct {
			Task struct {
				ID string `json:"id"`
			} `json:"task"`
			Status json.RawMessage `json:"status"`
			JobID  string          `json:"job_id"`
		}
		if json.Unmarshal(record.Payload, &value) != nil {
			return errors.New("invalid saved shell ownership fact")
		}
		switch record.Type {
		case "job_intent":
			if value.Task.ID != "" {
				pending[value.Task.ID] = true
			}
		case "job_outcome":
			var status struct {
				Task struct {
					ID string `json:"id"`
				} `json:"task"`
			}
			if json.Unmarshal(value.Status, &status) != nil {
				return errors.New("invalid saved shell outcome")
			}
			delete(pending, status.Task.ID)
		case "job_start_failed", "job_abandoned":
			delete(pending, value.JobID)
		}
	}
	if len(pending) > 64 {
		return errors.New("interrupted shell ownership recovery exceeds64 jobs; no job was restarted")
	}
	ids := make([]string, 0, len(pending))
	for id := range pending {
		ids = append(ids, id)
	}
	sort.Strings(ids)
	p.recoveredUnknown = append([]string(nil), ids...)
	for _, id := range ids {
		if err := p.append(ctx, "job_abandoned", map[string]any{"job_id": id, "status": "unknown", "at": time.Now().UTC(), "notice": "Previous process owner ended without a completed outcome; execution and side effects unknown. No PID adoption, signals, restart or replay."}); err != nil {
			return err
		}
	}
	return nil
}
