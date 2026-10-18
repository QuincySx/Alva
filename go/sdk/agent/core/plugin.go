// INPUT: context, errors, fmt, unicode/utf8, agent/abi, kernel/abi, kernel/bus, llm/wire
// OUTPUT: Plugin, Finalizer, Registrar, LateContext, assembly snapshots
// POS: [L2.5 SDK assembly] SDK contribution registration and read-only late wiring without harness defaults.
package core

import (
	"context"
	"errors"
	"fmt"
	"unicode/utf8"

	agentabi "github.com/QuincySx/Alva/go/sdk/agent/abi"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/kernel/bus"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type Plugin interface {
	Name() string
	Register(context.Context, *Registrar) error
}
type Finalizer interface {
	Finalize(context.Context, LateContext) ([]abi.Tool, error)
}

type PluginAssemblySnapshot struct {
	Name              string   `json:"name"`
	Tools             []string `json:"tools"`
	FinalizedTools    []string `json:"finalized_tools"`
	Middleware        []string `json:"middleware"`
	Commands          []string `json:"commands"`
	SystemPromptCount int      `json:"system_prompt_count"`
}

type AgentAssemblySnapshot struct {
	Plugins    []PluginAssemblySnapshot `json:"plugins"`
	Tools      []string                 `json:"tools"`
	Middleware []string                 `json:"middleware"`
	Commands   []agentabi.Command       `json:"commands"`
}

type registration struct {
	sealed          bool
	tools           []abi.Tool
	middleware      []middlewareContribution
	prompts         []string
	commands        []agentabi.Command
	toolNames       map[string]bool
	middlewareNames map[string]bool
	commandNames    map[string]bool
	writer          bus.Writer
	workspace       string
	contextSystem   *abi.ContextSystem
	contextSource   abi.ContextSource
}

type Registrar struct {
	registration *registration
	contribution *PluginAssemblySnapshot
}

func (r *Registrar) Tool(tool abi.Tool) error {
	if r.registration.sealed {
		return errors.New("registration sealed")
	}
	if err := r.registration.addTool(tool); err != nil {
		return err
	}
	if r.contribution != nil {
		r.contribution.Tools = append(r.contribution.Tools, tool.Name())
	}
	return nil
}

func (registration *registration) addTool(tool abi.Tool) error {
	if nilContribution(tool) || tool.Name() == "" || !utf8.ValidString(tool.Name()) || !utf8.ValidString(tool.Description()) {
		return errors.New("tool identity missing")
	}
	if !wire.ValidJSON(tool.ParametersSchema()) {
		return fmt.Errorf("invalid tool schema %q", tool.Name())
	}
	name := tool.Name()
	if registration.toolNames[name] {
		return fmt.Errorf("duplicate tool %q", name)
	}
	registration.toolNames[name] = true
	registration.tools = append(registration.tools, tool)
	return nil
}

// Middleware contributes one executable layer. Without an explicit order it
// shares the normal order (3000), preserving registration order among peers.
// Lower orders enter first; after hooks and wrapper returns unwind in reverse.
// Ordering belongs to this assembly, not to a method on the implementation.
func (r *Registrar) Middleware(middleware abi.Middleware, order ...int) error {
	if r.registration.sealed {
		return errors.New("registration sealed")
	}
	if nilContribution(middleware) || !validIdentity(middleware.Name()) {
		return errors.New("middleware identity missing")
	}
	if len(order) > 1 {
		return errors.New("middleware accepts at most one order")
	}
	if adapter, ok := middleware.(interface{ validateHook() error }); ok {
		if err := adapter.validateHook(); err != nil {
			return err
		}
	}
	name := middleware.Name()
	if r.registration.middlewareNames[name] {
		return fmt.Errorf("duplicate middleware %q", name)
	}
	r.registration.middlewareNames[name] = true
	value := 3000
	if len(order) == 1 {
		value = order[0]
	}
	r.registration.middleware = append(r.registration.middleware, middlewareContribution{middleware: middleware, order: value})
	if r.contribution != nil {
		r.contribution.Middleware = append(r.contribution.Middleware, name)
	}
	return nil
}

func (r *Registrar) SystemPrompt(prompt string) error {
	if r.registration.sealed {
		return errors.New("registration sealed")
	}
	if prompt != "" {
		r.registration.prompts = append(r.registration.prompts, prompt)
		if r.contribution != nil {
			r.contribution.SystemPromptCount++
		}
	}
	return nil
}

func (r *Registrar) Command(command agentabi.Command) error {
	if r.registration.sealed {
		return errors.New("registration sealed")
	}
	if !validIdentity(command.Name) || !utf8.ValidString(command.Description) || r.registration.commandNames[command.Name] {
		return errors.New("empty or duplicate command")
	}
	r.registration.commandNames[command.Name] = true
	r.registration.commands = append(r.registration.commands, command)
	if r.contribution != nil {
		r.contribution.Commands = append(r.contribution.Commands, command.Name)
	}
	return nil
}

func (r *Registrar) BusWriter() bus.Writer { return r.registration.writer }
func (r *Registrar) Bus() bus.Handle       { return r.registration.writer.Handle() }
func (r *Registrar) Workspace() string     { return r.registration.workspace }
func (r *Registrar) ContextSystem(system *abi.ContextSystem) error {
	if r.registration.sealed {
		return errors.New("registration sealed")
	}
	if system == nil || nilContribution(system.Hooks) || nilContribution(system.Handle) {
		return errors.New("context hooks and handle are required")
	}
	if system.Apply != nil && nilContribution(system.Apply) || system.Counter != nil && nilContribution(system.Counter) {
		return errors.New("typed-nil context executor or counter")
	}
	if r.registration.contextSystem != nil {
		return errors.New("duplicate context system")
	}
	r.registration.contextSystem = cloneContextSystem(system)
	return nil
}

func validIdentity(name string) bool { return name != "" && utf8.ValidString(name) }

// Own the descriptor; hook/handle/counter implementations retain their explicit
// shared identity and are responsible for their own mutable state and lifetime.
func cloneContextSystem(system *abi.ContextSystem) *abi.ContextSystem {
	if system == nil {
		return nil
	}
	copy := *system
	return &copy
}

// ContextSource contributes a read-only working view independently of context
// policy. It never replaces the complete conversation log or its authority.
func (r *Registrar) ContextSource(source abi.ContextSource) error {
	if r.registration.sealed {
		return errors.New("registration sealed")
	}
	if nilContribution(source) {
		return errors.New("context source is required")
	}
	if r.registration.contextSource != nil {
		return errors.New("duplicate context source")
	}
	r.registration.contextSource = source
	return nil
}

type LateContext struct {
	Bus             bus.Handle
	Workspace       string
	Session         abi.ConversationReader
	Model           abi.LanguageModel
	ToolDefinitions []wire.ToolDefinition
	Assembly        AgentAssemblySnapshot
}

// The wrapper is deliberate: merely narrowing the interface while passing the
// full store would still allow a finalizer to type-assert write authority back.
type conversationView struct{ reader abi.ConversationReader }

func (view conversationView) ID() string { return view.reader.ID() }
func (view conversationView) Messages(ctx context.Context) ([]wire.Message, error) {
	messages, err := view.reader.Messages(ctx)
	if err != nil {
		return nil, err
	}
	return wire.CloneMessages(messages), nil
}
