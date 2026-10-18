// INPUT: context, errors, os, filepath, reflect, builtin/toolvalidation, llm/provider, agent/core/context/security, kernel/abi, llm/wire.
// OUTPUT: Runner, Builder, NewBuilder, NewDemo and NewOpenAI harness entry points.
// POS: [L5 harness] Default-by-name application harness above the independently usable SDK.
package harness

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"reflect"

	"github.com/QuincySx/Alva/go/extension/builtin"
	"github.com/QuincySx/Alva/go/extension/toolvalidation"
	"github.com/QuincySx/Alva/go/integration/llm/provider"
	agentcontext "github.com/QuincySx/Alva/go/sdk/agent/context"
	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/agent/security"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type Runner interface {
	Run(context.Context, []wire.Message, func(abi.Event) error) error
}

type Builder struct {
	model                           abi.LanguageModel
	session                         abi.ConversationLog
	plugins                         []core.Plugin
	tools                           []abi.Tool
	workspace                       string
	middleware                      []middlewareContribution
	prompt                          string
	maxIterations, maxResponseBytes int
	contextSystem                   *abi.ContextSystem
}

func NewBuilder(model abi.LanguageModel, session abi.ConversationLog) *Builder {
	return &Builder{model: model, session: session}
}
func (b *Builder) Plugin(plugin core.Plugin) *Builder {
	b.plugins = append(b.plugins, plugin)
	return b
}
func (b *Builder) Tool(tool abi.Tool) *Builder { b.tools = append(b.tools, tool); return b }

// DeclaredTools returns actual explicitly added Tool instances, detached from
// the slice. Plugin contributions are observed after assembly, not synthesized
// here by name. Hosts can include these exact objects in security/rebind rules.
func (b *Builder) DeclaredTools() []abi.Tool           { return append([]abi.Tool(nil), b.tools...) }
func (b *Builder) Workspace(workspace string) *Builder { b.workspace = workspace; return b }

type middlewareContribution struct {
	middleware abi.Middleware
	order      []int
}

// Middleware delegates the optional assembly order to the SDK registration path.
func (b *Builder) Middleware(middleware abi.Middleware, order ...int) *Builder {
	b.middleware = append(b.middleware, middlewareContribution{middleware: middleware, order: append([]int(nil), order...)})
	return b
}
func (b *Builder) SystemPrompt(prompt string) *Builder { b.prompt = prompt; return b }
func (b *Builder) MaxIterations(limit int) *Builder    { b.maxIterations = limit; return b }
func (b *Builder) MaxResponseBytes(limit int) *Builder { b.maxResponseBytes = limit; return b }
func (b *Builder) ContextSystem(system *abi.ContextSystem) *Builder {
	if system == nil {
		b.contextSystem = nil
	} else {
		copy := *system
		b.contextSystem = &copy
	}
	return b
}
func (b *Builder) Build(ctx context.Context) (*core.Agent, error) {
	// Freeze the harness recipe before ID/Name or other user implementations
	// can mutate the original Builder. Such edits belong to the next Build.
	recipe := *b
	recipe.plugins = append([]core.Plugin(nil), b.plugins...)
	recipe.tools = append([]abi.Tool(nil), b.tools...)
	recipe.middleware = append([]middlewareContribution(nil), b.middleware...)
	for index := range recipe.middleware {
		recipe.middleware[index].order = append([]int(nil), recipe.middleware[index].order...)
	}
	if b.contextSystem != nil {
		system := *b.contextSystem
		recipe.contextSystem = &system
	}
	b = &recipe
	if nilImplementation(b.model) || nilImplementation(b.session) || b.session.ID() == "" {
		return nil, errors.New("harness model and identified session are required")
	}
	workspace := b.workspace
	if workspace == "" {
		var err error
		workspace, err = os.Getwd()
		if err != nil {
			return nil, err
		}
	}
	if !filepath.IsAbs(workspace) {
		return nil, errors.New("harness workspace must be absolute")
	}
	builder := core.NewBuilder(b.model, b.session).Workspace(workspace)
	if b.prompt != "" {
		builder.SystemPrompt(b.prompt)
	}
	if b.maxIterations != 0 {
		builder.MaxIterations(b.maxIterations)
	}
	if b.maxResponseBytes != 0 {
		builder.MaxResponseBytes(b.maxResponseBytes)
	}
	if b.contextSystem != nil {
		builder.ContextSystem(b.contextSystem)
	}
	for _, contribution := range b.middleware {
		builder.Middleware(contribution.middleware, contribution.order...)
	}
	names := make(map[string]bool)
	knownTools := append([]abi.Tool(nil), b.tools...)
	for _, plugin := range b.plugins {
		if nilImplementation(plugin) {
			return nil, errors.New("nil harness plugin")
		}
		names[plugin.Name()] = true
		switch typed := plugin.(type) {
		case *ContextRetrievalPlugin:
			knownTools = append(knownTools, contributedTools(typed)...)
		case *nativeSessionPlugin:
			knownTools = append(knownTools, contributedTools(typed)...)
		}
		builder.Plugin(plugin)
	}
	backend, err := agentcontext.NewMemoryReferences(b.session.ID(), referenceStoreLimit)
	if err != nil {
		return nil, err
	}
	retrieval := NewContextRetrievalPlugin(b.session.ID(), backend)
	if !names[retrieval.Name()] {
		knownTools = append(knownTools, agentcontext.NewReferenceReadTool(backend, b.session.ID()), agentcontext.NewReferenceSearchTool(backend, b.session.ID()))
	}
	guard, err := security.NewGuard(workspace, nil, nil, security.NewPermissionManager(security.Ask, nil))
	if err != nil {
		return nil, err
	}
	defaults := []core.Plugin{NewMemoryPlugin(), builtin.NewSecurityPlugin(guard, knownTools), SystemContextPlugin{}, retrieval, toolvalidation.New()}
	if b.contextSystem != nil {
		defaults = append(defaults, contextBudgetPlugin{system: b.contextSystem})
	}
	for _, plugin := range defaults {
		if !names[plugin.Name()] {
			builder.Plugin(plugin)
		}
	}
	for _, tool := range b.tools {
		builder.Tool(tool)
	}
	return builder.Build(ctx)
}

func nilImplementation(value any) bool {
	if value == nil {
		return true
	}
	v := reflect.ValueOf(value)
	switch v.Kind() {
	case reflect.Chan, reflect.Func, reflect.Interface, reflect.Map, reflect.Pointer, reflect.Slice:
		return v.IsNil()
	default:
		return false
	}
}

func contributedTools(plugin core.Plugin) []abi.Tool {
	switch typed := plugin.(type) {
	case *ContextRetrievalPlugin:
		return []abi.Tool{agentcontext.NewReferenceReadTool(typed.backend, typed.sessionID), agentcontext.NewReferenceSearchTool(typed.backend, typed.sessionID)}
	case *nativeSessionPlugin:
		return []abi.Tool{typed.history}
	default:
		return nil
	}
}

// NewSecurityPlugin is the product recipe over the builtin wrapper. Only
// explicitly supplied tools and this harness's concrete owned contributions are
// trusted; arbitrary Plugin metadata cannot self-authorize an implementation.
func NewSecurityPlugin(guard *security.Guard, tools []abi.Tool, plugins ...core.Plugin) core.Plugin {
	trusted := append([]abi.Tool(nil), tools...)
	for _, plugin := range plugins {
		trusted = append(trusted, contributedTools(plugin)...)
	}
	return builtin.NewSecurityPlugin(guard, trusted)
}

// NewDemo is an explicitly local fixture: the model deterministically calls a
// harmless echo tool, then reads its committed result. It is not a paid model.
func NewDemo(ctx context.Context) (Runner, error) {
	return NewDemoSession(ctx, core.NewMemorySession("demo-session"))
}

// NewDemoSession binds the supplied native authority; no hardcoded session ID or
// presentation transcript is substituted by the harness.
func NewDemoSession(ctx context.Context, session abi.ConversationLog, plugins ...core.Plugin) (Runner, error) {
	builder := NewDemoBuilder(session)
	for _, plugin := range plugins {
		builder.Plugin(plugin)
	}
	return builder.Build(ctx)
}
func NewDemoBuilder(session abi.ConversationLog) *Builder {
	return NewBuilder(demoModel{}, session).Tool(echoTool{})
}

func NewOpenAI(ctx context.Context, config provider.Config) (Runner, error) {
	return NewOpenAISession(ctx, config, core.NewMemorySession("live-session"))
}

func NewOpenAISession(ctx context.Context, config provider.Config, session abi.ConversationLog, plugins ...core.Plugin) (Runner, error) {
	builder, err := NewOpenAIBuilder(config, session)
	if err != nil {
		return nil, err
	}
	for _, plugin := range plugins {
		builder.Plugin(plugin)
	}
	return builder.Build(ctx)
}
func NewOpenAIBuilder(config provider.Config, session abi.ConversationLog) (*Builder, error) {
	model, err := provider.NewOpenAIChat(config)
	if err != nil {
		return nil, err
	}
	return NewBuilder(model, session), nil
}
