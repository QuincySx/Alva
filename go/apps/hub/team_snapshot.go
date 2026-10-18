// INPUT: Committed Team/Worker/Task identities, product statuses and selected evidence sequences
// OUTPUT: Bounded TeamSnapshot handoff metadata without Worker transcripts or implicit execution
// POS: [L7 Hub product] Cross-engine continuity is a product index, not native-log permission or task success evidence
package hub

import "github.com/QuincySx/Alva/go/integration/engine"

type TeamSnapshot struct {
	Role          string               `json:"role"`
	TeamID        string               `json:"team_id"`
	LeadSessionID string               `json:"lead_session_id"`
	Status        string               `json:"status"`
	CapturedAt    int64                `json:"captured_at"`
	Workers       []TeamWorkerSnapshot `json:"workers"`
	Processing    string               `json:"processing"`
}
type TeamWorkerSnapshot struct {
	WorkerID    string          `json:"worker_id"`
	SessionID   string          `json:"session_id"`
	Lifecycle   WorkerLifecycle `json:"lifecycle"`
	BranchID    string          `json:"branch_id"`
	EngineID    string          `json:"engine_id"`
	Status      engine.Outcome  `json:"status"`
	Queued      int             `json:"queued"`
	Running     int             `json:"running"`
	Head        *TeamTaskIndex  `json:"head,omitempty"`
	LastOutcome *TeamTaskIndex  `json:"last_outcome,omitempty"`
}
type TeamTaskIndex struct {
	TaskID           string         `json:"task_id"`
	Sequence         uint64         `json:"sequence"`
	Status           engine.Outcome `json:"status"`
	TurnID           string         `json:"turn_id,omitempty"`
	ResultSequence   uint64         `json:"result_sequence,omitempty"`
	RequiredBranchID string         `json:"required_branch_id,omitempty"`
}
