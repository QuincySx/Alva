// INPUT: context, JSON, errors, filepath/sync, portable utility/timer, core/security, ABI and native session.
// OUTPUT: Optional utility Plugin, Config, actual registry search/cancellable sleep and durable presentation preferences.
// POS: [L4 native adapter] Native preference CAS, authority and schema evidence wrap portable utility operations.
package native

import (
	"context"
	"encoding/json"
	"errors"
	"path/filepath"
	"sync"

	utility "github.com/QuincySx/Alva/extension/utility"
	"github.com/QuincySx/Alva/extension/utility/timer"
	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

type Config struct {
	Repository            *session.Repository
	Owner, SessionID, Cwd string
	Guard                 *security.Guard
}
type Plugin struct {
	config      Config
	log         *session.Session
	mu          sync.Mutex
	runID       string
	state       *abi.AgentState
	preferences map[string]string
	revision    uint64
	tools       []abi.Tool
}

func New(ctx context.Context, c Config) (*Plugin, error) {
	if c.Repository == nil || c.Guard == nil || !filepath.IsAbs(c.Cwd) {
		return nil, errors.New("explicit native utility authority/guard/cwd required")
	}
	log, err := c.Repository.OpenSession(ctx, c.Owner, c.SessionID)
	if err != nil {
		return nil, err
	}
	p := &Plugin{config: c, log: log, preferences: map[string]string{}}
	entry, found, err := c.Repository.LatestRecord(ctx, c.Owner, c.SessionID, "alva.utility", "preferences_snapshot")
	if err != nil {
		return nil, err
	}
	if found {
		_, raw, err := c.Repository.ReadArtifactFull(ctx, c.Owner, c.SessionID, entry.Value.URI, 16<<10)
		if err != nil {
			return nil, err
		}
		var record abi.SessionRecord
		if err = json.Unmarshal(raw, &record); err != nil {
			return nil, err
		}
		if record.Kind != abi.RecordExtension || record.Namespace != "alva.utility" || record.Type != "preferences_snapshot" || record.SchemaVersion != 1 || record.ID != entry.ID || record.Sequence != entry.Sequence {
			return nil, errors.New("utility snapshot identity/type mismatch")
		}
		if err = json.Unmarshal(record.Payload, &p.preferences); err != nil || p.preferences == nil {
			return nil, errors.New("invalid utility preferences snapshot")
		}
		for key, value := range p.preferences {
			if err = utility.ValidatePreference(key, value); err != nil {
				return nil, err
			}
		}
		p.revision = entry.Sequence
	}
	config, err := utility.NewConfigTool(p)
	if err != nil {
		return nil, err
	}
	search, err := utility.NewSearchTool(utility.SearchConfig{Snapshot: func() abi.ToolRegistrySnapshot {
		p.mu.Lock()
		defer p.mu.Unlock()
		if p.state == nil {
			return abi.ToolRegistrySnapshot{}
		}
		return p.state.Tools().Snapshot()
	}, RetainSchema: p.retainSchema})
	if err != nil {
		return nil, err
	}
	for _, tool := range []abi.Tool{config, search} {
		p.tools = append(p.tools, &utilityTool{Tool: tool, plugin: p, name: tool.Name()})
	}
	p.tools = append(p.tools, timer.NewTool())
	return p, nil
}
func (*Plugin) Name() string { return "utility" }
func (p *Plugin) Tools() []abi.Tool {
	result := make([]abi.Tool, len(p.tools))
	for i, t := range p.tools {
		result[i] = t
	}
	return result
}
func (p *Plugin) Register(_ context.Context, r *core.Registrar) error {
	for _, t := range p.tools {
		if err := r.Tool(t); err != nil {
			return err
		}
	}
	if err := r.Middleware(p); err != nil {
		return err
	}
	return r.SystemPrompt("Utility config stores only explicit response.language, response.verbosity and response.format presentation preferences in this native session; those values affect the next model request, never credentials/providers/permissions/tool grants. tool_search inspects the actual live registry with bounded name/description indexes; selected schemas are references and discovery never starts MCP/AEP workers. sleep is cancellable up to five minutes; prefer events and do not busy-poll.")
}
func (p *Plugin) OnRunStart(_ context.Context, r abi.RuntimeContext) error {
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.runID != "" || r.State == nil || !p.log.SameAuthority(r.State.Session()) || r.RunID == "" || r.Workspace != p.config.Cwd {
		return errors.New("utility actual session/workspace/run mismatch")
	}
	p.runID = r.RunID
	p.state = r.State
	return nil
}
func (p *Plugin) OnRunEnd(_ context.Context, r abi.RuntimeContext, _ error) error {
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.runID == r.RunID {
		p.runID = ""
		p.state = nil
	}
	return nil
}
func (p *Plugin) check(e abi.ToolExecutionContext) (*abi.AgentState, error) {
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.runID == "" || p.runID != e.RunID || e.SessionID != p.config.SessionID || !p.log.SameAuthority(e.Session) || e.Workspace != p.config.Cwd {
		return nil, errors.New("utility is not bound to actual active native run")
	}
	return p.state, nil
}
func (p *Plugin) BeforeModel(ctx context.Context, r abi.RuntimeContext, request *abi.ModelRequest) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	p.mu.Lock()
	valid := r.RunID == p.runID && r.State != nil && p.log.SameAuthority(r.State.Session()) && r.Workspace == p.config.Cwd
	p.mu.Unlock()
	if !valid {
		return errors.New("utility preference consumer authority mismatch")
	}
	return utility.ApplyPreferences(ctx, p, request)
}
func (p *Plugin) Preferences(ctx context.Context) (utility.PreferenceSnapshot, error) {
	if err := ctx.Err(); err != nil {
		return utility.PreferenceSnapshot{}, err
	}
	p.mu.Lock()
	defer p.mu.Unlock()
	values := make(map[string]string, len(p.preferences))
	for key, value := range p.preferences {
		values[key] = value
	}
	return utility.PreferenceSnapshot{Values: values, Revision: p.revision}, nil
}
func (p *Plugin) SetPreference(ctx context.Context, key, value string) error {
	return p.set(ctx, key, value)
}
func (p *Plugin) set(ctx context.Context, key, value string) error {
	if err := utility.ValidatePreference(key, value); err != nil {
		return err
	}
	p.mu.Lock()
	defer p.mu.Unlock()
	next := map[string]string{}
	for k, v := range p.preferences {
		next[k] = v
	}
	next[key] = value
	raw, _ := json.Marshal(next)
	record, err := p.log.AppendExtensionSnapshot(ctx, "alva.utility", "preferences_snapshot", p.revision, raw)
	if err != nil {
		return &abi.SessionError{Operation: "commit utility preferences", SessionID: p.config.SessionID, Err: err}
	}
	p.preferences = next
	p.revision = record.Sequence
	if err = p.log.Flush(ctx); err != nil {
		return &abi.SessionError{Operation: "flush utility preferences", SessionID: p.config.SessionID, Err: err}
	}
	return nil
}
