// INPUT: errors, sync
// OUTPUT: BoardRegistry, NewBoardRegistry
// POS: [L3 capability library] Scope-local board identity with acyclic parent links and genuinely reader-only upward views.
package scope

import (
	"errors"
	"sync"
)

type boardKey struct{ scope, id string }
type BoardRegistry struct {
	mu       sync.Mutex
	boards   map[boardKey]*Blackboard
	parents  map[string]string
	capacity int
	limits   BoardLimits
}

func NewBoardRegistry(capacity int, limits BoardLimits) (*BoardRegistry, error) {
	if capacity < 1 {
		return nil, errors.New("positive board registry capacity required")
	}
	if _, err := NewBlackboard(limits); err != nil {
		return nil, err
	}
	return &BoardRegistry{boards: make(map[boardKey]*Blackboard), parents: make(map[string]string), capacity: capacity, limits: limits}, nil
}
func (registry *BoardRegistry) SetParent(child, parent string) error {
	if child == "" || parent == "" || child == parent {
		return errors.New("invalid scope parent")
	}
	registry.mu.Lock()
	defer registry.mu.Unlock()
	if existing := registry.parents[child]; existing != "" && existing != parent {
		return errors.New("scope parent cannot be rebound")
	}
	if _, exists := registry.parents[child]; !exists && len(registry.parents) >= registry.capacity {
		return errors.New("scope link capacity exceeded")
	}
	for current := parent; current != ""; current = registry.parents[current] {
		if current == child {
			return errors.New("scope parent cycle")
		}
	}
	registry.parents[child] = parent
	return nil
}
func (registry *BoardRegistry) GetOrCreate(scopeID, boardID string) (*Blackboard, error) {
	if scopeID == "" || boardID == "" {
		return nil, errors.New("board scope/identity required")
	}
	registry.mu.Lock()
	defer registry.mu.Unlock()
	key := boardKey{scopeID, boardID}
	if board := registry.boards[key]; board != nil {
		return board, nil
	}
	if len(registry.boards) >= registry.capacity {
		return nil, errors.New("board registry capacity exceeded")
	}
	board, err := NewBlackboard(registry.limits)
	if err != nil {
		return nil, err
	}
	registry.boards[key] = board
	return board, nil
}
func (registry *BoardRegistry) ParentBoardFor(childID, boardID string) (BoardReader, bool) {
	registry.mu.Lock()
	defer registry.mu.Unlock()
	parent := registry.parents[childID]
	if parent == "" {
		return nil, false
	}
	board := registry.boards[boardKey{parent, boardID}]
	if board == nil {
		return nil, false
	}
	return boardView{board}, true
}
