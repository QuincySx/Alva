// INPUT: context, errors, sync, unicode/utf8, agent/abi/context/scope, kernel/abi, llm/wire.
// OUTPUT: ChildFactory, GrantCatalog, ChildRequest, Child, ModelResolver, TemplateRegistry, Templates and Budget.
// POS: [L4 extension] Harness-owned child execution/budget contracts without native storage or parent-history authority in the SDK.
package subagent

import (
	"context"
	"errors"
	"sync"
	"unicode/utf8"

	agentabi "github.com/QuincySx/Alva/sdk/agent/abi"
	"github.com/QuincySx/Alva/sdk/agent/context/scope"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type Runner interface {
	Run(context.Context, []wire.Message, func(abi.Event) error) error
}
type Child struct {
	Runner  Runner
	Session abi.ConversationLog
	Link    abi.RecordReference
	Close   func(context.Context) error
	// Optional physical completion evidence, independent of journal errors.
	// Without it, Close must return nil only after cleanup actually completes.
	CleanupComplete func() bool
}
type ChildRequest struct {
	Parent                              abi.ConversationLog
	ParentRunID, SpawnCallID, Workspace string
	Scope                               *scope.SpawnScope
	Model                               abi.LanguageModel
	Grants                              []abi.Tool
	Hooks                               []abi.ContextHooks
	Budget                              *Budget
	Templates                           TemplateRegistry
	Models                              ModelResolver
	Communications                      *agentabi.CommunicationRegistry
}
type ChildFactory interface {
	// Any returned Child.Close transfers cleanup ownership to the caller, even
	// with an error. A factory that already cleaned up must return no closer.
	Prepare(context.Context, ChildRequest) (Child, error)
}

// GrantCatalog is an optional host-owned restriction over actual parent Tool
// instances, not a contribution's self-reported permission annotation.
type GrantCatalog interface{ GrantableTools() []abi.Tool }
type ChildFactoryFunc func(context.Context, ChildRequest) (Child, error)

func (fn ChildFactoryFunc) Prepare(ctx context.Context, request ChildRequest) (Child, error) {
	if fn == nil {
		return Child{}, errors.New("subagent factory function is nil")
	}
	return fn(ctx, request)
}

type ModelResolver interface {
	Resolve(context.Context, string) (abi.LanguageModel, error)
}
type ModelResolverFunc func(context.Context, string) (abi.LanguageModel, error)

func (fn ModelResolverFunc) Resolve(ctx context.Context, name string) (abi.LanguageModel, error) {
	if fn == nil {
		return nil, errors.New("model resolver function is nil")
	}
	return fn(ctx, name)
}

type Template struct {
	Name, Description, Role, SystemPrompt, Model string
	Tools                                        []string
}
type TemplateRegistry interface {
	Get(string) (Template, bool)
	List() []Template
}
type Templates struct{ values []Template }

func NewTemplates(values []Template) (*Templates, error) {
	if len(values) > 64 {
		return nil, errors.New("subagent template count exceeds 64")
	}
	registry := &Templates{}
	seen := map[string]bool{}
	for _, value := range values {
		if !validTemplate(value) || seen[value.Name] {
			return nil, errors.New("invalid or duplicated subagent template")
		}
		seen[value.Name] = true
		value.Tools = append([]string(nil), value.Tools...)
		registry.values = append(registry.values, value)
	}
	return registry, nil
}
func validTemplate(value Template) bool {
	if value.Name == "" || len(value.Name) > 128 || len(value.Description) > 512 || len(value.Role) > 128 || len(value.SystemPrompt) > 16<<10 || len(value.Model) > 256 || len(value.Tools) > 128 {
		return false
	}
	for _, text := range []string{value.Name, value.Description, value.Role, value.SystemPrompt, value.Model} {
		if !utf8.ValidString(text) {
			return false
		}
	}
	seen := map[string]bool{}
	for _, name := range value.Tools {
		if name == "" || len(name) > 256 || !utf8.ValidString(name) || seen[name] {
			return false
		}
		seen[name] = true
	}
	return true
}
func (registry *Templates) Get(name string) (Template, bool) {
	for _, template := range registry.values {
		if template.Name == name {
			template.Tools = append([]string(nil), template.Tools...)
			return template, true
		}
	}
	return Template{}, false
}
func (registry *Templates) List() []Template {
	values := make([]Template, len(registry.values))
	for index, value := range registry.values {
		value.Tools = append([]string(nil), value.Tools...)
		values[index] = value
	}
	return values
}

// Admission counts the whole recursive tree. The semaphore limits only model
// streams, not parent coordinators waiting for children (which would deadlock).
type Budget struct {
	mu             sync.Mutex
	started, limit int
	models         chan struct{}
}

func NewBudget(maxChildren, maxParallelModels int) (*Budget, error) {
	if maxChildren < 1 || maxChildren > 128 || maxParallelModels < 1 || maxParallelModels > 16 {
		return nil, errors.New("invalid bounded recursive spawn/model budget")
	}
	return &Budget{limit: maxChildren, models: make(chan struct{}, maxParallelModels)}, nil
}
func (b *Budget) admit() error {
	b.mu.Lock()
	defer b.mu.Unlock()
	if b.started >= b.limit {
		return errors.New("recursive child admission budget exceeded")
	}
	b.started++
	return nil
}
func (b *Budget) Started() int { b.mu.Lock(); defer b.mu.Unlock(); return b.started }

type ModelBudgetMiddleware struct{ Budget *Budget }

func (ModelBudgetMiddleware) Name() string { return "child-model-budget" }
func (m ModelBudgetMiddleware) WrapModel(ctx context.Context, runtime abi.RuntimeContext, request abi.ModelRequest, emit func(wire.StreamChunk) error, next abi.ModelFunc) error {
	if m.Budget == nil {
		return errors.New("child model budget unavailable")
	}
	select {
	case m.Budget.models <- struct{}{}:
	case <-ctx.Done():
		return ctx.Err()
	}
	defer func() { <-m.Budget.models }()
	return next(ctx, runtime, request, emit)
}
