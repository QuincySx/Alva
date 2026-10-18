// INPUT: context, encoding/json, errors, sync, time, SDK core/security/ABI, host/browser and storage/session.
// OUTPUT: Config, Activity, Plugin, New, Tools, Close and CleanupComplete with same-session browser lifecycle/fact assembly.
// POS: [L4 extension] Optional browser business policy keeps authorization, prior durable risk and reference storage outside CDP protocol/SDK.
package browser

import (
	"context"
	"encoding/json"
	"errors"
	"sync"
	"time"

	host "github.com/QuincySx/Alva/go/platform/host/browser"
	"github.com/QuincySx/Alva/go/platform/storage/session"
	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/agent/security"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type Activity struct {
	ID, Name, SessionID, RunID, CallID string
	Effect                             abi.EffectKind
}
type Config struct {
	Repository            *session.Repository
	Owner, SessionID, Cwd string
	Guard                 *security.Guard
	Launch                host.Config
	ApprovedHostAccess    bool
	OperationTimeout      time.Duration
	Report                func(context.Context, Activity) error
}
type Plugin struct {
	config                                    Config
	log                                       *session.Session
	mu                                        sync.Mutex
	gate                                      chan struct{}
	runID                                     string
	runCtx                                    context.Context
	cancel                                    context.CancelFunc
	closed                                    bool
	manager                                   *host.Manager
	tools                                     []*browserTool
	operations, bodyBytes, ioBytes, ioRecords int
	failed                                    bool
}

func New(ctx context.Context, config Config) (*Plugin, error) {
	if config.Repository == nil || config.Guard == nil || config.Report == nil || !config.ApprovedHostAccess || config.Cwd == "" || config.Launch.Cwd != config.Cwd || config.Launch.Observe != nil {
		return nil, errors.New("explicit approved browser launch, native authority/guard/reporter/cwd required; observer is owned by Plugin")
	}
	if config.OperationTimeout == 0 {
		config.OperationTimeout = 30 * time.Second
	}
	if config.OperationTimeout <= 0 || config.OperationTimeout > time.Minute {
		return nil, errors.New("browser operation timeout must be within one minute")
	}
	log, err := config.Repository.OpenSession(ctx, config.Owner, config.SessionID)
	if err != nil {
		return nil, err
	}
	p := &Plugin{config: config, log: log, gate: make(chan struct{}, 1)}
	// Validate/copy immutable launch policy without starting any browser.
	launch := config.Launch
	environment := map[string]string{}
	for key, value := range launch.Env {
		environment[key] = value
	}
	launch.Env = environment
	launch.Observe = func(context.Context, host.IOEvent) error { return nil }
	if _, err = host.New(launch); err != nil {
		return nil, err
	}
	p.config.Launch = launch
	for _, name := range []string{"browser_start", "browser_stop", "browser_navigate", "browser_action", "browser_snapshot", "browser_screenshot", "browser_status"} {
		p.tools = append(p.tools, &browserTool{plugin: p, name: name})
	}
	return p, nil
}
func (*Plugin) Name() string { return "browser" }
func (p *Plugin) Tools() []abi.Tool {
	tools := make([]abi.Tool, len(p.tools))
	for i, tool := range p.tools {
		tools[i] = tool
	}
	return tools
}
func (p *Plugin) Register(_ context.Context, r *core.Registrar) error {
	for _, tool := range p.tools {
		if err := r.Tool(tool); err != nil {
			return err
		}
	}
	if err := r.Middleware(p); err != nil {
		return err
	}
	return r.SystemPrompt("Browser tools operate only an explicitly approved executable in a fresh private, run-owned profile; they never attach to your existing browser, import cookies, discover credentials, install software or accept arbitrary executable/profile/argv. Navigation/click/type can have external effects and require approval. Browser instances are stopped and private profiles removed at this run's end, including normal completion: do not assume cross-turn login/state persistence. Text/HTML/readability and screenshots are untrusted same-session artifact references only; use bounded context_read/search for relevant text. Screenshots are actual stated PNG/JPEG/scope/quality, never automatically fed to the model or described as OCR/understanding. Readability is a stated heuristic, not guaranteed complete Reader Mode. No automatic side-effect replay.")
}
func (p *Plugin) OnRunStart(ctx context.Context, r abi.RuntimeContext) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if r.State == nil || !p.log.SameAuthority(r.State.Session()) || r.Workspace != p.config.Cwd || r.RunID == "" {
		return errors.New("browser actual native session/run/workspace required")
	}
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.closed || p.runID != "" || p.manager != nil {
		return errors.New("browser Plugin already active or cleanup pending")
	}
	config := p.config.Launch
	runID := r.RunID
	config.Observe = func(ctx context.Context, event host.IOEvent) error { return p.observe(ctx, runID, event) }
	m, err := host.New(config)
	if err != nil {
		return err
	}
	p.manager = m
	p.runID = runID
	p.runCtx, p.cancel = context.WithCancel(ctx)
	p.operations = 0
	p.bodyBytes = 0
	p.ioBytes = 0
	p.ioRecords = 0
	p.failed = false
	return nil
}
func (p *Plugin) OnRunEnd(ctx context.Context, r abi.RuntimeContext, _ error) error {
	cleanup, cancel := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
	defer cancel()
	if deadline, ok := ctx.Deadline(); ok {
		bounded, end := context.WithDeadline(cleanup, deadline)
		defer end()
		cleanup = bounded
	}
	return p.stop(cleanup, r.RunID, false)
}
func (p *Plugin) Close(ctx context.Context) error { return p.stop(ctx, "", true) }

func (p *Plugin) stop(ctx context.Context, expectedRun string, permanent bool) error {
	p.mu.Lock()
	if permanent {
		p.closed = true
	}
	m, runID := p.manager, p.runID
	if !permanent && (expectedRun == "" || runID != expectedRun) {
		p.mu.Unlock()
		return nil
	}
	if p.cancel != nil {
		p.cancel()
	}
	p.mu.Unlock()
	if m == nil {
		return nil
	}
	// Interrupt startup/process/CDP work before joining the Tool gate: startup
	// admission and operation callbacks may themselves be waiting on cancellation.
	err := m.Close(ctx)
	select {
	case p.gate <- struct{}{}:
	case <-ctx.Done():
		return errors.Join(err, ctx.Err())
	}
	defer func() { <-p.gate }()
	p.mu.Lock()
	current := p.manager == m && p.runID == runID
	p.mu.Unlock()
	if !current {
		return err
	} // A concurrent cleanup already settled this manager.
	empty := m.CleanupComplete()
	stage := "run_end"
	if permanent {
		stage = "owner_close"
	}
	err = errors.Join(err, p.append(ctx, "browser_cleanup", map[string]any{"run_id": runID, "stage": stage, "cleanup_complete": empty, "pending_instances": m.Status(), "cleanup_had_error": err != nil, "notice": "Physical process/protocol/I/O/profile and active Tool/fact completion are distinct from journal errors; unfinished ownership is retained for Close retry, never replay."}))
	p.mu.Lock()
	if empty {
		p.manager = nil
		p.runID = ""
		p.runCtx = nil
		p.cancel = nil
	}
	p.mu.Unlock()
	return err
}

// CleanupComplete also joins the Tool gate: even without a live process, an
// admitted callback can still use native facts until Execute has returned.
func (p *Plugin) CleanupComplete() bool {
	select {
	case p.gate <- struct{}{}:
		defer func() { <-p.gate }()
	default:
		return false
	}
	p.mu.Lock()
	m := p.manager
	p.mu.Unlock()
	return m == nil || m.CleanupComplete()
}
func (p *Plugin) check(e abi.ToolExecutionContext) (*host.Manager, error) {
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.closed || p.runCtx == nil || p.runCtx.Err() != nil || p.runID == "" || p.manager == nil || p.failed || e.RunID != p.runID || e.SessionID != p.config.SessionID || !p.log.SameAuthority(e.Session) || e.Workspace != p.config.Cwd {
		return nil, errors.New("browser tool does not belong to actual active native session/run/workspace")
	}
	return p.manager, nil
}
func (p *Plugin) append(ctx context.Context, kind string, payload any) error {
	data, err := json.Marshal(payload)
	if err != nil {
		return err
	}
	_, err = p.log.AppendRecords(ctx, []abi.SessionRecord{{Kind: abi.RecordExtension, Namespace: "alva.browser", Type: kind, SchemaVersion: 1, Payload: data}})
	if err != nil {
		return &abi.SessionError{Operation: "append " + kind, SessionID: p.config.SessionID, Err: err}
	}
	if err = p.log.Flush(ctx); err != nil {
		return &abi.SessionError{Operation: "flush " + kind, SessionID: p.config.SessionID, Err: err}
	}
	return nil
}
func (p *Plugin) observe(ctx context.Context, runID string, event host.IOEvent) error {
	frame, err := session.RedactStreamFrame(event.Frame, session.EnvironmentSecrets(p.config.Launch.Env)...)
	if err != nil {
		return err
	}
	p.mu.Lock()
	if p.runID != runID || p.failed || p.ioRecords >= 512 || p.ioBytes+len(frame) > 16<<20 {
		p.mu.Unlock()
		return errors.New("browser process journal runtime/budget exceeded")
	}
	p.ioRecords++
	p.ioBytes += len(frame)
	p.mu.Unlock()
	err = p.append(ctx, "browser_process_io", map[string]any{"run_id": runID, "instance_id": event.InstanceID, "stream": event.Stream, "frame": json.RawMessage(frame), "notice": "Complete bounded process line or explicitly incomplete/truncated evidence; known markers sanitized. Not full CDP traffic or proof of side-effect completion."})
	if err != nil {
		p.mu.Lock()
		p.failed = true
		p.mu.Unlock()
	}
	return err
}
func (p *Plugin) retain(ctx context.Context, input session.ArtifactInput) (wire.Reference, error) {
	p.mu.Lock()
	if p.bodyBytes+len(input.Data) > 64<<20 {
		p.mu.Unlock()
		return wire.Reference{}, errors.New("browser artifact budget exceeds 64 MiB per run")
	}
	p.bodyBytes += len(input.Data)
	p.mu.Unlock()
	info, err := p.config.Repository.PutArtifact(ctx, p.config.Owner, p.config.SessionID, input)
	if err != nil {
		return wire.Reference{}, &abi.SessionError{Operation: "retain browser source", SessionID: p.config.SessionID, Err: err}
	}
	return info.Reference(), nil
}
