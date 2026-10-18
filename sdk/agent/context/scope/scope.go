// INPUT: context, crypto/rand, encoding/hex, errors, sync, time, agent/abi, kernel/abi
// OUTPUT: SpawnScope, Config, NewRoot, SessionTracker
// POS: [L3 capability library] Spawn tree identity, direct-parent authority ceilings and lifecycle without executing or creating a host session.
package scope

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	agentabi "github.com/QuincySx/Alva/sdk/agent/abi"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"sync"
	"time"
	"unicode/utf8"
)

type Config struct {
	// SessionID binds an already-created root conversation; empty generates a
	// target ID. Child IDs remain generated and must be created by the factory.
	SessionID               string
	Timeout                 time.Duration
	MaxIterations, MaxDepth int
}
type SpawnScope struct {
	mu                                          sync.RWMutex
	id, parentID, sessionID, role, systemPrompt string
	depth, children                             int
	completed                                   bool
	model                                       abi.LanguageModel
	tools                                       []abi.Tool
	config                                      Config
}

func identity() (string, error) {
	var raw [16]byte
	if _, err := rand.Read(raw[:]); err != nil {
		return "", err
	}
	return hex.EncodeToString(raw[:]), nil
}
func NewRoot(model abi.LanguageModel, tools []abi.Tool, config Config) (*SpawnScope, error) {
	if model == nil || config.Timeout <= 0 || config.MaxIterations < 1 || config.MaxDepth < 0 {
		return nil, errors.New("valid model and scope limits required")
	}
	if _, err := abi.NewToolRegistry(tools); err != nil {
		return nil, err
	}
	id, err := identity()
	if err != nil {
		return nil, err
	}
	session := config.SessionID
	if !utf8.ValidString(session) || len(session) > 512 {
		return nil, errors.New("invalid root session identity")
	}
	if session == "" {
		session, err = identity()
		if err != nil {
			return nil, err
		}
	}
	return &SpawnScope{id: id, sessionID: session, role: "root", model: model, tools: append([]abi.Tool(nil), tools...), config: config}, nil
}

// SpawnChild requires explicit direct-parent grants; nil means no tools. A
// parent-bound `agent` tool is never inherited. Session-sensitive wrappers must
// be rebound by the product factory before child execution, not reused blindly.
func (parent *SpawnScope) SpawnChild(ctx context.Context, config agentabi.ChildScopeConfig, allowedToolNames []string) (*SpawnScope, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if config.Role == "" || config.Timeout < 0 || config.MaxIterations < 0 {
		return nil, errors.New("invalid child scope configuration")
	}
	parent.mu.Lock()
	defer parent.mu.Unlock()
	if parent.completed {
		return nil, errors.New("completed scope cannot spawn")
	}
	if parent.depth >= parent.config.MaxDepth {
		return nil, errors.New("scope depth exceeded")
	}
	if config.Timeout > parent.config.Timeout || config.MaxIterations > parent.config.MaxIterations {
		return nil, errors.New("child scope cannot exceed direct-parent execution budget")
	}
	id, err := identity()
	if err != nil {
		return nil, err
	}
	session, err := identity()
	if err != nil {
		return nil, err
	}
	limits := parent.config
	limits.SessionID = ""
	if config.Timeout > 0 {
		limits.Timeout = config.Timeout
	}
	if config.MaxIterations > 0 {
		limits.MaxIterations = config.MaxIterations
	}
	tools := filterTools(parent.tools, allowedToolNames)
	parent.children++
	return &SpawnScope{id: id, parentID: parent.id, sessionID: session, role: config.Role, systemPrompt: config.SystemPrompt, depth: parent.depth + 1, model: parent.model, tools: tools, config: limits}, nil
}
func (scope *SpawnScope) Snapshot() agentabi.ScopeSnapshot {
	scope.mu.RLock()
	defer scope.mu.RUnlock()
	return agentabi.ScopeSnapshot{ID: scope.id, ParentID: scope.parentID, Role: scope.role, SessionID: scope.sessionID, Depth: scope.depth, MaxDepth: scope.config.MaxDepth, ChildrenCount: scope.children, Completed: scope.completed}
}
func (scope *SpawnScope) Model() abi.LanguageModel { return scope.model }
func (scope *SpawnScope) SessionID() string        { return scope.sessionID }
func (scope *SpawnScope) Timeout() time.Duration   { return scope.config.Timeout }
func (scope *SpawnScope) MaxIterations() int       { return scope.config.MaxIterations }
func (scope *SpawnScope) SystemPrompt() string     { return scope.systemPrompt }
func (scope *SpawnScope) ToolsByNames(allowed []string) []abi.Tool {
	scope.mu.RLock()
	defer scope.mu.RUnlock()
	return filterTools(scope.tools, allowed)
}
func (scope *SpawnScope) ParentToolNames() []string {
	scope.mu.RLock()
	defer scope.mu.RUnlock()
	var names []string
	for _, tool := range scope.tools {
		if tool.Name() != "agent" {
			names = append(names, tool.Name())
		}
	}
	return names
}
func (scope *SpawnScope) Complete() { scope.mu.Lock(); scope.completed = true; scope.mu.Unlock() }
func filterTools(tools []abi.Tool, allowed []string) []abi.Tool {
	names := make(map[string]bool, len(allowed))
	for _, name := range allowed {
		names[name] = true
	}
	var result []abi.Tool
	for _, tool := range tools {
		if tool.Name() != "agent" && names[tool.Name()] {
			result = append(result, tool)
		}
	}
	return result
}

type SessionLink struct{ SessionID, ParentSessionID, ScopeID string }
type SessionTracker struct {
	mu       sync.RWMutex
	links    map[string]SessionLink
	capacity int
}

func NewSessionTracker(capacity int) (*SessionTracker, error) {
	if capacity < 1 {
		return nil, errors.New("positive session tracker capacity required")
	}
	return &SessionTracker{links: make(map[string]SessionLink), capacity: capacity}, nil
}
func (tracker *SessionTracker) Register(link SessionLink) error {
	if link.SessionID == "" || link.ScopeID == "" || link.SessionID == link.ParentSessionID {
		return errors.New("invalid tracked session identity")
	}
	tracker.mu.Lock()
	defer tracker.mu.Unlock()
	if _, exists := tracker.links[link.SessionID]; exists {
		return errors.New("session already tracked")
	}
	if len(tracker.links) >= tracker.capacity {
		return errors.New("session tracker capacity exceeded")
	}
	if link.ParentSessionID != "" {
		if _, exists := tracker.links[link.ParentSessionID]; !exists {
			return errors.New("parent session not tracked")
		}
	}
	tracker.links[link.SessionID] = link
	return nil
}
func (tracker *SessionTracker) Ancestors(sessionID string) ([]SessionLink, error) {
	tracker.mu.RLock()
	defer tracker.mu.RUnlock()
	var result []SessionLink
	for sessionID != "" {
		link, exists := tracker.links[sessionID]
		if !exists {
			return nil, errors.New("session not tracked")
		}
		result = append(result, link)
		sessionID = link.ParentSessionID
	}
	return result, nil
}
