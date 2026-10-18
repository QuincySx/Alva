// INPUT: context, crypto/sha256, encoding/hex/json, errors, fmt, reflect, sort, sync, unicode/utf8, kernel/abi
// OUTPUT: PermissionManager, PermissionMode, ApprovalHandler, ApprovalRequest, ApprovalDecision
// POS: [L3 capability library] Session-scoped fail-closed HITL decisions with explicit cancellation and bounded authority.
package security

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"reflect"
	"sort"
	"sync"
	"unicode/utf8"

	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
)

type PermissionMode string

const (
	Ask         PermissionMode = "ask"
	AcceptEdits PermissionMode = "accept_edits"
	Plan        PermissionMode = "plan"
)

type DecisionScope string

const (
	Once    DecisionScope = "once"
	Session DecisionScope = "session"
)

var ErrPermissionDenied = errors.New("permission denied")
var ErrApprovalRequired = errors.New("approval required but no handler is available")

type ApprovalRequest struct {
	ID, SessionID, RunID, CallID, ToolName string
	Effect                                 abi.Effect
	InputSummary                           string
	ArgumentsSHA256                        string
}
type ApprovalDecision struct {
	Allowed bool
	Scope   DecisionScope
}
type ApprovalHandler interface {
	Approve(context.Context, ApprovalRequest) (ApprovalDecision, error)
}
type ApprovalFunc func(context.Context, ApprovalRequest) (ApprovalDecision, error)

func (fn ApprovalFunc) Approve(ctx context.Context, request ApprovalRequest) (ApprovalDecision, error) {
	if fn == nil {
		return ApprovalDecision{}, ErrApprovalRequired
	}
	return fn(ctx, request)
}

type PermissionManager struct {
	mu           sync.RWMutex
	mode         PermissionMode
	handler      ApprovalHandler
	cache        map[string]bool
	flights      map[string]*approvalFlight
	serial       uint64
	revision     uint64
	generation   map[string]uint64
	sessionModes map[string]PermissionMode
	rules        *PermissionRules
	rulesErr     error
}
type approvalFlight struct {
	done      chan struct{}
	err       error
	sessionID string
	serial    uint64
	complete  bool
}

func NewPermissionManager(mode PermissionMode, handler ApprovalHandler, rules ...PermissionRules) *PermissionManager {
	manager := &PermissionManager{mode: mode, handler: handler, cache: make(map[string]bool), flights: make(map[string]*approvalFlight), generation: make(map[string]uint64), sessionModes: make(map[string]PermissionMode)}
	if len(rules) > 1 {
		manager.rulesErr = errors.New("at most one permission rules snapshot permitted")
	} else if len(rules) == 1 {
		owned := cloneRules(rules[0])
		manager.rulesErr = owned.Validate()
		manager.rules = &owned
	}
	return manager
}
func (manager *PermissionManager) Mode() PermissionMode {
	manager.mu.RLock()
	defer manager.mu.RUnlock()
	return manager.mode
}

// ModeForSession returns the trusted host's explicit per-session override or
// the base mode. This map does NOT establish ancestry or a parent grant ceiling.
func (manager *PermissionManager) ModeForSession(sessionID string) PermissionMode {
	manager.mu.RLock()
	defer manager.mu.RUnlock()
	return manager.modeForSession(sessionID)
}
func (manager *PermissionManager) modeForSession(sessionID string) PermissionMode {
	if mode, ok := manager.sessionModes[sessionID]; ok {
		return mode
	}
	return manager.mode
}

// SetSessionMode is host control, not a model tool or implicit approval.
// Products must independently approve policy changes and enforce the directly
// authorized parent's ceiling before calling it. Only this session's decisions
// and in-flight approvals are invalidated; siblings retain their authority.
func (manager *PermissionManager) SetSessionMode(sessionID string, mode PermissionMode) error {
	if sessionID == "" || len(sessionID) > 512 || !utf8.ValidString(sessionID) {
		return errors.New("invalid permission session identity")
	}
	if mode != Ask && mode != AcceptEdits && mode != Plan {
		return errors.New("unknown permission mode")
	}
	manager.mu.Lock()
	defer manager.mu.Unlock()
	if _, ok := manager.sessionModes[sessionID]; !ok && len(manager.sessionModes) >= 4096 {
		return errors.New("permission session mode budget exceeded")
	}
	manager.clearSession(sessionID)
	manager.sessionModes[sessionID] = mode
	return nil
}

// SetMode explicitly resets the complete manager policy, including all
// per-session overrides and prior cached decisions. It is not a session setter.
func (manager *PermissionManager) SetMode(mode PermissionMode) error {
	if mode != Ask && mode != AcceptEdits && mode != Plan {
		return errors.New("unknown permission mode")
	}
	manager.mu.Lock()
	manager.mode = mode
	manager.revision++
	manager.flights = make(map[string]*approvalFlight)
	manager.cache = make(map[string]bool)
	manager.sessionModes = make(map[string]PermissionMode)
	manager.mu.Unlock()
	return nil
}
func (manager *PermissionManager) ClearSession(sessionID string) {
	manager.mu.Lock()
	defer manager.mu.Unlock()
	manager.clearSession(sessionID)
}
func (manager *PermissionManager) clearSession(sessionID string) {
	delete(manager.sessionModes, sessionID)
	manager.generation[sessionID]++
	prefix := fmt.Sprintf("%d:%s", len(sessionID), sessionID)
	for key := range manager.cache {
		if len(key) >= len(prefix) && key[:len(prefix)] == prefix {
			delete(manager.cache, key)
		}
	}
	for key, flight := range manager.flights {
		if flight.sessionID == sessionID {
			delete(manager.flights, key)
		}
	}
}
func (manager *PermissionManager) Authorize(ctx context.Context, request ApprovalRequest) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	if request.SessionID == "" || request.ToolName == "" {
		return errors.New("approval session/tool identity missing")
	}
	for _, value := range []string{request.ID, request.SessionID, request.RunID, request.CallID, request.ToolName, request.Effect.Description} {
		if !utf8.ValidString(value) {
			return errors.New("approval identity/effect must be valid UTF-8")
		}
	}
	for _, claim := range request.Effect.Resources {
		if !utf8.ValidString(claim.Resource) {
			return errors.New("approval resource must be valid UTF-8")
		}
	}
	if len(request.InputSummary) > 256<<10 || !utf8.ValidString(request.InputSummary) {
		return errors.New("approval input summary must be valid UTF-8 within 256 KiB")
	}
	if request.ArgumentsSHA256 != "" {
		decoded, err := hex.DecodeString(request.ArgumentsSHA256)
		if err != nil || len(decoded) != sha256.Size {
			return errors.New("invalid approval arguments hash")
		}
	}
	if manager.rulesErr != nil {
		return errors.Join(ErrPermissionDenied, manager.rulesErr)
	}
	manager.mu.RLock()
	mode, handler, revision := manager.modeForSession(request.SessionID), manager.handler, manager.revision
	generation := manager.generation[request.SessionID]
	key := fmt.Sprintf("%d:%s%s", len(request.SessionID), request.SessionID, request.ToolName)
	cached, exists := manager.cache[key]
	manager.mu.RUnlock()
	if mode != Ask && mode != AcceptEdits && mode != Plan {
		return ErrPermissionDenied
	}
	if mode == Plan && request.Effect.Kind != abi.EffectRead {
		return ErrPermissionDenied
	}
	explicitAsk := false
	if manager.rules != nil {
		decision, matched := manager.rules.Match(request.ToolName, request.InputSummary)
		if matched {
			switch decision {
			case RulesDeny:
				return ErrPermissionDenied
			case RulesAllow:
				return nil
			case RulesAsk:
				explicitAsk = true
			}
		}
	}
	if !explicitAsk && (request.Effect.Kind == abi.EffectRead || (mode == AcceptEdits && request.Effect.Kind == abi.EffectWrite)) {
		return nil
	}
	if exists && !explicitAsk {
		if cached {
			return nil
		}
		return ErrPermissionDenied
	}
	if nilImplementation(handler) {
		return ErrApprovalRequired
	}
	request.Effect.Resources = append([]abi.ResourceClaim(nil), request.Effect.Resources...)
	flightKey := ""
	var flight *approvalFlight
	// Requests without actual execution identity remain usable for standalone
	// session policy, but never gain invocation-scoped reuse authority.
	if request.RunID != "" && request.CallID != "" {
		flightKey = approvalFingerprint(request)
		manager.mu.Lock()
		if manager.revision != revision || manager.generation[request.SessionID] != generation {
			manager.mu.Unlock()
			return ErrPermissionDenied
		}
		if existing := manager.flights[flightKey]; existing != nil {
			manager.mu.Unlock()
			select {
			case <-ctx.Done():
				return ctx.Err()
			case <-existing.done:
				if err := ctx.Err(); err != nil {
					return err
				}
				manager.mu.RLock()
				valid := manager.revision == revision && manager.generation[request.SessionID] == generation && manager.flights[flightKey] == existing
				manager.mu.RUnlock()
				if !valid {
					return ErrPermissionDenied
				}
				return existing.err
			}
		}
		if len(manager.flights) >= 4096 {
			oldestKey := ""
			oldest := uint64(^uint64(0))
			for candidate, value := range manager.flights {
				if value.complete && value.serial < oldest {
					oldestKey, oldest = candidate, value.serial
				}
			}
			if oldestKey == "" {
				manager.mu.Unlock()
				return errors.New("approval invocation budget exceeded")
			}
			delete(manager.flights, oldestKey)
		}
		manager.serial++
		flight = &approvalFlight{done: make(chan struct{}), sessionID: request.SessionID, serial: manager.serial}
		manager.flights[flightKey] = flight
		manager.mu.Unlock()
	}
	decision, approvalErr := handler.Approve(ctx, request)
	approvalErr = errors.Join(approvalErr, ctx.Err())
	if approvalErr == nil && decision.Scope != Once && decision.Scope != Session {
		approvalErr = errors.New("invalid approval decision scope")
	}
	manager.mu.Lock()
	invalidated := manager.revision != revision || manager.generation[request.SessionID] != generation || (flight != nil && manager.flights[flightKey] != flight)
	if invalidated {
		approvalErr = errors.Join(approvalErr, ErrPermissionDenied)
	}
	if approvalErr == nil && decision.Scope == Session {
		manager.cache[key] = decision.Allowed
	}
	if approvalErr == nil && !decision.Allowed {
		approvalErr = ErrPermissionDenied
	}
	if flight != nil {
		flight.err = approvalErr
		flight.complete = true
		close(flight.done)
		if approvalErr != nil && !errors.Is(approvalErr, ErrPermissionDenied) {
			if manager.flights[flightKey] == flight {
				delete(manager.flights, flightKey)
			}
		}
	}
	manager.mu.Unlock()
	return approvalErr
}

func approvalFingerprint(request ApprovalRequest) string {
	effect := request.Effect
	effect.Resources = append([]abi.ResourceClaim(nil), effect.Resources...)
	sort.Slice(effect.Resources, func(i, j int) bool {
		if effect.Resources[i].Resource == effect.Resources[j].Resource {
			return effect.Resources[i].Access < effect.Resources[j].Access
		}
		return effect.Resources[i].Resource < effect.Resources[j].Resource
	})
	encoded, _ := json.Marshal(struct {
		SessionID, RunID, CallID, ToolName string
		InputSummary, ArgumentsSHA256      string
		Effect                             abi.Effect
	}{request.SessionID, request.RunID, request.CallID, request.ToolName, request.InputSummary, request.ArgumentsSHA256, effect})
	hash := sha256.Sum256(encoded)
	return hex.EncodeToString(hash[:])
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
