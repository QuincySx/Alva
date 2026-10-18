// INPUT: context, errors, filepath/reflect/sync, core/security, ABI/bus, session.
// OUTPUT: Native planning Config with replaceable Service, Plugin and exact real Task/Plan Tools.
// POS: [L4 extension] Planning bookkeeping is optional Plugin policy; permission-mode changes are not faked with success text or global setters.
package planning

import (
	"context"
	"errors"
	"path/filepath"
	"reflect"
	"sync"

	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/kernel/bus"
)

type Config struct {
	Repository            *session.Repository
	Owner, SessionID, Cwd string
	Guard                 *security.Guard
	// Service is a trusted host-supplied business backend, bound to this
	// session by its owner. Nil selects the native durable implementation.
	// Its lifetime and persistence remain the supplying host's responsibility.
	Service Service
}
type Plugin struct {
	config  Config
	log     *session.Session
	service Service
	mu      sync.Mutex
	runID   string
	tools   []*planningTool
}

func New(ctx context.Context, c Config) (*Plugin, error) {
	if c.Repository == nil || c.Guard == nil || !filepath.IsAbs(c.Cwd) {
		return nil, errors.New("actual native planning authority/cwd/guard required")
	}
	if c.Service != nil && missingService(c.Service) {
		return nil, errors.New("typed-nil planning service")
	}
	log, err := c.Repository.OpenSession(ctx, c.Owner, c.SessionID)
	if err != nil {
		return nil, err
	}
	selected := c.Service
	if selected == nil {
		selected, err = newService(ctx, c.Repository, log, c.Owner, c.SessionID)
		if err != nil {
			return nil, err
		}
	}
	p := &Plugin{config: c, log: log, service: selected}
	for _, name := range []string{"task_create", "task_get", "task_list", "task_update", "task_stop", "task_output", "todo_write"} {
		p.tools = append(p.tools, &planningTool{plugin: p, name: name})
	}
	return p, nil
}
func (*Plugin) Name() string { return "planning" }
func (p *Plugin) Tools() []abi.Tool {
	out := make([]abi.Tool, len(p.tools))
	for i, t := range p.tools {
		out[i] = t
	}
	return out
}
func (p *Plugin) Service() Service { return p.service }
func (p *Plugin) Register(_ context.Context, r *core.Registrar) error {
	for _, t := range p.tools {
		if err := r.Tool(t); err != nil {
			return err
		}
	}
	if err := r.Middleware(p); err != nil {
		return err
	}
	if err := bus.Provide[Service](r.BusWriter(), p.service); err != nil {
		return err
	}
	return r.SystemPrompt("Planning tools consume an explicitly configured session-bound workflow Service. The default native Service stores durable local_workflow task records and notes; an explicitly supplied Service owns its business-state persistence. Tool execution and retained detail evidence still belong to this actual native session. Creating/updating/stopping a planning task does NOT launch or kill an agent/process, and does not invent an output file. task_stop cancels only workflow bookkeeping. task_output exposes explicitly appended note references, not execution logs. Dependencies must be completed before marking running; terminal tasks cannot reopen. todo_write replaces configured plan notes, never silently edits CLAUDE.md or an arbitrary workspace file. No enter_plan_mode/exit_plan_mode tools are registered: this plugin does not claim to restrict or lift actual permissions, change a global manager, or bypass parent authorization. Permission-policy transitions remain an explicit host operation pending a correct policy-change approval contract.")
}
func (p *Plugin) OnRunStart(_ context.Context, r abi.RuntimeContext) error {
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.runID != "" || r.State == nil || !p.log.SameAuthority(r.State.Session()) || r.Workspace != p.config.Cwd || r.RunID == "" {
		return errors.New("planning actual session/workspace/run mismatch")
	}
	p.runID = r.RunID
	return nil
}
func (p *Plugin) OnRunEnd(_ context.Context, r abi.RuntimeContext, _ error) error {
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.runID == r.RunID {
		p.runID = ""
	}
	return nil
}
func (p *Plugin) check(e abi.ToolExecutionContext) error {
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.runID == "" || e.RunID != p.runID || e.SessionID != p.config.SessionID || !p.log.SameAuthority(e.Session) || e.Workspace != p.config.Cwd {
		return errors.New("planning is not bound to actual active native run")
	}
	return nil
}

func missingService(value Service) bool {
	if value == nil {
		return true
	}
	v := reflect.ValueOf(value)
	switch v.Kind() {
	case reflect.Pointer, reflect.Func, reflect.Map, reflect.Slice, reflect.Interface, reflect.Chan:
		return v.IsNil()
	}
	return false
}
