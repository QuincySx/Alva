// INPUT: context, crypto/sha256, encoding/hex/json, errors/filepath/sync/time, core/security, host Workspace, ABI/bus/wire, session.
// OUTPUT: Explicit Notebook Plugin/Config with exact notebook_edit, shared-lease checked writes and full original/prepared references.
// POS: [L4 extension] Native cell editing preserves filesystem authority and evidence without executing notebooks or pretending OS hash-CAS.
package notebook

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	workspacefs "github.com/QuincySx/Alva/platform/host/workspace"
	"path/filepath"
	"sync"
	"time"

	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/kernel/bus"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type Activity struct {
	ID, Name, SessionID, RunID, CallID string
	Effect                             abi.EffectKind
}
type Config struct {
	Repository            *session.Repository
	Owner, SessionID, Cwd string
	Guard                 *security.Guard
	Workspace             *workspacefs.Workspace
	Locks                 *core.ToolLockRegistry
	Report                func(context.Context, Activity) error
}
type Plugin struct {
	config Config
	log    *session.Session
	tool   *notebookTool
	mu     sync.Mutex
	runID  string
}

func New(ctx context.Context, c Config) (*Plugin, error) {
	if c.Repository == nil || c.Guard == nil || c.Workspace == nil || c.Locks == nil || c.Report == nil || !filepath.IsAbs(c.Cwd) {
		return nil, errors.New("actual notebook workspace/session authority, guard/shared locks and durable risk reporter required")
	}
	log, err := c.Repository.OpenSession(ctx, c.Owner, c.SessionID)
	if err != nil {
		return nil, err
	}
	p := &Plugin{config: c, log: log}
	p.tool = &notebookTool{p}
	return p, nil
}
func (*Plugin) Name() string        { return "notebook" }
func (p *Plugin) Tools() []abi.Tool { return []abi.Tool{p.tool} }
func (p *Plugin) Register(_ context.Context, r *core.Registrar) error {
	if err := r.Tool(p.tool); err != nil {
		return err
	}
	if err := r.Middleware(p); err != nil {
		return err
	}
	return r.SystemPrompt("notebook_edit edits existing nbformat4.5+ cell source by unique ID; insert appends unless insert_before is supplied. It never starts a notebook kernel or executes cells. Changed code clears stale outputs/execution_count. Original notebook bytes and prepared replacement are retained unchanged as same-session references; no notebook/media body is automatically fed to the model. Writes retain an actual shared global host lease and pinned parent directory; snapshot/hash checks reject observed changes, but are optimistic against noncooperating external OS writers, not an OS atomic hash-CAS. A failed replacement may have occurred: inspect retained evidence, never auto-replay.")
}
func (p *Plugin) Finalize(_ context.Context, late core.LateContext) ([]abi.Tool, error) {
	locks, ok := bus.Resolve[*core.ToolLockRegistry](late.Bus)
	if !ok || locks != p.config.Locks {
		return nil, errors.New("notebook must use final assembly's actual shared locks")
	}
	return nil, nil
}
func (p *Plugin) OnRunStart(_ context.Context, r abi.RuntimeContext) error {
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.runID != "" || r.State == nil || !p.log.SameAuthority(r.State.Session()) || r.Workspace != p.config.Cwd || r.RunID == "" {
		return errors.New("notebook actual session/workspace/run mismatch")
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
	if p.runID == "" || p.runID != e.RunID || e.SessionID != p.config.SessionID || !p.log.SameAuthority(e.Session) || e.Workspace != p.config.Cwd {
		return errors.New("notebook is not bound to actual active native run")
	}
	return nil
}

type notebookTool struct{ plugin *Plugin }

func (*notebookTool) Name() string { return "notebook_edit" }
func (*notebookTool) Description() string {
	return "Edit an existing nbformat4.5+ notebook cell by unique ID (replace/insert/delete). Does not execute code. Full original/prepared bytes remain references; observed concurrent changes and symlinks are rejected; no OS hash-CAS claim."
}
func (*notebookTool) ParametersSchema() json.RawMessage {
	return json.RawMessage(`{"type":"object","additionalProperties":false,"required":["notebook_path","cell_id","edit_mode"],"properties":{"notebook_path":{"type":"string"},"cell_id":{"type":"string","maxLength":64},"new_source":{"type":"string","maxLength":131072},"cell_type":{"type":"string","enum":["code","markdown"]},"edit_mode":{"type":"string","enum":["replace","insert","delete"]},"insert_before":{"type":"string","maxLength":64}}}`)
}
func (*notebookTool) Scheduling(wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{ExecutionMode: abi.Coordinator}
}
func (*notebookTool) IsReadOnly(wire.ToolCall) bool { return false }
func (*notebookTool) DescribeEffect(wire.ToolCall) abi.Effect {
	return abi.Effect{Kind: abi.EffectWrite, Description: "Mutate an explicitly selected notebook file without executing cells; full backup and observed-conflict checked replacement"}
}
func (t *notebookTool) Execute(ctx context.Context, e abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	var zero wire.ToolResult
	p := t.plugin
	if e.ActualTool != t || e.CallID != call.ID || call.Name != t.Name() {
		return zero, errors.New("notebook requires actual dispatched tool/call identity")
	}
	a, err := decode(call.Arguments)
	if err != nil {
		return zero, err
	}
	if err = p.check(e); err != nil {
		return zero, err
	}
	if err = p.config.Guard.Authorize(ctx, e, call, t.DescribeEffect(call)); err != nil {
		return zero, err
	}
	release, err := p.config.Locks.Acquire(ctx, abi.Scheduling{ExecutionMode: abi.SerialGlobal}, p.config.Cwd)
	if err != nil {
		return zero, err
	}
	defer release()
	if err = p.check(e); err != nil {
		return zero, err
	}
	before, err := p.config.Workspace.ReadSnapshot(ctx, a.Path, p.config.Guard)
	if err != nil {
		return zero, err
	}
	result, err := transform(before.Data, a)
	if err != nil {
		return zero, err
	}
	hash := sha256.Sum256([]byte(e.RunID + "\x00" + call.ID))
	id := "notebook-" + hex.EncodeToString(hash[:16])
	original, err := p.retain(ctx, "notebook-original", id, before.Data, "Original notebook bytes unchanged; retained before an attempted edit; no sanitization, execution or automatic model/media delivery")
	if err != nil {
		return zero, err
	}
	prepared, err := p.retain(ctx, "notebook-prepared", id, result.Data, "Prepared replacement JSON; exact source newline preservation; not proof of a completed write, no execution or automatic model/media delivery")
	if err != nil {
		return zero, err
	}
	if err = p.config.Report(ctx, Activity{ID: id, Name: "Notebook cell edit", SessionID: e.SessionID, RunID: e.RunID, CallID: e.CallID, Effect: abi.EffectWrite}); err != nil {
		return zero, err
	}
	if err = p.append(ctx, "edit_intent", map[string]any{"run_id": e.RunID, "call_id": call.ID, "path": a.Path, "cell_id": a.ID, "mode": a.Mode, "original": original, "prepared": prepared, "state": "replacement_attempted"}); err != nil {
		return zero, err
	}
	if err = p.check(e); err != nil {
		return zero, err
	}
	after, writeErr := p.config.Workspace.ReplaceCheckedSnapshot(ctx, a.Path, before, result.Data, p.config.Guard)
	cleanup, cancel := context.WithTimeout(context.WithoutCancel(ctx), 5*time.Second)
	defer cancel()
	receipt := map[string]any{"run_id": e.RunID, "call_id": call.ID, "path": a.Path, "cell_id": a.ID, "edit_mode": a.Mode, "cell_index": result.Index, "original": original, "prepared": prepared, "written": writeErr == nil, "new_sha256": after.SHA256, "cleared_code_outputs": result.ClearedOutputs, "executed": false, "notice": "Pinned-parent atomic rename with optimistic snapshot checks; not OS hash-CAS against noncooperating writers; originals/prepared bytes are references only"}
	writeErr = errors.Join(writeErr, p.append(cleanup, "edit_outcome", receipt))
	if writeErr != nil {
		return zero, writeErr
	}
	raw, _ := json.Marshal(receipt)
	return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: string(raw)}}}, nil
}
func (p *Plugin) retain(ctx context.Context, kind, name string, data []byte, processing string) (wire.Reference, error) {
	info, err := p.config.Repository.PutArtifact(ctx, p.config.Owner, p.config.SessionID, session.ArtifactInput{Kind: kind, Name: name, MediaType: "application/x-ipynb+json", Processing: processing, Data: data, Text: true})
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
	_, err = p.log.AppendRecords(ctx, []abi.SessionRecord{{Kind: abi.RecordExtension, Namespace: "alva.notebook", Type: kind, SchemaVersion: 1, Payload: raw}})
	if err != nil {
		return &abi.SessionError{Operation: "append " + kind, SessionID: p.config.SessionID, Err: err}
	}
	if err = p.log.Flush(ctx); err != nil {
		return &abi.SessionError{Operation: "flush " + kind, SessionID: p.config.SessionID, Err: err}
	}
	return nil
}
