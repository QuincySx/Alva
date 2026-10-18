// INPUT: context, JSON, errors, filepath/reflect/sync/time, portable interaction, core/security, ABI/wire and native session.
// OUTPUT: Explicit human-question Plugin/Config, exact ask_human Tool and reference-only durable answer receipts.
// POS: [L4 native adapter] Native question authority, Guard, concurrency, deadlines and durable intent/outcome wrap shared question operations.
package native

import (
	"context"
	"encoding/json"
	"errors"
	"path/filepath"
	"reflect"
	"sync"
	"time"

	interaction "github.com/QuincySx/Alva/extension/interaction"
	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type Config struct {
	Repository            *session.Repository
	Owner, SessionID, Cwd string
	Guard                 *security.Guard
	Handler               interaction.Handler
	Timeout               time.Duration
}
type Plugin struct {
	config  Config
	log     *session.Session
	tool    *questionTool
	mu      sync.Mutex
	runID   string
	life    context.Context
	cancel  context.CancelFunc
	pending int
}

func New(ctx context.Context, c Config) (*Plugin, error) {
	if c.Repository == nil || c.Guard == nil || c.Handler == nil || !filepath.IsAbs(c.Cwd) {
		return nil, errors.New("explicit native question authority/guard/cwd and real human handler required")
	}
	v := reflect.ValueOf(c.Handler)
	switch v.Kind() {
	case reflect.Pointer, reflect.Func, reflect.Interface, reflect.Map, reflect.Slice, reflect.Chan:
		if v.IsNil() {
			return nil, errors.New("typed nil human handler")
		}
	}
	if c.Timeout == 0 {
		c.Timeout = 5 * time.Minute
	}
	if c.Timeout <= 0 || c.Timeout > 30*time.Minute {
		return nil, errors.New("human wait timeout must be within 30 minutes")
	}
	log, err := c.Repository.OpenSession(ctx, c.Owner, c.SessionID)
	if err != nil {
		return nil, err
	}
	p := &Plugin{config: c, log: log}
	references, err := c.Repository.BoundReferences(ctx, c.Owner, c.SessionID)
	if err != nil {
		return nil, err
	}
	tool, err := interaction.NewTool(c.Handler, references)
	if err != nil {
		return nil, err
	}
	p.tool = &questionTool{Tool: tool, plugin: p}
	return p, nil
}
func (*Plugin) Name() string        { return "interaction" }
func (p *Plugin) Tools() []abi.Tool { return []abi.Tool{p.tool} }
func (p *Plugin) Register(_ context.Context, r *core.Registrar) error {
	if err := r.Tool(p.tool); err != nil {
		return err
	}
	if err := r.Middleware(p); err != nil {
		return err
	}
	return r.SystemPrompt("ask_human asks a real human clarification through the explicitly configured host handler, with free text or 2..4 options. It is NOT permission approval and no response grants file/shell/network access. Complete sanitized questions and answers are retained as same-session references, not automatically fed wholesale into context; use context_read/search on selected evidence. Canceled/expired questions reject late responses; no question is silently replayed.")
}
func (p *Plugin) OnRunStart(ctx context.Context, r abi.RuntimeContext) error {
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.runID != "" || p.pending != 0 || r.State == nil || !p.log.SameAuthority(r.State.Session()) || r.Workspace != p.config.Cwd || r.RunID == "" {
		return errors.New("question actual session/workspace/run mismatch")
	}
	p.runID = r.RunID
	p.life, p.cancel = context.WithCancel(ctx)
	return nil
}
func (p *Plugin) OnRunEnd(_ context.Context, r abi.RuntimeContext, _ error) error {
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.runID == r.RunID {
		p.cancel()
		if p.pending > 0 {
			return errors.New("human handler still active after run ended")
		}
		p.runID = ""
	}
	return nil
}

type questionTool struct {
	abi.Tool
	plugin *Plugin
}

func (*questionTool) Scheduling(wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{ConcurrentSafe: true, ManagesOwnTimeout: true}
}

func (t *questionTool) Execute(ctx context.Context, e abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	var zero wire.ToolResult
	p := t.plugin
	if e.ActualTool != t || t != p.tool || call.ID == "" || call.Name != t.Name() || e.CallID != call.ID {
		return zero, errors.New("question requires actual dispatched tool/call identity")
	}
	q, err := interaction.ParseQuestion(e, call)
	if err != nil {
		return zero, err
	}
	if err := p.config.Guard.Authorize(ctx, e, call, t.DescribeEffect(call)); err != nil {
		return zero, err
	}
	p.mu.Lock()
	if p.runID == "" || p.runID != e.RunID || e.SessionID != p.config.SessionID || !p.log.SameAuthority(e.Session) || e.Workspace != p.config.Cwd || p.life.Err() != nil || p.pending >= 4 {
		p.mu.Unlock()
		return zero, errors.New("question is not bound to actual active run or capacity exceeded")
	}
	p.pending++
	life := p.life
	p.mu.Unlock()
	defer func() { p.mu.Lock(); p.pending--; p.mu.Unlock() }()
	deadline, cancel := context.WithTimeout(life, p.config.Timeout)
	stop := context.AfterFunc(ctx, cancel)
	defer cancel()
	defer stop()
	intent := false
	receipt, runErr := interaction.Ask(deadline, q, p.config.Handler, func(ctx context.Context, kind, name string, value any) (wire.Reference, error) {
		ref, err := p.retain(ctx, kind, name, value)
		if err == nil && kind == "human-question" {
			err = p.append(ctx, "question_intent", map[string]any{"question_id": q.ID, "run_id": e.RunID, "call_id": call.ID, "question": ref})
			intent = err == nil
		}
		return ref, err
	})
	if !intent {
		return zero, runErr
	}
	cleanup, end := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
	defer end()
	runErr = errors.Join(runErr, p.append(cleanup, "question_outcome", map[string]any{"question_id": q.ID, "run_id": e.RunID, "call_id": call.ID, "answer": receipt.Answer, "answered": runErr == nil, "notice": "Human clarification is not permission approval"}))
	if runErr != nil {
		return zero, runErr
	}
	return receipt.Result(call.ID)
}

func (p *Plugin) retain(ctx context.Context, kind, name string, value any) (wire.Reference, error) {
	raw, err := json.Marshal(value)
	if err != nil {
		return wire.Reference{}, err
	}
	raw, err = session.RedactJSON(raw)
	if err != nil {
		return wire.Reference{}, err
	}
	info, err := p.config.Repository.PutArtifact(ctx, p.config.Owner, p.config.SessionID, session.ArtifactInput{Kind: kind, Name: name, MediaType: "application/json", Processing: "Complete known-credential-redacted human clarification; not permission approval; no automatic full model delivery", Data: raw, Text: true})
	if err != nil {
		return wire.Reference{}, &abi.SessionError{Operation: "retain " + kind, SessionID: p.config.SessionID, Err: err}
	}
	return info.Reference(), nil
}
func (p *Plugin) append(ctx context.Context, kind string, value any) error {
	raw, err := json.Marshal(value)
	if err != nil {
		return err
	}
	_, err = p.log.AppendRecords(ctx, []abi.SessionRecord{{Kind: abi.RecordExtension, Namespace: "alva.interaction", Type: kind, SchemaVersion: 1, Payload: raw}})
	if err != nil {
		return &abi.SessionError{Operation: "append " + kind, SessionID: p.config.SessionID, Err: err}
	}
	if err = p.log.Flush(ctx); err != nil {
		return &abi.SessionError{Operation: "flush " + kind, SessionID: p.config.SessionID, Err: err}
	}
	return nil
}
