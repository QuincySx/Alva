// INPUT: context, errors, reflect, sync, time, agent/abi/core/context/scope/security, kernel/abi.
// OUTPUT: Config, Plugin, New, Tool, scope lifecycle and selected template/model contracts.
// POS: [L4 extension] Optional recursive agent Tool Plugin binds actual runtime state without inheriting root tools or logs.
package subagent

import (
	"context"
	"errors"
	"reflect"
	"sync"
	"time"

	agentabi "github.com/QuincySx/Alva/go/sdk/agent/abi"
	agentcontext "github.com/QuincySx/Alva/go/sdk/agent/context"
	"github.com/QuincySx/Alva/go/sdk/agent/context/scope"
	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/agent/security"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
)

type Config struct {
	SessionID                                               string
	Factory                                                 ChildFactory
	Guard                                                   *security.Guard
	References                                              agentcontext.ReferenceBackend
	Timeout                                                 time.Duration
	MaxIterations, MaxDepth, MaxChildren, MaxParallelModels int
	Scope                                                   *scope.SpawnScope
	Budget                                                  *Budget
	Templates                                               TemplateRegistry
	Models                                                  ModelResolver
	Communications                                          *agentabi.CommunicationRegistry
}
type Plugin struct {
	mu     sync.Mutex
	config Config
	tool   *agentTool
	scope  *scope.SpawnScope
	budget *Budget
	state  *abi.AgentState
	runID  string
}

func New(config Config) (*Plugin, error) {
	if config.SessionID == "" || nilImplementation(config.Factory) || config.Guard == nil || nilImplementation(config.References) || config.Timeout <= 0 || config.Timeout > 30*time.Minute || config.MaxIterations < 1 || config.MaxIterations > 256 || config.MaxDepth < 0 || config.MaxDepth > 8 {
		return nil, errors.New("explicit subagent session/factory/security/reference capabilities and finite limits required")
	}
	if config.Models != nil && nilImplementation(config.Models) || config.Templates != nil && nilImplementation(config.Templates) {
		return nil, errors.New("typed-nil optional subagent capability")
	}
	if config.MaxChildren == 0 {
		config.MaxChildren = 32
	}
	if config.MaxParallelModels == 0 {
		config.MaxParallelModels = 4
	}
	if _, err := NewBudget(config.MaxChildren, config.MaxParallelModels); err != nil {
		return nil, err
	}
	if config.Scope != nil && (config.Scope.SessionID() != config.SessionID || config.Budget == nil) {
		return nil, errors.New("recursive plugin requires actual child scope/session and shared budget")
	}
	p := &Plugin{config: config}
	p.tool = &agentTool{plugin: p}
	return p, nil
}
func (*Plugin) Name() string     { return "subagent" }
func (p *Plugin) Tool() abi.Tool { return p.tool }
func (p *Plugin) Register(_ context.Context, r *core.Registrar) error {
	if err := r.Tool(p.tool); err != nil {
		return err
	}
	if err := r.Middleware(p); err != nil {
		return err
	}
	return r.SystemPrompt("The agent tool explicitly delegates one task to an independent native child session. Tools are a strict subset of the actual direct parent's granted catalog; empty means no host tools. Each child has its own history/references/MCP journal and cannot read parent/ancestor/sibling logs. Child output arrives as a selected-output reference owned by this parent session, not full child history. Recursion, model streams and wall-clock budgets are finite; no background daemon, automatic replay, provider credential discovery or automatic Hub Worker creation is implied.")
}
func (p *Plugin) OnRunStart(ctx context.Context, runtime abi.RuntimeContext) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if runtime.State == nil || runtime.State.Session().ID() != p.config.SessionID || runtime.RunID == "" {
		return errors.New("subagent actual session/run mismatch")
	}
	// Snapshot outside p.mu: registry queries this Tool's live schema.
	tools := runtime.State.Tools().Snapshot().Tools
	if catalog, ok := p.config.Factory.(GrantCatalog); ok {
		var supported []abi.Tool
		for _, tool := range tools {
			for _, allowed := range catalog.GrantableTools() {
				if sameActualTool(tool, allowed) {
					supported = append(supported, tool)
					break
				}
			}
		}
		tools = supported
	}
	active := p.config.Scope
	if active == nil {
		var err error
		active, err = scope.NewRoot(runtime.State.Model(), tools, scope.Config{SessionID: p.config.SessionID, Timeout: p.config.Timeout, MaxIterations: p.config.MaxIterations, MaxDepth: p.config.MaxDepth})
		if err != nil {
			return err
		}
	}
	if active.Snapshot().Completed {
		return errors.New("completed child scope cannot run again")
	}
	budget := p.config.Budget
	if budget == nil {
		var err error
		budget, err = NewBudget(p.config.MaxChildren, p.config.MaxParallelModels)
		if err != nil {
			return err
		}
	}
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.runID != "" {
		return errors.New("subagent Plugin already active")
	}
	p.scope = active
	p.budget = budget
	p.state = runtime.State
	p.runID = runtime.RunID
	return nil
}

func sameActualTool(left, right abi.Tool) bool {
	return left != nil && right != nil && reflect.TypeOf(left) == reflect.TypeOf(right) && reflect.ValueOf(left).Comparable() && reflect.ValueOf(right).Comparable() && left == right
}

func nilImplementation(value any) bool {
	if value == nil {
		return true
	}
	reflected := reflect.ValueOf(value)
	switch reflected.Kind() {
	case reflect.Chan, reflect.Func, reflect.Interface, reflect.Map, reflect.Pointer, reflect.Slice:
		return reflected.IsNil()
	default:
		return false
	}
}
func (p *Plugin) grantableNames(active *scope.SpawnScope, state *abi.AgentState) []string {
	if active == nil {
		return nil
	}
	names := active.ParentToolNames()
	catalog, restricted := p.config.Factory.(GrantCatalog)
	if !restricted {
		return names
	}
	if state == nil {
		return nil
	}
	allowed := catalog.GrantableTools()
	var result []string
	for _, name := range names {
		actual, exists := state.Tools().Get(name)
		if !exists {
			continue
		}
		for _, tool := range allowed {
			if sameActualTool(actual, tool) {
				result = append(result, name)
				break
			}
		}
	}
	return result
}
func (p *Plugin) OnRunEnd(_ context.Context, runtime abi.RuntimeContext, _ error) error {
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.runID == runtime.RunID {
		if p.scope != nil {
			p.scope.Complete()
		}
		p.runID = ""
		p.state = nil
	}
	return nil
}
