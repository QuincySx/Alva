// INPUT: Explicit ordinary-session schedule drafts, pinned routes and normal Hub execution owner
// OUTPUT: Paused-first automation facts, separate durable contract and atomic scheduled Turn admission
// POS: [L7 Hub product] Pure L7 product scheduling vocabulary; no timers, OS service or SDK policy in the core
package hub

import (
	"context"
	"errors"
	"strings"
	"unicode/utf8"
)

type AutomationDraft struct {
	Title           string `json:"title"`
	Prompt          string `json:"prompt"`
	IntervalMinutes int64  `json:"interval_minutes"`
}

func (d AutomationDraft) Validate() error {
	if !utf8.ValidString(d.Title) || strings.TrimSpace(d.Title) == "" || utf8.RuneCountInString(d.Title) > 120 {
		return errors.New("automation title requires 1..120 characters")
	}
	if !utf8.ValidString(d.Prompt) || strings.TrimSpace(d.Prompt) == "" || len(d.Prompt) > 16<<10 {
		return errors.New("automation prompt requires 1..16384 UTF-8 bytes")
	}
	if d.IntervalMinutes < 5 || d.IntervalMinutes > 10080 {
		return errors.New("automation interval requires 5..10080 minutes")
	}
	return nil
}

type Automation struct {
	ID        string `json:"id"`
	SessionID string `json:"session_id"`
	AutomationDraft
	Enabled       bool   `json:"enabled"`
	Revision      uint64 `json:"revision"`
	BranchID      string `json:"branch_id"`
	Target        Target `json:"target"`
	NextRunAt     int64  `json:"next_run_at,omitempty"`
	LastRunAt     int64  `json:"last_run_at,omitempty"`
	LastStatus    string `json:"last_status"`
	LastError     string `json:"last_error,omitempty"`
	RunningTurnID string `json:"running_turn_id,omitempty"`
}

// An index candidate grants no execution: the claim must recheck its revision,
// due time, ordinary-session scope, current route and exact stored prompt.
type AutomationTick struct {
	ID        string `json:"id"`
	SessionID string `json:"session_id"`
	Revision  uint64 `json:"revision"`
}

var ErrAutomationStale = errors.New("automation changed, paused, claimed or not due; refresh before acting")
var ErrAutomationSkipped = errors.New("automation tick skipped; no catch-up or queued execution")
var ErrAutomationRouteChanged = errors.New("automation paused because its route or collaboration scope changed")

// This separate capability keeps schedule mutation out of the general Store.
type AutomationStore interface {
	ListAutomations(context.Context, string) ([]Automation, error)
	Automation(context.Context, string, string) (Automation, error)
	CreateAutomation(context.Context, string, AutomationDraft) (Automation, error)
	UpdateAutomation(context.Context, string, string, uint64, AutomationDraft) (Automation, error)
	SetAutomationEnabled(context.Context, string, string, uint64, bool, bool) (Automation, error)
	DeleteAutomation(context.Context, string, string, uint64) error
	DueAutomations(context.Context, int64) ([]AutomationTick, error)
	ClaimAutomation(context.Context, AutomationTick, int64) (Claim, error)
	SkipBusyAutomation(context.Context, AutomationTick, int64) error
	PauseInvalidAutomation(context.Context, AutomationTick) error
}

func (r *Runtime) StartAutomation(owner, admission context.Context, tick AutomationTick, at int64) (string, <-chan error, error) {
	store, ok := r.store.(AutomationStore)
	if !ok {
		return "", nil, errors.New("product store has no durable automations")
	}
	return r.start(owner, tick.SessionID, func(ctx context.Context) (Claim, error) {
		// Admission cancellation ends only this transaction. After commit the
		// ordinary Turn remains owned by Runtime, not by its short-lived timer
		// query/request. Runtime.Close can also cancel an in-flight admission.
		claimContext, cancel := context.WithCancel(admission)
		stop := context.AfterFunc(ctx, cancel)
		defer stop()
		defer cancel()
		if err := admission.Err(); err != nil {
			return Claim{}, err
		}
		return store.ClaimAutomation(claimContext, tick, at)
	})
}
