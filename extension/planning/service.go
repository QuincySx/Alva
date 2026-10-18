// INPUT: bytes, context, encoding/json, errors, sync, unicode/utf8, agent Task, ABI/wire, storage/session.
// OUTPUT: Typed TaskRecord/TaskPage, Service and native fact-backed Task/Plan implementation with optimistic persistence.
// POS: [L4 extension] Planning state has real durable consumers without inventing processes, output paths or kernel setters.
package planning

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"sync"
	"unicode/utf8"

	"github.com/QuincySx/Alva/platform/storage/session"
	agentabi "github.com/QuincySx/Alva/sdk/agent/abi"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
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
type state struct {
	Tasks []TaskRecord    `json:"tasks"`
	Notes *wire.Reference `json:"notes,omitempty"`
}
type nativeService struct {
	mu        sync.Mutex
	repo      *session.Repository
	log       *session.Session
	owner, id string
	revision  uint64
	state     state
}

func newService(ctx context.Context, repo *session.Repository, log *session.Session, owner, id string) (*nativeService, error) {
	s := &nativeService{repo: repo, log: log, owner: owner, id: id, state: state{Tasks: []TaskRecord{}}}
	entry, found, err := repo.LatestRecord(ctx, owner, id, "alva.planning", "state_snapshot")
	if err != nil {
		return nil, err
	}
	if !found {
		return s, nil
	}
	_, data, err := repo.ReadArtifactFull(ctx, owner, id, entry.Value.URI, 256<<10)
	if err != nil {
		return nil, err
	}
	var record abi.SessionRecord
	if err = json.Unmarshal(data, &record); err != nil {
		return nil, err
	}
	if record.Kind != abi.RecordExtension || record.Namespace != "alva.planning" || record.Type != "state_snapshot" || record.SchemaVersion != 1 || record.ID != entry.ID || record.Sequence != entry.Sequence {
		return nil, errors.New("planning snapshot identity/type mismatch")
	}
	var fields map[string]json.RawMessage
	if json.Unmarshal(record.Payload, &fields) != nil || fields == nil || fields["tasks"] == nil || bytes.Equal(bytes.TrimSpace(fields["tasks"]), []byte("null")) {
		return nil, errors.New("planning snapshot requires explicit task array")
	}
	decoder := json.NewDecoder(bytes.NewReader(record.Payload))
	decoder.DisallowUnknownFields()
	if err = decoder.Decode(&s.state); err != nil {
		return nil, err
	}
	if err = s.validate(ctx, s.state); err != nil {
		return nil, err
	}
	s.revision = record.Sequence
	return s, nil
}
func bounded(text string, n int) bool { return utf8.ValidString(text) && len(text) <= n }
func (s *nativeService) validate(ctx context.Context, value state) error {
	if len(value.Tasks) > 128 {
		return errors.New("planning task budget exceeds 128")
	}
	ids := map[string]bool{}
	for _, r := range value.Tasks {
		if r.Task.Kind != agentabi.TaskLocalWorkflow || r.Task.ID == "" || len(r.Task.ID) > 64 || ids[r.Task.ID] || r.Task.Title == "" || !bounded(r.Task.Title, 512) || !bounded(r.Description, 4096) || !bounded(r.Owner, 128) || len(r.BlockedBy) > 20 || len(r.Output) > 32 || len(r.Metadata) > 4096 || r.Task.OutputFile != "" || r.Task.OutputOffset < 0 || r.Task.CreatedAt.IsZero() {
			return errors.New("invalid bounded planning task")
		}
		ids[r.Task.ID] = true
		if r.Task.Status != agentabi.TaskPending && r.Task.Status != agentabi.TaskRunning && !r.Task.Status.Terminal() {
			return errors.New("invalid planning task status")
		}
		if len(r.Metadata) > 0 {
			var metadata map[string]json.RawMessage
			if json.Unmarshal(r.Metadata, &metadata) != nil || metadata == nil {
				return errors.New("planning metadata must be JSON object")
			}
		}
		for _, ref := range r.Output {
			if _, err := s.repo.ArtifactMetadata(ctx, s.owner, s.id, ref.URI); err != nil {
				return err
			}
		}
	}
	for _, r := range value.Tasks {
		seen := map[string]bool{}
		for _, id := range r.BlockedBy {
			if id == r.Task.ID || !ids[id] || seen[id] {
				return errors.New("invalid planning dependency")
			}
			seen[id] = true
			if r.Task.Status == agentabi.TaskRunning || r.Task.Status == agentabi.TaskSucceeded {
				for _, candidate := range value.Tasks {
					if candidate.Task.ID == id && candidate.Task.Status != agentabi.TaskSucceeded {
						return errors.New("active/completed task has an unsatisfied dependency")
					}
				}
			}
		}
	}
	// Reject indirect cycles, not just a task depending on itself.
	visiting, visited := map[string]bool{}, map[string]bool{}
	byID := map[string]TaskRecord{}
	for _, r := range value.Tasks {
		byID[r.Task.ID] = r
	}
	var visit func(string) error
	visit = func(id string) error {
		if visiting[id] {
			return errors.New("planning dependency cycle")
		}
		if visited[id] {
			return nil
		}
		visiting[id] = true
		for _, dep := range byID[id].BlockedBy {
			if err := visit(dep); err != nil {
				return err
			}
		}
		visiting[id] = false
		visited[id] = true
		return nil
	}
	for id := range ids {
		if err := visit(id); err != nil {
			return err
		}
	}
	if value.Notes != nil {
		if _, err := s.repo.ArtifactMetadata(ctx, s.owner, s.id, value.Notes.URI); err != nil {
			return err
		}
	}
	return nil
}
func cloneState(value state) state {
	raw, _ := json.Marshal(value)
	var copy state
	json.Unmarshal(raw, &copy)
	return copy
}
func (s *nativeService) save(ctx context.Context, next state) error {
	if err := s.validate(ctx, next); err != nil {
		return err
	}
	raw, err := json.Marshal(next)
	if err != nil {
		return err
	}
	if len(raw) > 128<<10 {
		return errors.New("planning snapshot exceeds 128 KiB")
	}
	record, err := s.log.AppendExtensionSnapshot(ctx, "alva.planning", "state_snapshot", s.revision, raw)
	if err != nil {
		return &abi.SessionError{Operation: "commit planning snapshot", SessionID: s.id, Err: err}
	}
	// Canonical native redaction is authoritative; don't keep a secret-bearing
	// uncommitted cache that differs from the actual stored fact.
	var committed state
	if err = json.Unmarshal(record.Payload, &committed); err != nil {
		return err
	}
	s.state = committed
	s.revision = record.Sequence
	if err = s.log.Flush(ctx); err != nil {
		return &abi.SessionError{Operation: "flush planning snapshot", SessionID: s.id, Err: err}
	}
	return nil
}
func (s *nativeService) Create(ctx context.Context, a CreateRequest) (TaskRecord, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	task, err := agentabi.NewTask(agentabi.TaskLocalWorkflow, a.Title, "", "")
	if err != nil {
		return TaskRecord{}, err
	}
	r := TaskRecord{Task: task, Description: a.Description, Owner: a.Owner, Metadata: a.Metadata, BlockedBy: append([]string(nil), a.BlockedBy...)}
	next := cloneState(s.state)
	next.Tasks = append(next.Tasks, r)
	if err = s.save(ctx, next); err != nil {
		return TaskRecord{}, err
	}
	return cloneState(s.state).Tasks[len(s.state.Tasks)-1], nil
}
func (s *nativeService) Get(ctx context.Context, id string) (TaskRecord, bool, error) {
	if err := ctx.Err(); err != nil {
		return TaskRecord{}, false, err
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	for _, r := range cloneState(s.state).Tasks {
		if r.Task.ID == id {
			return r, true, nil
		}
	}
	return TaskRecord{}, false, nil
}
func (s *nativeService) List(ctx context.Context, status agentabi.TaskStatus, after, limit int) (TaskPage, error) {
	if err := ctx.Err(); err != nil {
		return TaskPage{}, err
	}
	if status != "" && status != agentabi.TaskPending && status != agentabi.TaskRunning && !status.Terminal() {
		return TaskPage{}, errors.New("invalid planning status filter")
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if after < 0 || after > len(s.state.Tasks) || limit < 1 || limit > 20 {
		return TaskPage{}, errors.New("bounded task cursor/limit required")
	}
	page := TaskPage{Revision: s.revision, Next: after}
	tasks := cloneState(s.state).Tasks
	for index := after; index < len(tasks); index++ {
		if status != "" && tasks[index].Task.Status != status {
			page.Next = index + 1
			continue
		}
		if len(page.Tasks) == limit {
			page.HasMore = true
			break
		}
		page.Tasks = append(page.Tasks, tasks[index])
		page.Next = index + 1
	}
	return page, nil
}
func (s *nativeService) Update(ctx context.Context, a UpdateRequest) (TaskRecord, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	next := cloneState(s.state)
	index := -1
	for i := range next.Tasks {
		if next.Tasks[i].Task.ID == a.ID {
			index = i
			break
		}
	}
	if index < 0 {
		return TaskRecord{}, errors.New("planning task not found")
	}
	r := &next.Tasks[index]
	if a.Title != nil {
		r.Task.Title = *a.Title
	}
	if a.Description != nil {
		r.Description = *a.Description
	}
	if a.Owner != nil {
		r.Owner = *a.Owner
	}
	if a.Metadata != nil {
		r.Metadata = append(json.RawMessage(nil), a.Metadata...)
	}
	if a.BlockedBy != nil {
		r.BlockedBy = append([]string(nil), a.BlockedBy...)
	}
	if a.Status != "" {
		if a.Status == agentabi.TaskRunning {
			for _, dep := range r.BlockedBy {
				for _, candidate := range next.Tasks {
					if candidate.Task.ID == dep && candidate.Task.Status != agentabi.TaskSucceeded {
						return TaskRecord{}, errors.New("task dependency is not succeeded")
					}
				}
			}
		}
		if err := r.Task.Transition(a.Status); err != nil {
			return TaskRecord{}, err
		}
	}
	if a.AppendOutput != nil {
		if !bounded(*a.AppendOutput, 64<<10) {
			return TaskRecord{}, errors.New("planning output note exceeds 64 KiB UTF-8")
		}
		ref, err := s.text(ctx, "planning-output-note", a.ID, *a.AppendOutput)
		if err != nil {
			return TaskRecord{}, err
		}
		r.Output = append(r.Output, ref)
		if err = r.Task.AdvanceOutput(r.Task.OutputOffset + int64(len(*a.AppendOutput))); err != nil {
			return TaskRecord{}, err
		}
	}
	if err := s.save(ctx, next); err != nil {
		return TaskRecord{}, err
	}
	return cloneState(s.state).Tasks[index], nil
}
func (s *nativeService) text(ctx context.Context, kind, name, text string) (wire.Reference, error) {
	raw, _ := json.Marshal(text)
	clean, err := session.RedactJSON(raw)
	if err != nil {
		return wire.Reference{}, err
	}
	if err = json.Unmarshal(clean, &text); err != nil {
		return wire.Reference{}, err
	}
	info, err := s.repo.PutArtifact(ctx, s.owner, s.id, session.ArtifactInput{Kind: kind, Name: name, MediaType: "text/plain; charset=utf-8", Processing: "User/model-authored planning note; known credentials redacted; not executed, no workspace file written and no automatic model delivery", Data: []byte(text), Text: true})
	if err != nil {
		return wire.Reference{}, &abi.SessionError{Operation: "retain planning note", SessionID: s.id, Err: err}
	}
	return info.Reference(), nil
}
func (s *nativeService) SetNotes(ctx context.Context, text string) (wire.Reference, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if !bounded(text, 64<<10) {
		return wire.Reference{}, errors.New("plan notes exceed 64 KiB UTF-8")
	}
	ref, err := s.text(ctx, "planning-notes", "plan", text)
	if err != nil {
		return wire.Reference{}, err
	}
	next := cloneState(s.state)
	next.Notes = &ref
	if err = s.save(ctx, next); err != nil {
		return wire.Reference{}, err
	}
	return ref, nil
}
func (s *nativeService) Notes(ctx context.Context) (*wire.Reference, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	return cloneState(s.state).Notes, nil
}
