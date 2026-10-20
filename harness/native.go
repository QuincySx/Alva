// INPUT: platform/system/strictjson, context, encoding/json, sync, storage/session, agent/core, kernel/abi.
// OUTPUT: NativeSessionPlugins binding a bounded context source, run claims, references and history.
// POS: [L5 harness] Explicit native harness recipe connects one actual session authority without SDK storage policy.
package harness

import (
	"context"
	"encoding/json"
	"errors"
	"sync"

	"github.com/QuincySx/Alva/platform/storage/session"
	strictjson "github.com/QuincySx/Alva/platform/system/strictjson"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

// The caller must obtain owner/nativeID from its trusted session binding. These
// are not independent credentials accepted from UI fields.
func NativeSessionPlugins(ctx context.Context, repository *session.Repository, owner, nativeID string) ([]core.Plugin, error) {
	return nativeSessionPlugins(ctx, repository, owner, nativeID, true)
}

// NativeSessionViews supplies context/references/history when an explicit outer
// execution owner (such as durable.Controller) already holds the native claim.
// It grants no execution claim and must not be used by an unowned runner.
func NativeSessionViews(ctx context.Context, repository *session.Repository, owner, nativeID string) ([]core.Plugin, error) {
	return nativeSessionPlugins(ctx, repository, owner, nativeID, false)
}
func nativeSessionPlugins(ctx context.Context, repository *session.Repository, owner, nativeID string, claim bool) ([]core.Plugin, error) {
	if repository == nil {
		return nil, errors.New("native session repository required")
	}
	backend, err := repository.BoundReferences(ctx, owner, nativeID)
	if err != nil {
		return nil, err
	}
	source := &nativeContextSource{repository: repository, owner: owner, id: nativeID}
	return []core.Plugin{&nativeContextPlugin{source: source}, &nativeSessionPlugin{repository: repository, owner: owner, id: nativeID, claim: claim, history: &nativeHistoryTool{repository: repository, owner: owner, id: nativeID}}, NewContextRetrievalPlugin(nativeID, backend)}, nil
}

// Source and context lifecycle are separate contributions so child communication
// hooks and explicit user context policies retain their own ContextSystem.
type nativeContextPlugin struct{ source abi.ContextSource }

func (*nativeContextPlugin) Name() string { return "native-context-source" }
func (p *nativeContextPlugin) Register(_ context.Context, r *core.Registrar) error {
	return r.ContextSource(p.source)
}

type nativeSessionPlugin struct {
	repository *session.Repository
	owner, id  string
	history    *nativeHistoryTool
	claim      bool
}

func (*nativeSessionPlugin) Name() string { return "native-session" }
func (p *nativeSessionPlugin) Register(_ context.Context, r *core.Registrar) error {
	// Claim the actual native log before any other run-start side effects.
	if p.claim {
		if err := r.Middleware(&nativeRunClaim{repository: p.repository, owner: p.owner, id: p.id}, -10000); err != nil {
			return err
		}
	}
	if err := r.Tool(p.history); err != nil {
		return err
	}
	retention := p.repository.ArtifactStorage()
	return r.SystemPrompt("This actual native session has durable typed execution records and session-owned artifacts. Artifact byte retention is " + string(retention.Retention) + ". native_history lists/searches small record references; context_read/context_search explicitly read those references. Original artifact bytes are retained with hashes within that declared lifetime. The model working view keeps a bounded recent user-turn tail and marks omitted history with explicit retrieval references; it does not summarize the prefix. No compression, frame extraction or media understanding is implied. Interrupted tools are marked execution-unknown and are never automatically replayed.")
}

type nativeRunClaim struct {
	mu         sync.Mutex
	repository *session.Repository
	owner, id  string
	release    func()
	runID      string
}

func (*nativeRunClaim) Name() string { return "native-run-claim" }
func (m *nativeRunClaim) OnRunStart(ctx context.Context, runtime abi.RuntimeContext) error {
	if runtime.State == nil || runtime.State.Session().ID() != m.id {
		return errors.New("native run claim does not belong to this runtime session")
	}
	actual, err := m.repository.OpenSession(ctx, m.owner, m.id)
	if err != nil {
		return err
	}
	if !actual.SameAuthority(runtime.State.Session()) || runtime.RunID == "" {
		return errors.New("native run claim requires the actual repository/session authority and run")
	}
	m.mu.Lock()
	defer m.mu.Unlock()
	if m.release != nil {
		return session.ErrActive
	}
	_, release, err := m.repository.BeginRun(ctx, m.owner, m.id)
	if err != nil {
		return err
	}
	m.release = release
	m.runID = runtime.RunID
	return nil
}
func (m *nativeRunClaim) OnRunEnd(_ context.Context, runtime abi.RuntimeContext, _ error) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	if m.release != nil && m.runID == runtime.RunID {
		m.release()
		m.release = nil
		m.runID = ""
	}
	return nil
}

type nativeHistoryTool struct {
	repository *session.Repository
	owner, id  string
}

func (nativeHistoryTool) Name() string { return "native_history" }
func (nativeHistoryTool) Description() string {
	return "List or literal-search small metadata for committed records in this actual native session only. Fixed high-water pagination; use returned references for explicit bounded reads. Never resumes Agents, opens global history or replays tools."
}
func (nativeHistoryTool) ParametersSchema() json.RawMessage {
	return json.RawMessage(`{"type":"object","properties":{"action":{"type":"string","enum":["list","search"]},"query":{"type":"string","maxLength":256},"after":{"type":"integer","minimum":0},"high_water":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":20}},"required":["action"],"additionalProperties":false}`)
}
func (t *nativeHistoryTool) Execute(ctx context.Context, execution abi.ToolExecutionContext, call wire.ToolCall) (wire.ToolResult, error) {
	if t == nil || t.repository == nil || t.id == "" || execution.SessionID != t.id || execution.ActualTool != t || call.Name != t.Name() || call.ID == "" || execution.CallID != call.ID {
		return wire.ToolResult{}, errors.New("history does not belong to this execution session")
	}
	var args struct {
		Action    string `json:"action"`
		Query     string `json:"query"`
		After     uint64 `json:"after"`
		HighWater uint64 `json:"high_water"`
		Limit     int    `json:"limit"`
	}
	if len(call.Arguments) > 4096 {
		return wire.ToolResult{}, errors.New("native history arguments exceed 4 KiB")
	}
	if err := strictjson.ValidateObject(call.Arguments, &args); err != nil {
		return wire.ToolResult{}, err
	}
	if err := json.Unmarshal(call.Arguments, &args); err != nil {
		return wire.ToolResult{}, err
	}
	if args.Limit == 0 {
		args.Limit = 5
	}
	if args.Limit < 1 || args.Limit > 20 {
		return wire.ToolResult{}, errors.New("history limit must be 1..20")
	}
	var payload []byte
	var err error
	for limit := args.Limit; limit >= 1; limit-- {
		switch args.Action {
		case "list":
			var page session.RecordPage
			page, err = t.repository.Page(ctx, t.owner, t.id, args.After, limit, args.HighWater)
			if err == nil {
				payload, err = json.Marshal(page)
			}
		case "search":
			var page session.SearchPage
			page, err = t.repository.Search(ctx, t.owner, t.id, args.Query, args.After, limit, args.HighWater)
			if err == nil {
				payload, err = json.Marshal(page)
			}
		default:
			return wire.ToolResult{}, errors.New("unknown history action")
		}
		if err != nil {
			return wire.ToolResult{}, err
		}
		if len(payload) <= 4096 {
			return wire.ToolResult{CallID: call.ID, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: string(payload)}}}, nil
		}
	}
	return wire.ToolResult{}, errors.New("history metadata exceeds bounded output budget")
}
func (nativeHistoryTool) Scheduling(wire.ToolCall) abi.Scheduling {
	return abi.Scheduling{ConcurrentSafe: true}
}
func (nativeHistoryTool) DescribeEffect(wire.ToolCall) abi.Effect {
	return abi.Effect{Kind: abi.EffectRead, Description: "Read this native session's bounded committed metadata"}
}
func (nativeHistoryTool) IsReadOnly(wire.ToolCall) bool { return true }
