// INPUT: encoding/json, errors, fmt, reflect, sync, unicode/utf8, kernel/bus, llm/wire
// OUTPUT: AgentState, RuntimeContext, ToolRegistry, Extensions, typed extension operations
// POS: [L1 kernel contracts] Actual mutable model/tool/log runtime state separated from immutable loop configuration.
package abi

import (
	"encoding/json"
	"errors"
	"fmt"
	"github.com/QuincySx/Alva/sdk/kernel/bus"
	"github.com/QuincySx/Alva/sdk/llm/wire"
	"reflect"
	"sync"
	"unicode/utf8"
)

type RuntimeContext struct {
	// InvocationModel is captured after all BeforeModel hooks, before the
	// wrapper chain. It identifies this invocation's actual implementation;
	// State.SetModel during wrappers affects only the next invocation. It is
	// nil outside ModelFunc/WrapModel, not an authorization policy or setter.
	InvocationModel LanguageModel
	// InvocationSessionID is the log actually bound to this model/preparation
	// round, even if State.SetSession schedules a different log for the next
	// round. It is empty in run-wide hooks and is identity, not a credential.
	InvocationSessionID string
	RunID               string
	State               *AgentState
	Workspace           string
	Capabilities        bus.Handle
}
type AgentState struct {
	mu         sync.RWMutex
	model      LanguageModel
	session    ConversationLog
	tools      *ToolRegistry
	extensions *Extensions
}

func NewAgentState(model LanguageModel, session ConversationLog, tools []Tool) (*AgentState, error) {
	if nilImplementation(model) || nilImplementation(session) || session.ID() == "" {
		return nil, errors.New("model and identified conversation log required")
	}
	registry, err := NewToolRegistry(tools)
	if err != nil {
		return nil, err
	}
	return &AgentState{model: model, session: session, tools: registry, extensions: NewExtensions()}, nil
}
func (state *AgentState) Model() LanguageModel {
	state.mu.RLock()
	defer state.mu.RUnlock()
	return state.model
}
func (state *AgentState) SetModel(model LanguageModel) error {
	if nilImplementation(model) {
		return errors.New("model required")
	}
	state.mu.Lock()
	state.model = model
	state.mu.Unlock()
	return nil
}
func (state *AgentState) Session() ConversationLog {
	state.mu.RLock()
	defer state.mu.RUnlock()
	return state.session
}
func (state *AgentState) SetSession(session ConversationLog) error {
	if nilImplementation(session) || session.ID() == "" {
		return errors.New("identified conversation log required")
	}
	state.mu.Lock()
	state.session = session
	state.mu.Unlock()
	return nil
}
func (state *AgentState) Tools() *ToolRegistry    { return state.tools }
func (state *AgentState) Extensions() *Extensions { return state.extensions }

type ToolRegistrySnapshot struct {
	Version     uint64
	Tools       []Tool
	Definitions []wire.ToolDefinition
}
type ToolRegistry struct {
	mu      sync.RWMutex
	tools   []Tool
	indices map[string]int
	version uint64
}

func NewToolRegistry(tools []Tool) (*ToolRegistry, error) {
	registry := &ToolRegistry{indices: make(map[string]int)}
	for _, tool := range tools {
		if err := registry.Register(tool); err != nil {
			return nil, err
		}
	}
	return registry, nil
}
func validateTool(tool Tool) error {
	if nilImplementation(tool) || tool.Name() == "" || !utf8.ValidString(tool.Name()) || !utf8.ValidString(tool.Description()) || !wire.ValidJSON(tool.ParametersSchema()) {
		return errors.New("tool identity or schema invalid")
	}
	return nil
}
func (registry *ToolRegistry) Register(tool Tool) error {
	if err := validateTool(tool); err != nil {
		return err
	}
	name := tool.Name()
	registry.mu.Lock()
	defer registry.mu.Unlock()
	if _, exists := registry.indices[name]; exists {
		return fmt.Errorf("duplicate tool %q", name)
	}
	registry.indices[name] = len(registry.tools)
	registry.tools = append(registry.tools, tool)
	registry.version++
	return nil
}
func (registry *ToolRegistry) Replace(tool Tool) (Tool, error) {
	if err := validateTool(tool); err != nil {
		return nil, err
	}
	name := tool.Name()
	registry.mu.Lock()
	defer registry.mu.Unlock()
	index, exists := registry.indices[name]
	if !exists {
		return nil, fmt.Errorf("tool %q is not registered", name)
	}
	old := registry.tools[index]
	registry.tools[index] = tool
	registry.version++
	return old, nil
}
func (registry *ToolRegistry) Remove(name string) bool {
	registry.mu.Lock()
	defer registry.mu.Unlock()
	index, exists := registry.indices[name]
	if !exists {
		return false
	}
	registry.tools = append(registry.tools[:index], registry.tools[index+1:]...)
	delete(registry.indices, name)
	for key, position := range registry.indices {
		if position > index {
			registry.indices[key] = position - 1
		}
	}
	registry.version++
	return true
}
func (registry *ToolRegistry) Get(name string) (Tool, bool) {
	registry.mu.RLock()
	defer registry.mu.RUnlock()
	index, exists := registry.indices[name]
	if !exists {
		return nil, false
	}
	return registry.tools[index], true
}
func (registry *ToolRegistry) Snapshot() ToolRegistrySnapshot {
	registry.mu.RLock()
	snapshot := ToolRegistrySnapshot{Version: registry.version, Tools: append([]Tool(nil), registry.tools...)}
	registry.mu.RUnlock()
	for _, tool := range snapshot.Tools {
		snapshot.Definitions = append(snapshot.Definitions, wire.ToolDefinition{Name: tool.Name(), Description: tool.Description(), Parameters: append(json.RawMessage(nil), tool.ParametersSchema()...)})
	}
	return snapshot
}

type Extensions struct {
	mu     sync.RWMutex
	values map[reflect.Type]any
}

func NewExtensions() *Extensions { return &Extensions{values: make(map[reflect.Type]any)} }
func PutExtension[T any](extensions *Extensions, value T) {
	extensions.mu.Lock()
	defer extensions.mu.Unlock()
	extensions.values[reflect.TypeFor[T]()] = value
}
func GetExtension[T any](extensions *Extensions) (T, bool) {
	extensions.mu.RLock()
	defer extensions.mu.RUnlock()
	var zero T
	value, ok := extensions.values[reflect.TypeFor[T]()]
	if !ok {
		return zero, false
	}
	typed, ok := value.(T)
	return typed, ok
}
func DeleteExtension[T any](extensions *Extensions) {
	extensions.mu.Lock()
	delete(extensions.values, reflect.TypeFor[T]())
	extensions.mu.Unlock()
}

// Interface equality alone cannot reject a typed-nil executable implementation.
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
