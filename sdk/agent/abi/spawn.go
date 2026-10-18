// INPUT: context, encoding/json, errors, sort, sync, time, kernel/abi, kernel/bus
// OUTPUT: ChildScopeConfig, ScopeSnapshot, SpawnCommunication, SpawnCommContext, SpawnCommHandle, CommunicationRegistry
// POS: [L2.3 agent contracts] Agent-level spawn lifecycle and opt-in communication vocabulary above the generic kernel.
package abi

import (
	"context"
	"encoding/json"
	"errors"
	kernel "github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/kernel/bus"
	"sort"
	"sync"
	"time"
)

type ChildScopeConfig struct {
	Role, SystemPrompt string
	Timeout            time.Duration
	MaxIterations      int
}
type ScopeSnapshot struct {
	ID, ParentID, Role, SessionID  string
	Depth, MaxDepth, ChildrenCount int
	Completed                      bool
}
type SpawnCommContext struct {
	ParentScopeID, ParentSessionID, ChildScopeID, ChildSessionID, Role string
	Bus                                                                bus.Handle
}
type SpawnResult struct {
	Text    string
	IsError bool
	Error   string
}
type OnChildComplete interface {
	Call(context.Context, SpawnResult) error
}
type ChildCompleteFunc func(context.Context, SpawnResult) error

func (fn ChildCompleteFunc) Call(ctx context.Context, result SpawnResult) error {
	if fn == nil {
		return errors.New("nil child completion callback")
	}
	return fn(ctx, result)
}

type SpawnCommHandle struct {
	Hooks      []kernel.ContextHooks
	OnComplete OnChildComplete
}
type SpawnCommunication interface {
	Kind() string
	Description() string
	ConfigSchema() json.RawMessage
	Attach(context.Context, SpawnCommContext, json.RawMessage) (SpawnCommHandle, error)
}
type CommunicationRegistry struct {
	mu       sync.RWMutex
	values   map[string]SpawnCommunication
	capacity int
}

func NewCommunicationRegistry(capacity int) (*CommunicationRegistry, error) {
	if capacity < 1 {
		return nil, errors.New("positive communication capacity required")
	}
	return &CommunicationRegistry{values: make(map[string]SpawnCommunication), capacity: capacity}, nil
}
func (registry *CommunicationRegistry) Register(communication SpawnCommunication) error {
	if nilImplementation(communication) {
		return errors.New("invalid spawn communication")
	}
	kind := communication.Kind()
	if kind == "" || !json.Valid(communication.ConfigSchema()) {
		return errors.New("invalid spawn communication")
	}
	registry.mu.Lock()
	defer registry.mu.Unlock()
	if _, exists := registry.values[kind]; exists {
		return errors.New("duplicate communication kind")
	}
	if len(registry.values) >= registry.capacity {
		return errors.New("spawn communication capacity exceeded")
	}
	registry.values[kind] = communication
	return nil
}
func (registry *CommunicationRegistry) Get(kind string) (SpawnCommunication, bool) {
	registry.mu.RLock()
	defer registry.mu.RUnlock()
	value, exists := registry.values[kind]
	return value, exists
}
func (registry *CommunicationRegistry) List() []SpawnCommunication {
	registry.mu.RLock()
	defer registry.mu.RUnlock()
	keys := make([]string, 0, len(registry.values))
	for key := range registry.values {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	values := make([]SpawnCommunication, 0, len(keys))
	for _, key := range keys {
		values = append(values, registry.values[key])
	}
	return values
}
