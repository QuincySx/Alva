// INPUT: Durable product Worker identity, trusted managed-path binding and small SHA-indexed evidence
// OUTPUT: Pure worktree/evidence/adoption facts and owner-scoped product mutation contract
// POS: [L7 Hub product] Hub core owns product supervision only; no Git, filesystem, storage or native SDK dependency
package hub

import "context"

type WorktreeBinding struct {
	Version    int    `json:"version"`
	WorkerID   string `json:"worker_id"`
	Workspace  string `json:"shared_workspace"`
	Path       string `json:"path"`
	Branch     string `json:"branch"`
	BaseCommit string `json:"base_commit"`
}
type WorktreeChange struct {
	Path         string `json:"path"`
	BeforeSHA256 string `json:"before_sha256,omitempty"`
	AfterSHA256  string `json:"after_sha256,omitempty"`
	BeforeMode   string `json:"before_mode,omitempty"`
	AfterMode    string `json:"after_mode,omitempty"`
}
type WorktreeEvidence struct {
	ID            string           `json:"id"`
	WorkerID      string           `json:"worker_id"`
	BaseCommit    string           `json:"base_commit"`
	WorkerSHA256  string           `json:"worker_sha256"`
	SharedSHA256  string           `json:"shared_sha256"`
	PatchSHA256   string           `json:"patch_sha256"`
	PatchBytes    int              `json:"patch_bytes"`
	SnapshotBytes int              `json:"snapshot_bytes"`
	CreatedAt     int64            `json:"created_at"`
	Changes       []WorktreeChange `json:"changes"`
}

type WorktreeRemovalReview struct {
	WorkerSHA256 string `json:"worker_sha256"`
	IndexSHA256  string `json:"index_sha256"`
	Head         string `json:"head"`
	Files        int    `json:"files"`
	Bytes        int    `json:"bytes"`
	CreatedAt    int64  `json:"created_at"`
}
type WorkerWorktree struct {
	State    string                 `json:"state"` // provisioning | ready | provision_failed | provision_unknown | removing | removed | removal_unknown
	Binding  WorktreeBinding        `json:"binding"`
	Evidence *WorktreeEvidence      `json:"evidence,omitempty"`
	Adoption string                 `json:"adoption,omitempty"` // adopting | applied | unknown
	Problem  string                 `json:"problem,omitempty"`
	Revision uint64                 `json:"revision"`
	Removal  *WorktreeRemovalReview `json:"removal,omitempty"`
}

// Every write resolves the actual Lead/Team/Worker and idle native identity in
// the same transaction. No client-supplied native session or arbitrary target
// path can retarget an existing session. Beginning provisioning is one-way:
// failure preserves the new private target and never silently returns shared.
type WorktreeStore interface {
	BeginWorkerWorktree(context.Context, string, string, string) (Worker, error)
	FinishWorkerWorktree(context.Context, string, string, string, uint64, WorktreeBinding, string) (Worker, error)
	SaveWorkerEvidence(context.Context, string, string, string, uint64, WorktreeEvidence) (Worker, error)
	BeginWorkerAdoption(context.Context, string, string, string, uint64, string) (Worker, error)
	FinishWorkerAdoption(context.Context, string, string, string, uint64, string) (Worker, error)
	SaveWorkerRemovalReview(context.Context, string, string, string, uint64, WorktreeRemovalReview) (Worker, error)
	BeginWorkerRemoval(context.Context, string, string, string, uint64, string) (Worker, error)
	FinishWorkerRemoval(context.Context, string, string, string, uint64, bool) (Worker, error)
}
