// INPUT: context, encoding/json, agent Task and llm/wire.
// OUTPUT: TaskRecord, TaskPage, CreateRequest, UpdateRequest and replaceable Service.
// POS: [L4 extension] Portable workflow contracts contain no native execution, storage or authorization policy.
package planning

import (
	"context"
	"encoding/json"

	agentabi "github.com/QuincySx/Alva/sdk/agent/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type TaskRecord struct {
	Task        agentabi.Task    `json:"task"`
	Description string           `json:"description"`
	Owner       string           `json:"owner,omitempty"`
	BlockedBy   []string         `json:"blocked_by,omitempty"`
	Metadata    json.RawMessage  `json:"metadata,omitempty"`
	Output      []wire.Reference `json:"output,omitempty"`
}
type TaskPage struct {
	Tasks    []TaskRecord `json:"tasks"`
	Next     int          `json:"next"`
	HasMore  bool         `json:"has_more"`
	Revision uint64       `json:"revision"`
}
type CreateRequest struct {
	Title, Description, Owner string
	Metadata                  json.RawMessage
	BlockedBy                 []string
}
type UpdateRequest struct {
	ID                        string
	Title, Description, Owner *string
	Status                    agentabi.TaskStatus
	Metadata                  json.RawMessage
	BlockedBy                 []string
	AppendOutput              *string
}

// Service is an explicitly bound host capability. Implementations own task
// validation, concurrency and persistence; they grant no execution authority.
// Output references must remain readable through the actual session's reader.
type Service interface {
	Create(context.Context, CreateRequest) (TaskRecord, error)
	Get(context.Context, string) (TaskRecord, bool, error)
	List(context.Context, agentabi.TaskStatus, int, int) (TaskPage, error)
	Update(context.Context, UpdateRequest) (TaskRecord, error)
	SetNotes(context.Context, string) (wire.Reference, error)
	Notes(context.Context) (*wire.Reference, error)
}
