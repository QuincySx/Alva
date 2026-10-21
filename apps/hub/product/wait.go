// INPUT: Saved active Hub branch/Turn, actual persistent native journal and explicit reviewed wait commands.
// OUTPUT: Cold wait view, bound resolution, same-Turn continuation and revision-bound abandonment.
// POS: [L7 product] One supplied facts authority and the existing Hub execution owner; UI/HTTP never launch a second Agent.
package product

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/harness/durable"
	"github.com/QuincySx/Alva/harness/durable/journal"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage/blob"
	"github.com/QuincySx/Alva/sdk/llm/wire"
	"time"
)

type WaitView struct {
	Status        engine.Outcome   `json:"status"`
	Supported     bool             `json:"supported"`
	Enabled       bool             `json:"enabled"`
	BranchID      string           `json:"branch_id"`
	TurnID        string           `json:"turn_id,omitempty"`
	Snapshot      durable.Snapshot `json:"snapshot"`
	CanResume     bool             `json:"can_resume"`
	ResumeProblem string           `json:"resume_problem,omitempty"`
}
type WaitTarget struct {
	BranchID string `json:"branch_id"`
	TurnID   string `json:"turn_id"`
	Revision uint64 `json:"revision"`
}
type WaitResolution struct {
	Target    WaitTarget      `json:"target"`
	RequestID string          `json:"request_id"`
	Binding   string          `json:"binding"`
	Result    json.RawMessage `json:"result"`
}

func (s *HubService) waitView(ctx context.Context, id string) (WaitView, hub.Branch, *journal.Backend, error) {
	var view WaitView
	d, err := s.Store.Detail(ctx, id)
	if err != nil {
		return view, hub.Branch{}, nil, err
	}
	b, err := ActiveBranch(d)
	if err != nil {
		return view, b, nil, err
	}
	view.BranchID, view.Status = b.ID, d.Session.Status
	descriptor, ok := s.Runtime.Descriptor(b.Target.EngineID)
	// This product controller binds the supplied Alva journal. Another engine's
	// own waiting capability never grants access to that native authority.
	view.Supported = b.Target.EngineID == "alva" && ok && descriptor.Runtime.Capabilities().DurableWaits
	settings, err := s.Store.Settings(ctx, id)
	if err != nil {
		return view, b, nil, err
	}
	view.Enabled = settings.DurableWaits
	t, err := s.Store.BranchTurn(ctx, id, b.ID)
	if err != nil && !errors.Is(err, hub.ErrNotFound) {
		return view, b, nil, err
	}
	if err == nil {
		view.TurnID = t.ID
	}
	view.Snapshot.State = durable.State{Version: 1, Status: durable.Idle}
	if !view.Supported || b.NativeSessionID == "" {
		return view, b, nil, nil
	}
	_, exists, err := s.Repository.LatestRecord(ctx, id, b.NativeSessionID, durable.Namespace, durable.SnapshotType)
	if err != nil {
		return view, b, nil, err
	}
	if !view.Enabled && !exists {
		return view, b, nil, nil
	}
	backend, err := journal.Open(ctx, s.Repository, id, b.NativeSessionID)
	if err != nil {
		return view, b, nil, err
	}
	view.Snapshot, err = backend.Load(ctx)
	if err != nil {
		return view, b, nil, err
	}
	view.CanResume = view.Enabled && d.Session.Status == engine.Waiting && t.Status == engine.Waiting && view.Snapshot.State.Status == durable.Waiting && !s.Runtime.SessionActive(id)
	for _, r := range view.Snapshot.State.Requests {
		view.CanResume = view.CanResume && len(r.Result) > 0 && time.Now().Before(r.ExpiresAt)
	}
	if view.CanResume && (s.ValidateWaitRecipe == nil || s.ValidateWaitRecipe(ctx, b, settings) != nil) {
		view.CanResume = false
		view.ResumeProblem = "Execution recipe changed or cannot be verified; inspect and abandon before new work."
	}
	return view, b, backend, nil
}
func (s *HubService) Waits(ctx context.Context, id string) (WaitView, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	view, _, _, err := s.waitView(ctx, id)
	return view, err
}
func (s *HubService) EnableDurableWaits(ctx context.Context, id string, enabled bool) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	v, _, _, err := s.waitView(ctx, id)
	if err != nil {
		return err
	}
	if !v.Supported || s.ValidateWaitRecipe == nil {
		return engine.ErrUnsupported
	}
	if enabled && (s.Repository.RecordRetention() != blob.Persistent || s.Repository.ArtifactStorage().Retention != blob.Persistent) {
		return errors.New("durable waits require persistent facts and artifacts")
	}
	if v.Snapshot.State.Status != durable.Idle || s.Runtime.SessionActive(id) {
		return errors.New("review and abandon the existing checkpoint before changing waits")
	}
	return s.Store.SetDurableWaits(ctx, id, enabled)
}
func checkWaitTarget(view WaitView, target WaitTarget) error {
	if !view.Supported {
		return engine.ErrUnsupported
	}
	if view.BranchID != target.BranchID || view.TurnID == "" || view.TurnID != target.TurnID || view.Snapshot.Revision != target.Revision {
		return durable.ErrConflict
	}
	return nil
}
func (s *HubService) ResolveWait(ctx context.Context, id string, answer WaitResolution) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	view, b, backend, err := s.waitView(ctx, id)
	if err != nil {
		return err
	}
	if err = checkWaitTarget(view, answer.Target); err != nil {
		return err
	}
	if !view.Enabled || view.Status != engine.Waiting || view.Snapshot.State.Status != durable.Waiting || backend == nil || s.Runtime.SessionActive(id) {
		return errors.New("wait resolution requires inactive native execution")
	}
	_, release, err := s.Repository.BeginRun(ctx, id, b.NativeSessionID)
	if err != nil {
		return err
	}
	defer release()
	actual, err := backend.Load(ctx)
	if err != nil {
		return err
	}
	if actual.Revision != answer.Target.Revision {
		return durable.ErrConflict
	}
	c, err := durable.New(durable.Config{Backend: reviewedWaitBackend{Backend: backend, revision: answer.Target.Revision}, TTL: 24 * time.Hour})
	if err != nil {
		return err
	}
	return c.Resolve(ctx, durable.Resolution{RequestID: answer.RequestID, Binding: answer.Binding, Result: answer.Result})
}
func (s *HubService) ResumeWait(ctx context.Context, id string, target WaitTarget) (string, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	view, b, _, err := s.waitView(ctx, id)
	if err != nil {
		return "", err
	}
	if err = checkWaitTarget(view, target); err != nil {
		return "", err
	}
	if !view.CanResume || s.ValidateWaitRecipe == nil {
		if view.ResumeProblem != "" {
			return "", errors.New(view.ResumeProblem)
		}
		return "", errors.New("resolve all live requests before explicit continuation")
	}
	settings, err := s.Store.Settings(ctx, id)
	if err != nil {
		return "", err
	}
	if err = s.ValidateWaitRecipe(ctx, b, settings); err != nil {
		return "", err
	}
	if len(s.attachments[id]) != 0 {
		return "", errors.New("continuation does not submit draft attachments")
	}
	if err = ctx.Err(); err != nil {
		return "", err
	}
	turn, _, err := s.Runtime.ResumeWait(context.Background(), id, b.ID, target.TurnID, target.Revision)
	return turn, err
}
func (s *HubService) AbandonWait(ctx context.Context, id string, target WaitTarget) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	view, b, backend, err := s.waitView(ctx, id)
	if err != nil {
		return err
	}
	if err = checkWaitTarget(view, target); err != nil {
		return err
	}
	if backend == nil || s.Runtime.SessionActive(id) {
		return errors.New("active execution must physically exit before abandonment")
	}
	if view.Snapshot.State.Status == durable.Idle && view.Status != engine.Waiting {
		return errors.New("there is no pending or interrupted checkpoint to abandon")
	}
	// Controller owns the actual lease until both native reset and product
	// reconciliation finish. A product commit failure retains an idle native
	// checkpoint, which can be explicitly inspected and reconciled again.
	c, err := durable.New(durable.Config{Backend: reviewedWaitBackend{Backend: backend, revision: target.Revision, after: func(ctx context.Context) error { return s.Store.AbandonWaitingTurn(ctx, id, b.ID, target.TurnID) }}, TTL: 24 * time.Hour})
	if err != nil {
		return err
	}
	return c.Abandon(ctx)
}

type reviewedWaitBackend struct {
	durable.Backend
	revision uint64
	after    func(context.Context) error
}

func (b reviewedWaitBackend) Commit(ctx context.Context, revision uint64, state durable.State, messages []wire.Message) (durable.Snapshot, error) {
	if revision != b.revision {
		return durable.Snapshot{}, durable.ErrConflict
	}
	snapshot, err := b.Backend.Commit(ctx, revision, state, messages)
	if err == nil && b.after != nil {
		err = b.after(ctx)
	}
	return snapshot, err
}
