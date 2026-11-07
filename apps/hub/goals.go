// INPUT: context, immutable goal metadata and explicit project/session ownership.
// OUTPUT: Goal facts, idempotent creation receipts and separate scope-bound listing capabilities.
// POS: [L7 Hub product] Goals describe desired outcomes; creation never starts an execution or changes permissions.
package hub

import (
	"context"
	"errors"
	"fmt"
	"path/filepath"
	"strings"
	"unicode"
	"unicode/utf8"
)

var ErrInvalidGoal = errors.New("invalid goal")
var ErrGoalConflict = errors.New("goal request changed; use a new request identity")
var ErrGoalScope = errors.New("goal belongs to a different project or session")

type GoalScopeKind string

const (
	GoalProjectScope GoalScopeKind = "project"
	GoalSessionScope GoalScopeKind = "session"
)

// GoalScope contains the saved canonical workspace, never a filesystem grant.
// A session scope additionally belongs to exactly one product session.
type GoalScope struct {
	Kind      GoalScopeKind `json:"kind"`
	Workspace string        `json:"workspace"`
	SessionID string        `json:"session_id,omitempty"`
}

func (s GoalScope) Validate() error {
	if !goalText(s.Workspace, 4096, false) || !filepath.IsAbs(s.Workspace) || filepath.Clean(s.Workspace) != s.Workspace {
		return fmt.Errorf("%w: canonical absolute workspace required", ErrInvalidGoal)
	}
	if s.Kind != GoalProjectScope && s.Kind != GoalSessionScope || s.Kind == GoalProjectScope && s.SessionID != "" || s.Kind == GoalSessionScope && !goalText(s.SessionID, 256, false) {
		return fmt.Errorf("%w: explicit project or session scope required", ErrInvalidGoal)
	}
	return nil
}

type GoalCreate struct {
	RequestID          string    `json:"request_id"`
	Name               string    `json:"name"`
	CompletionCriteria string    `json:"completion_criteria"`
	Scope              GoalScope `json:"scope"`
}

func (r GoalCreate) Validate() error {
	if !goalText(r.RequestID, 128, false) || !goalText(r.Name, 256, false) || !goalText(r.CompletionCriteria, 8192, true) {
		return fmt.Errorf("%w: request identity, name (256 bytes) and completion criteria (8192 bytes) required", ErrInvalidGoal)
	}
	return r.Scope.Validate()
}

func goalText(value string, max int, multiline bool) bool {
	if value == "" || strings.TrimSpace(value) == "" || len(value) > max || !utf8.ValidString(value) {
		return false
	}
	for _, char := range value {
		if unicode.IsControl(char) && !(multiline && (char == '\n' || char == '\r' || char == '\t')) {
			return false
		}
	}
	return true
}

// Goal is an immutable revision of a desired outcome. It is neither a Team nor
// a running task. Revisions pin references even if editing is added later.
type Goal struct {
	ID                 string    `json:"id"`
	Revision           uint64    `json:"revision"`
	RequestID          string    `json:"request_id"`
	Name               string    `json:"name"`
	CompletionCriteria string    `json:"completion_criteria"`
	Scope              GoalScope `json:"scope"`
	CreatedAt          int64     `json:"created_at"`
}

func (g Goal) Validate() error {
	if !goalText(g.ID, 128, false) || g.Revision != 1 || g.CreatedAt <= 0 {
		return fmt.Errorf("%w: stored identity, revision or timestamp", ErrInvalidGoal)
	}
	return g.Creation().Validate()
}

func (g Goal) Creation() GoalCreate {
	return GoalCreate{RequestID: g.RequestID, Name: g.Name, CompletionCriteria: g.CompletionCriteria, Scope: g.Scope}
}

type GoalReceipt struct {
	Goal     Goal `json:"goal"`
	Replayed bool `json:"replayed"`
}

// GoalStore atomically commits one fact per RequestID. Identical retries return
// the original receipt; changed requests fail without changing any fact.
type GoalStore interface {
	CreateGoal(context.Context, GoalCreate) (GoalReceipt, error)
	Goal(context.Context, string) (Goal, error)
	ListGoals(context.Context, GoalScope, int) ([]Goal, error)
}

// GoalCursor continues a descending immutable creation order in one exact scope.
// New goals do not shift an already opened page or hide older saved goals.
type GoalCursor struct {
	Created int64     `json:"created"`
	ID      string    `json:"id"`
	Scope   GoalScope `json:"scope"`
}

type GoalPage struct {
	Entries []Goal     `json:"entries"`
	Next    GoalCursor `json:"next"`
	HasMore bool       `json:"has_more"`
}

// GoalPageStore is the explicit capability for browsing every saved Goal.
// GoalStore's original bounded ListGoals contract remains available.
type GoalPageStore interface {
	PageGoals(context.Context, GoalScope, GoalCursor, int) (GoalPage, error)
}
