// INPUT: context, encoding/json, errors, filepath, sync/time, core/security, ABI/bus/wire, native/process, storage/session.
// OUTPUT: Explicit host-approved shell Config, Activity and run-owned Plugin with Close/CleanupComplete and exact Tools.
// POS: [L4 extension] Optional host shell owns every process and global lease; no implicit shell access or OS sandbox claim.
package shell

import (
	"context"
	"encoding/json"
	"errors"
	"path/filepath"
	"sync"
	"time"

	"github.com/QuincySx/Alva/go/platform/storage/session"
	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/agent/security"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/kernel/bus"
)

type Activity struct {
	ID, Name, SessionID, RunID, CallID string
	Effect                             abi.EffectKind
}
type Config struct {
	Repository                        *session.Repository
	Owner, SessionID, Cwd, Executable string
	Env                               map[string]string
	Guard                             *security.Guard
	Locks                             *core.ToolLockRegistry
	ApprovedHostAccess                bool
	Report                            func(context.Context, Activity) error
	// Optional trusted host lease extension. Hub supplies the actual admitted
	// product workspace lease, independent of the SDK's tool lock.
	RetainWorkspace func() (func(), error)
}
type Plugin struct {
	config         Config
	log            *session.Session
	mu             sync.Mutex
	runID          string
	life           context.Context
	cancel         context.CancelFunc
	closed         bool
	jobs           map[string]*job
	tools          []*shellTool
	gate           chan struct{}
	bytes, started int
	calls          int
	callsDone      chan struct{}
	// A Sessions owner, when explicitly supplied by product assembly, keeps
	// background resources beyond a successful run. Standalone New stays run-owned.
	persistent       bool
	runContext       context.Context
	reserved         bool
	generation       uint64
	recoveredUnknown []string
}

func New(ctx context.Context, c Config) (*Plugin, error) {
	if c.Repository == nil || c.Guard == nil || c.Locks == nil || c.Report == nil || !c.ApprovedHostAccess || !filepath.IsAbs(c.Cwd) || !filepath.IsAbs(c.Executable) || len(c.Env) > 128 {
		return nil, errors.New("explicit approved shell executable/cwd, actual native authority, shared locks, guard and risk reporter required")
	}
	env := map[string]string{}
	for k, v := range c.Env {
		env[k] = v
	}
	c.Env = env
	log, err := c.Repository.OpenSession(ctx, c.Owner, c.SessionID)
	if err != nil {
		return nil, err
	}
	p := &Plugin{config: c, log: log, jobs: map[string]*job{}, gate: make(chan struct{}, 1)}
	for _, name := range []string{"execute_shell", "read_shell_output", "write_shell_stdin", "kill_shell"} {
		p.tools = append(p.tools, &shellTool{plugin: p, name: name})
	}
	return p, nil
}
func (*Plugin) Name() string { return "shell" }
func (p *Plugin) Tools() []abi.Tool {
	out := make([]abi.Tool, len(p.tools))
	for i, t := range p.tools {
		out[i] = t
	}
	return out
}
func (p *Plugin) Register(_ context.Context, r *core.Registrar) error {
	return p.register(r, p)
}
func (p *Plugin) register(r *core.Registrar, lifecycle abi.Middleware) error {
	for _, t := range p.tools {
		if err := r.Tool(t); err != nil {
			return err
		}
	}
	if err := r.Middleware(lifecycle); err != nil {
		return err
	}
	if locks, ok := bus.Resolve[*core.ToolLockRegistry](r.Bus()); ok && locks != p.config.Locks {
		return errors.New("shell must share the actual host ToolLockRegistry")
	}
	lifetime := "Background jobs end with this run; they do not detach across turns or restarts."
	if p.persistent {
		lifetime = "Background jobs survive a successful Turn within this running application, under an explicit session owner. Stop/run failure, their own timeout or application Close cancels and reaps them. They do not survive/restart with the application; interrupted historical jobs become unknown without PID adoption or replay."
	}
	recovery := ""
	if len(p.recoveredUnknown) > 0 {
		ids, _ := json.Marshal(p.recoveredUnknown)
		recovery = " Previous application-owned jobs have UNKNOWN outcomes and were NOT adopted or restarted: " + string(ids) + ". These historical IDs are not live process handles; inspect native_history if needed."
	}
	return r.SystemPrompt("Host shell is explicitly enabled and NOT an OS sandbox. execute_shell runs the approved executable with an isolated explicit environment; command text may access the host/network and needs approval. Foreground waits for real cleanup. " + lifetime + " Every job retains a shared global execution lease until real group reap, including between turns; other host tools may wait until it exits or is explicitly stopped. Use read_shell_output for bounded full-output references, write_shell_stdin for explicit input/EOF, and kill_shell for termination. Opaque job IDs belong only to this native session, never arbitrary PIDs. All input/output is known-credential-redacted and reference-only; no automatic retries or arbitrary secret detection." + recovery)
}

func (p *Plugin) Finalize(_ context.Context, late core.LateContext) ([]abi.Tool, error) {
	locks, ok := bus.Resolve[*core.ToolLockRegistry](late.Bus)
	if !ok || locks != p.config.Locks {
		return nil, errors.New("shell requires its actual shared ToolLockRegistry in the final assembly")
	}
	return nil, nil
}
func (p *Plugin) OnRunStart(ctx context.Context, r abi.RuntimeContext) error {
	p.mu.Lock()
	defer p.mu.Unlock()
	return p.startLocked(ctx, r)
}
func (p *Plugin) startLocked(ctx context.Context, r abi.RuntimeContext) error {
	if p.closed || p.runID != "" || p.persistent && p.life.Err() != nil || r.State == nil || !p.log.SameAuthority(r.State.Session()) || r.Workspace != p.config.Cwd || r.RunID == "" {
		return errors.New("shell actual session/workspace/run mismatch")
	}
	if !p.persistent && !p.completeLocked() {
		return errors.New("previous owned shell still cleaning up")
	}
	if !p.persistent {
		p.jobs = map[string]*job{}
	}
	p.runID = r.RunID
	if !p.persistent {
		p.bytes = 0
	}
	p.started = 0
	p.runContext = ctx
	if !p.persistent {
		p.life, p.cancel = context.WithCancel(ctx)
	}
	return nil
}
func (p *Plugin) check(e abi.ToolExecutionContext) error {
	p.mu.Lock()
	defer p.mu.Unlock()
	return p.checkLocked(e)
}
func (p *Plugin) checkLocked(e abi.ToolExecutionContext) error {
	if p.closed || p.runID == "" || p.runID != e.RunID || e.SessionID != p.config.SessionID || !p.log.SameAuthority(e.Session) || e.Workspace != p.config.Cwd || p.life.Err() != nil || p.runContext == nil || p.runContext.Err() != nil {
		return errors.New("shell is not bound to actual active native run")
	}
	return nil
}

// Admission includes approval and management callbacks, which may still use
// native facts after the process itself has been reaped. No job/global lease is
// acquired here: stdin/kill must remain able to manage a running background job.
func (p *Plugin) beginCall(e abi.ToolExecutionContext) (context.Context, context.Context, func(), error) {
	p.mu.Lock()
	defer p.mu.Unlock()
	if err := p.checkLocked(e); err != nil {
		return nil, nil, nil, err
	}
	if p.calls == 0 {
		p.callsDone = make(chan struct{})
	}
	p.calls++
	return p.life, p.runContext, func() {
		p.mu.Lock()
		defer p.mu.Unlock()
		p.calls--
		if p.calls == 0 {
			close(p.callsDone)
		}
	}, nil
}
func (p *Plugin) OnRunEnd(ctx context.Context, r abi.RuntimeContext, runErr error) error {
	return p.endRun(ctx, r, runErr, 0)
}
func (p *Plugin) endRun(ctx context.Context, r abi.RuntimeContext, runErr error, generation uint64) error {
	p.mu.Lock()
	if p.runID != r.RunID || generation != 0 && p.generation != generation {
		p.mu.Unlock()
		return nil
	}
	if p.persistent && runErr == nil && p.runContext != nil && p.runContext.Err() == nil {
		p.runID = ""
		p.runContext = nil
		p.mu.Unlock()
		return nil
	}
	if p.cancel != nil {
		p.cancel()
	}
	p.mu.Unlock()
	cleanup, cancel := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
	defer cancel()
	err := p.cleanup(cleanup)
	p.mu.Lock()
	if p.runID == r.RunID && p.completeLocked() {
		p.runID = ""
		p.runContext = nil
	}
	p.mu.Unlock()
	return err
}
func (p *Plugin) Close(ctx context.Context) error {
	p.mu.Lock()
	p.closed = true
	if p.cancel != nil {
		p.cancel()
	}
	p.mu.Unlock()
	return p.cleanup(ctx)
}
func (p *Plugin) cleanup(ctx context.Context) error {
	p.mu.Lock()
	jobs := make([]*job, 0, len(p.jobs))
	for _, j := range p.jobs {
		jobs = append(jobs, j)
		j.cancel()
	}
	p.mu.Unlock()
	for _, j := range jobs {
		if j.process != nil {
			j.process.BeginClose()
		}
	}
	var result error
	for _, j := range jobs {
		select {
		case <-j.done:
			select {
			case <-j.leaseDone:
			case <-ctx.Done():
				return errors.Join(result, ctx.Err())
			}
			j.mu.Lock()
			if !abi.OnlyCancellation(j.err) {
				result = errors.Join(result, j.err)
			}
			j.mu.Unlock()
		case <-ctx.Done():
			return errors.Join(result, ctx.Err())
		}
	}
	p.mu.Lock()
	calls, callsDone := p.calls, p.callsDone
	p.mu.Unlock()
	if calls != 0 {
		select {
		case <-callsDone:
		case <-ctx.Done():
			return errors.Join(result, ctx.Err())
		}
	}
	// Wait for a launch already admitted before cancellation to publish its job.
	select {
	case p.gate <- struct{}{}:
		defer func() { <-p.gate }()
	case <-ctx.Done():
		return errors.Join(result, ctx.Err())
	}
	// Re-snapshot while new launch publication is excluded.
	p.mu.Lock()
	jobs = jobs[:0]
	for _, j := range p.jobs {
		jobs = append(jobs, j)
		j.cancel()
	}
	p.mu.Unlock()
	for _, j := range jobs {
		j.process.BeginClose()
		select {
		case <-j.done:
			select {
			case <-j.leaseDone:
			case <-ctx.Done():
				return errors.Join(result, ctx.Err())
			}
			j.mu.Lock()
			if !abi.OnlyCancellation(j.err) {
				result = errors.Join(result, j.err)
			}
			j.mu.Unlock()
		case <-ctx.Done():
			return errors.Join(result, ctx.Err())
		}
	}
	p.mu.Lock()
	complete := p.completeLocked()
	p.mu.Unlock()
	if !complete {
		return errors.Join(result, errors.New("owned shell physical cleanup is incomplete"))
	}
	return result
}
func (p *Plugin) completeLocked() bool {
	for _, j := range p.jobs {
		select {
		case <-j.done:
			select {
			case <-j.leaseDone:
			default:
				return false
			}
			if j.process != nil && !j.process.CleanupComplete() {
				return false
			}
		default:
			return false
		}
	}
	return true
}
func (p *Plugin) CleanupComplete() bool {
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.calls != 0 || !p.completeLocked() {
		return false
	}
	select {
	case p.gate <- struct{}{}:
		<-p.gate
		return true
	default:
		return false
	}
}
func (p *Plugin) append(ctx context.Context, kind string, value any) error {
	data, err := json.Marshal(value)
	if err != nil {
		return err
	}
	_, err = p.log.AppendRecords(ctx, []abi.SessionRecord{{Kind: abi.RecordExtension, Namespace: "alva.shell", Type: kind, SchemaVersion: 1, Payload: data}})
	if err != nil {
		return &abi.SessionError{Operation: "append shell " + kind, SessionID: p.config.SessionID, Err: err}
	}
	if err = p.log.Flush(ctx); err != nil {
		return &abi.SessionError{Operation: "flush shell " + kind, SessionID: p.config.SessionID, Err: err}
	}
	return nil
}
