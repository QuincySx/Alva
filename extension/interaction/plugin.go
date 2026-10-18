// INPUT: bytes, context, crypto/rand, encoding/hex/json, errors/io, filepath, reflect/sync/time, core/security, ABI/wire, native session, platform/system/httpjson.
// OUTPUT: Explicit human-question Plugin/Config, exact ask_human Tool and reference-only durable answer receipts.
// POS: [L4 extension] Real handler waits are cancellable, retained as facts and never interpreted as permission approvals.
package interaction

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"path/filepath"
	"reflect"
	"sync"
	"time"

	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/platform/system/httpjson"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type Config struct {
	Repository            *session.Repository
	Owner, SessionID, Cwd string
	Guard                 *security.Guard
	Handler               Handler
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
	p.tool = &questionTool{plugin: p}
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

type questionTool struct{ plugin *Plugin }

func (*questionTool) Name() string { return "ask_human" }
func (*questionTool) Description() string {
	return "Ask a real human clarification and wait for a response; optional 2..4 choices/multi-select. Returns sanitized full question/answer references; never grants permissions or reads ambient stdin."
}
func (*questionTool) ParametersSchema() json.RawMessage {
	return json.RawMessage(`{"type":"object","additionalProperties":false,"required":["question"],"properties":{"question":{"type":"string","maxLength":8192},"header":{"type":"string","maxLength":1024},"options":{"type":"array","minItems":2,"maxItems":4,"items":{"type":"object","additionalProperties":false,"required":["label"],"properties":{"label":{"type":"string"},"value":{"type":"string"}}}},"multi_select":{"type":"boolean"},"metadata":{"type":"object"}}}`)
}
func (*questionTool) Scheduling(wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{ConcurrentSafe: true, ManagesOwnTimeout: true}
}
func (*questionTool) IsReadOnly(wire.ToolCall) bool { return true }
func (*questionTool) DescribeEffect(wire.ToolCall) abi.Effect {
	return abi.Effect{Kind: abi.EffectRead, Description: "Human clarification only, explicitly not authorization for host side effects"}
}
func (t *questionTool) Execute(ctx context.Context, e abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	var zero wire.ToolResult
	p := t.plugin
	if e.ActualTool != t || t != p.tool || call.ID == "" || call.Name != t.Name() || e.CallID != call.ID {
		return zero, errors.New("question requires actual dispatched tool/call identity")
	}
	if !small(string(call.Arguments), 32<<10) {
		return zero, errors.New("question arguments exceed 32 KiB UTF-8")
	}
	var q Question
	if err := httpjson.ValidateObject(call.Arguments, &q); err != nil {
		return zero, err
	}
	d := json.NewDecoder(bytes.NewReader(call.Arguments))
	d.DisallowUnknownFields()
	if err := d.Decode(&q); err != nil {
		return zero, err
	}
	if d.Decode(new(any)) != io.EOF {
		return zero, errors.New("one question object required")
	}
	// Input identity fields are forbidden even though host DTOs have them.
	var fields map[string]json.RawMessage
	json.Unmarshal(call.Arguments, &fields)
	for key, value := range fields {
		if bytes.Equal(bytes.TrimSpace(value), []byte("null")) {
			return zero, errors.New("question fields cannot be null")
		}
		if key != "question" && key != "header" && key != "options" && key != "multi_select" && key != "metadata" {
			return zero, errors.New("host question identity is not model input")
		}
	}
	var random [16]byte
	if _, err := rand.Read(random[:]); err != nil {
		return zero, err
	}
	q.ID = "question-" + hex.EncodeToString(random[:])
	q.SessionID = e.SessionID
	q.RunID = e.RunID
	q.CallID = e.CallID
	if err := q.Validate(); err != nil {
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
	question, err := p.retain(ctx, "human-question", q.ID, q)
	if err != nil {
		return zero, err
	}
	if err = p.append(ctx, "question_intent", map[string]any{"question_id": q.ID, "run_id": e.RunID, "call_id": call.ID, "question": question}); err != nil {
		return zero, err
	}
	deadline, cancel := context.WithTimeout(life, p.config.Timeout)
	stop := context.AfterFunc(ctx, cancel)
	defer cancel()
	defer stop()
	answer, runErr := p.config.Handler.Ask(deadline, CloneQuestion(q))
	if deadline.Err() != nil {
		runErr = errors.Join(runErr, deadline.Err())
	}
	var resolved ResolvedAnswer
	if runErr == nil {
		resolved, runErr = q.Resolve(answer)
	}
	cleanup, end := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
	defer end()
	var reference wire.Reference
	if runErr == nil {
		reference, runErr = p.retain(cleanup, "human-answer", q.ID, resolved)
	}
	runErr = errors.Join(runErr, p.append(cleanup, "question_outcome", map[string]any{"question_id": q.ID, "run_id": e.RunID, "call_id": call.ID, "answer": reference, "answered": runErr == nil, "notice": "Human clarification is not permission approval"}))
	if runErr != nil {
		return zero, runErr
	}
	raw, _ := json.Marshal(map[string]any{"question_id": q.ID, "question": question, "answer": reference, "structured": resolved.Structured, "selected": resolved.Selected, "notice": "Use bounded context_read/search for the actual human answer; this is not host authorization"})
	return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: string(raw)}}}, nil
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
