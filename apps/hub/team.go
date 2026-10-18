// INPUT: context, filepath, durable product identities and engine outcomes
// OUTPUT: TeamRun/Worker/WorkerTask facts, resource claim algebra and TeamStore contract
// POS: [L7 Hub product] Product collaboration uses complete independent Hub sessions, not SDK native subagents
package hub

import (
	"context"
	"errors"
	"path/filepath"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/integration/engine"
)

type WorkerLifecycle string

const (
	WorkerActive     WorkerLifecycle = "active"
	WorkerHibernated WorkerLifecycle = "hibernated"
	WorkerArchived   WorkerLifecycle = "archived"
)

type TeamRun struct {
	ID            string `json:"id"`
	LeadSessionID string `json:"lead_session_id"`
	Status        string `json:"status"`
	CreatedAt     int64  `json:"created_at"`
}
type Worker struct {
	ID        string          `json:"id"`
	TeamID    string          `json:"team_id"`
	SessionID string          `json:"session_id"`
	Name      string          `json:"name"`
	Lifecycle WorkerLifecycle `json:"lifecycle"`
	CreatedAt int64           `json:"created_at"`
	Worktree  *WorkerWorktree `json:"worktree,omitempty"`
}
type TeamDetail struct {
	Team    TeamRun  `json:"team"`
	Workers []Worker `json:"workers"`
}

// Resource claims are scheduling intent, not filesystem grants or proof that
// an external engine obeys the SDK lock. Actual tools still use the shared lock;
// opaque external engines additionally acquire the whole-workspace lease.
type ResourceClaim struct {
	Path  string `json:"path"`
	Write bool   `json:"write"`
}

func ValidateTask(scope string, claims []ResourceClaim) error {
	if strings.TrimSpace(scope) == "" || len(scope) > 16<<10 || !utf8.ValidString(scope) || len(claims) > 16 {
		return errors.New("task requires 1..16 KiB scope and at most 16 resource claims")
	}
	for _, claim := range claims {
		if len(claim.Path) > 1024 || !utf8.ValidString(claim.Path) || (claim.Path != "." && !filepath.IsLocal(claim.Path)) || filepath.Clean(claim.Path) != claim.Path {
			return errors.New("resource path must be a clean local workspace-relative path")
		}
	}
	return nil
}
func ClaimsConflict(a, b []ResourceClaim) bool {
	for _, left := range a {
		for _, right := range b {
			if (left.Write || right.Write) && overlaps(filepath.Join("/", left.Path), filepath.Join("/", right.Path)) {
				return true
			}
		}
	}
	return false
}

const TaskQueued engine.Outcome = "queued"

type WorkerOutcome struct {
	Status         engine.Outcome `json:"status"`
	TurnID         string         `json:"turn_id"`
	Summary        string         `json:"summary"`
	ResultSequence uint64         `json:"result_sequence,omitempty"`
	ChangedFiles   []string       `json:"changed_files"`
	Verification   []string       `json:"verification"`
	Risks          []string       `json:"risks"`
	Processing     string         `json:"processing"`
}
type WorkerTask struct {
	ID               string          `json:"id"`
	Sequence         uint64          `json:"sequence"`
	TeamID           string          `json:"team_id"`
	WorkerID         string          `json:"worker_id"`
	Scope            string          `json:"scope"`
	Claims           []ResourceClaim `json:"claims"`
	Status           engine.Outcome  `json:"status"`
	TurnID           string          `json:"turn_id"`
	Outcome          *WorkerOutcome  `json:"outcome,omitempty"`
	CreatedAt        int64           `json:"created_at"`
	EndedAt          int64           `json:"ended_at"`
	RequiredBranchID string          `json:"required_branch_id,omitempty"`
}

// Drafts with native attachment references pin their exact saved branch at
// queue admission and claim. They cannot silently follow a later route switch.
type WorkerDraftStore interface {
	EnqueueWorkerOnBranch(context.Context, string, string, string, string, string, []ResourceClaim) (WorkerTask, error)
}
type WorkerTaskPage struct {
	Tasks   []WorkerTask `json:"tasks"`
	Next    uint64       `json:"next"`
	HasMore bool         `json:"has_more"`
}
type WorkerClaim struct {
	Task      WorkerTask
	Claim     *Claim
	BlockedBy []string
	Dormant   bool
}

// Every operation requires the actual lead and team IDs; a worker/native ID
// cannot select another team. Queue consumption and Turn admission are atomic.
type TeamStore interface {
	CreateTeam(context.Context, string) (TeamDetail, error)
	Team(context.Context, string, string) (TeamDetail, error)
	TeamForLead(context.Context, string) (TeamDetail, error)
	AddWorker(context.Context, string, string, string, Target) (Worker, error)
	SetWorkerLifecycle(context.Context, string, string, string, WorkerLifecycle) error
	EndTeam(context.Context, string, string) error
	EnqueueWorker(context.Context, string, string, string, string, []ResourceClaim) (WorkerTask, error)
	EditQueuedTask(context.Context, string, string, string, string, string, []ResourceClaim) error
	CancelQueuedTask(context.Context, string, string, string, string) error
	WorkerTasks(context.Context, string, string, string, uint64, int) (WorkerTaskPage, error)
	ClaimWorkerTask(context.Context, string, string, string) (WorkerClaim, error)
}
