// INPUT: context, errors, fmt, reflect, sort, strings, sync/atomic, agent/abi, kernel/abi, kernel/bus, kernel/core, llm/wire
// OUTPUT: Agent, Builder, NewBuilder, ErrRunInProgress
// POS: [L2.5 SDK assembly] Pure SDK assembly and invocation with no default capability or platform decisions.
package core

import (
	"context"
	"errors"
	"fmt"
	"reflect"
	"sort"
	"strings"
	"sync/atomic"

	agentabi "github.com/QuincySx/Alva/sdk/agent/abi"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/kernel/bus"
	kernel "github.com/QuincySx/Alva/sdk/kernel/core"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

var ErrRunInProgress = errors.New("agent already running")

type Builder struct {
	model              abi.LanguageModel
	session            abi.ConversationLog
	plugins            []Plugin
	tools              []abi.Tool
	middleware         []middlewareRegistration
	prompts            []string
	loop               kernel.AgentLoop
	maxIterations      int
	maxResponseBytes   int
	workspace          string
	contextSystem      *abi.ContextSystem
	maxConcurrentTools int
	modelConfig        wire.ModelConfig
}

func NewBuilder(model abi.LanguageModel, session abi.ConversationLog) *Builder {
	return &Builder{model: model, session: session, loop: kernel.DefaultAgentLoop{}}
}
func (b *Builder) Plugin(plugin Plugin) *Builder { b.plugins = append(b.plugins, plugin); return b }
func (b *Builder) Tool(tool abi.Tool) *Builder   { b.tools = append(b.tools, tool); return b }

// Middleware uses the same assembly order as Registrar.Middleware. Invalid
// multiple order arguments are retained as a Build error, before plugins run.
func (b *Builder) Middleware(middleware abi.Middleware, order ...int) *Builder {
	b.middleware = append(b.middleware, middlewareRegistration{middleware: middleware, orders: append([]int(nil), order...)})
	return b
}
func (b *Builder) SystemPrompt(prompt string) *Builder {
	b.prompts = append(b.prompts, prompt)
	return b
}
func (b *Builder) Loop(loop kernel.AgentLoop) *Builder   { b.loop = loop; return b }
func (b *Builder) MaxIterations(limit int) *Builder      { b.maxIterations = limit; return b }
func (b *Builder) MaxResponseBytes(limit int) *Builder   { b.maxResponseBytes = limit; return b }
func (b *Builder) MaxConcurrentTools(limit int) *Builder { b.maxConcurrentTools = limit; return b }
func (b *Builder) Workspace(workspace string) *Builder   { b.workspace = workspace; return b }
func (b *Builder) ContextSystem(system *abi.ContextSystem) *Builder {
	b.contextSystem = cloneContextSystem(system)
	return b
}
func (b *Builder) ModelConfig(config wire.ModelConfig) *Builder {
	b.modelConfig = wire.CloneModelConfig(config)
	return b
}

func (b *Builder) Build(ctx context.Context) (*Agent, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	// Callbacks may retain the original builder and configure its next Build.
	// Capture this recipe before invoking any contributed implementation.
	recipe := *b
	recipe.plugins = append([]Plugin(nil), b.plugins...)
	recipe.tools = append([]abi.Tool(nil), b.tools...)
	recipe.middleware = append([]middlewareRegistration(nil), b.middleware...)
	recipe.prompts = append([]string(nil), b.prompts...)
	recipe.contextSystem = cloneContextSystem(b.contextSystem)
	recipe.modelConfig = wire.CloneModelConfig(b.modelConfig)
	return recipe.assemble(ctx)
}

func (b *Builder) assemble(ctx context.Context) (*Agent, error) {
	if nilContribution(b.model) || nilContribution(b.session) || nilContribution(b.loop) || b.session.ID() == "" {
		return nil, errors.New("model, conversation log and loop are required")
	}
	if err := b.modelConfig.Validate(); err != nil {
		return nil, err
	}
	if b.maxIterations < 0 || b.maxResponseBytes < 0 {
		return nil, errors.New("limits cannot be negative")
	}
	if b.maxConcurrentTools < 0 || b.maxConcurrentTools > 256 {
		return nil, errors.New("tool concurrency limit must be between 0 and 256")
	}
	pluginNames := make(map[string]bool)
	identities := make([]string, len(b.plugins))
	for index, plugin := range b.plugins {
		if nilContribution(plugin) {
			return nil, errors.New("plugin identity missing")
		}
		name := plugin.Name()
		if !validIdentity(name) {
			return nil, errors.New("plugin identity missing or invalid")
		}
		if pluginNames[name] {
			return nil, fmt.Errorf("duplicate plugin %q", name)
		}
		pluginNames[name] = true
		identities[index] = name
	}
	writer, handle := bus.New()
	registration := &registration{writer: writer, workspace: b.workspace, toolNames: make(map[string]bool), middlewareNames: make(map[string]bool), commandNames: make(map[string]bool)}
	seal := func() { registration.sealed = true; writer.Seal() }
	defer seal() // Also closes escaped handles when a caller recovers a callback panic.
	registrar := &Registrar{registration: registration}
	if b.contextSystem != nil {
		if err := registrar.ContextSystem(b.contextSystem); err != nil {
			return nil, err
		}
	}
	for _, tool := range b.tools {
		if err := registrar.Tool(tool); err != nil {
			return nil, err
		}
	}
	for _, contribution := range b.middleware {
		if err := registrar.Middleware(contribution.middleware, contribution.orders...); err != nil {
			return nil, err
		}
	}
	for _, prompt := range b.prompts {
		if err := registrar.SystemPrompt(prompt); err != nil {
			return nil, err
		}
	}
	contributions := make([]PluginAssemblySnapshot, 0, len(b.plugins))
	for index, plugin := range b.plugins {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		contribution := PluginAssemblySnapshot{Name: identities[index]}
		err := plugin.Register(ctx, &Registrar{registration: registration, contribution: &contribution})
		if err == nil {
			err = ctx.Err()
		}
		if err != nil {
			return nil, fmt.Errorf("register plugin %q: %w", identities[index], err)
		}
		contributions = append(contributions, contribution)
	}
	seal()
	sort.SliceStable(registration.middleware, func(i, j int) bool {
		return registration.middleware[i].order < registration.middleware[j].order
	})
	snapshot := AgentAssemblySnapshot{Plugins: contributions, Commands: append([]agentabi.Command(nil), registration.commands...)}
	for _, tool := range registration.tools {
		snapshot.Tools = append(snapshot.Tools, tool.Name())
	}
	var middleware []abi.Middleware
	for _, contribution := range registration.middleware {
		snapshot.Middleware = append(snapshot.Middleware, contribution.middleware.Name())
		middleware = append(middleware, contribution.middleware)
	}
	baseline := cloneSnapshot(snapshot)
	baseDefinitions := toolDefinitions(registration.tools)
	for index, plugin := range b.plugins {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		if finalizer, ok := plugin.(Finalizer); ok {
			lateTools, err := finalizer.Finalize(ctx, LateContext{Bus: handle, Workspace: b.workspace, Session: conversationView{reader: b.session}, Model: b.model, ToolDefinitions: cloneToolDefinitions(baseDefinitions), Assembly: cloneSnapshot(baseline)})
			if err == nil {
				err = ctx.Err()
			}
			if err != nil {
				return nil, fmt.Errorf("finalize plugin %q: %w", identities[index], err)
			}
			for _, tool := range lateTools {
				if err := registration.addTool(tool); err != nil {
					return nil, fmt.Errorf("finalize plugin %q: %w", identities[index], err)
				}
				snapshot.Tools = append(snapshot.Tools, tool.Name())
				snapshot.Plugins[index].FinalizedTools = append(snapshot.Plugins[index].FinalizedTools, tool.Name())
			}
		}
	}
	state, err := abi.NewAgentState(b.model, b.session, registration.tools)
	if err != nil {
		return nil, err
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	return &Agent{loop: b.loop, config: kernel.RunConfig{State: state, Middleware: middleware, SystemPrompt: strings.Join(registration.prompts, "\n\n"), MaxIterations: b.maxIterations, MaxResponseBytes: b.maxResponseBytes, Workspace: b.workspace, Bus: handle, ContextSystem: registration.contextSystem, ContextSource: registration.contextSource, MaxConcurrentTools: b.maxConcurrentTools, ModelConfig: wire.CloneModelConfig(b.modelConfig)}, snapshot: snapshot, bus: handle}, nil
}

type Agent struct {
	loop     kernel.AgentLoop
	config   kernel.RunConfig
	snapshot AgentAssemblySnapshot
	bus      bus.Handle
	running  atomic.Bool
}

func (agent *Agent) Run(ctx context.Context, input []wire.Message, emit func(abi.Event) error) error {
	return agent.RunWithConfig(ctx, input, agent.config.ModelConfig, emit)
}

// RunWithConfig replaces model settings for this invocation only. The assembled
// default and active tool registry remain unchanged for later invocations.
func (agent *Agent) RunWithConfig(ctx context.Context, input []wire.Message, config wire.ModelConfig, emit func(abi.Event) error) error {
	if !agent.running.CompareAndSwap(false, true) {
		return ErrRunInProgress
	}
	defer agent.running.Store(false)
	runConfig := agent.config
	runConfig.Middleware = append([]abi.Middleware(nil), agent.config.Middleware...)
	runConfig.ContextSystem = cloneContextSystem(agent.config.ContextSystem)
	runConfig.ModelConfig = wire.CloneModelConfig(config)
	return agent.loop.Run(ctx, runConfig, wire.CloneMessages(input), emit)
}
func (agent *Agent) Snapshot() AgentAssemblySnapshot { return cloneSnapshot(agent.snapshot) }
func (agent *Agent) Bus() bus.Handle                 { return agent.bus }
func (agent *Agent) Session() abi.ConversationLog    { return agent.config.State.Session() }
func (agent *Agent) State() *abi.AgentState          { return agent.config.State }

func cloneSnapshot(snapshot AgentAssemblySnapshot) AgentAssemblySnapshot {
	copy := snapshot
	copy.Tools = append([]string(nil), snapshot.Tools...)
	copy.Middleware = append([]string(nil), snapshot.Middleware...)
	copy.Commands = append([]agentabi.Command(nil), snapshot.Commands...)
	copy.Plugins = append([]PluginAssemblySnapshot(nil), snapshot.Plugins...)
	for i := range copy.Plugins {
		copy.Plugins[i].Tools = append([]string(nil), snapshot.Plugins[i].Tools...)
		copy.Plugins[i].FinalizedTools = append([]string(nil), snapshot.Plugins[i].FinalizedTools...)
		copy.Plugins[i].Middleware = append([]string(nil), snapshot.Plugins[i].Middleware...)
		copy.Plugins[i].Commands = append([]string(nil), snapshot.Plugins[i].Commands...)
	}
	return copy
}

func toolDefinitions(tools []abi.Tool) []wire.ToolDefinition {
	result := make([]wire.ToolDefinition, 0, len(tools))
	for _, tool := range tools {
		result = append(result, wire.ToolDefinition{Name: tool.Name(), Description: tool.Description(), Parameters: append([]byte(nil), tool.ParametersSchema()...)})
	}
	return result
}
func cloneToolDefinitions(definitions []wire.ToolDefinition) []wire.ToolDefinition {
	result := append([]wire.ToolDefinition(nil), definitions...)
	for i := range result {
		result[i].Parameters = append([]byte(nil), result[i].Parameters...)
	}
	return result
}

func nilContribution(value any) bool {
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
