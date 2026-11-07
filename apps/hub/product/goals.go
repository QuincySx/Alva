// INPUT: Explicit GoalStore, persisted session routing and native artifact repository.
// OUTPUT: Shared goal creation and typed, scope-checked composer references with retained source artifacts.
// POS: [L7 Hub product] Creates desired outcomes without creating chats or executing; Send revalidates the original scope and revision.
package product

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"reflect"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type GoalSessions interface {
	Detail(context.Context, string) (hub.Detail, error)
}

type GoalArtifacts interface {
	Create(context.Context, string, string, string) (storage.Session, error)
	PutArtifact(context.Context, string, string, storage.ArtifactInput) (storage.ArtifactInfo, error)
	ReadArtifactFull(context.Context, string, string, string, int) (storage.ArtifactInfo, []byte, error)
}

type GoalsService struct {
	Store      hub.GoalStore
	Sessions   GoalSessions
	Repository GoalArtifacts
}

func NewGoalsService(store hub.GoalStore, sessions GoalSessions, repository GoalArtifacts) (*GoalsService, error) {
	if storage.Missing(store) || storage.Missing(sessions) || storage.Missing(repository) {
		return nil, errors.New("explicit goal store, session routing and artifact repository required")
	}
	return &GoalsService{Store: store, Sessions: sessions, Repository: repository}, nil
}

func canonicalGoalWorkspace(directory string) (string, error) {
	if !filepath.IsAbs(directory) {
		return "", fmt.Errorf("%w: explicitly selected project directory required", hub.ErrInvalidGoal)
	}
	canonical, err := filepath.EvalSymlinks(directory)
	if err != nil {
		return "", err
	}
	info, err := os.Stat(canonical)
	if err != nil {
		return "", err
	}
	if !info.IsDir() {
		return "", fmt.Errorf("%w: project must be an existing directory", hub.ErrInvalidGoal)
	}
	return canonical, nil
}

func (s *GoalsService) canonicalScope(ctx context.Context, scope hub.GoalScope) (hub.GoalScope, error) {
	if scope.Kind == hub.GoalSessionScope {
		detail, err := s.Sessions.Detail(ctx, scope.SessionID)
		if err != nil {
			return hub.GoalScope{}, err
		}
		if scope.Workspace != "" && scope.Workspace != detail.Session.Workspace {
			return hub.GoalScope{}, hub.ErrGoalScope
		}
		scope.Workspace = detail.Session.Workspace
	}
	canonical, err := canonicalGoalWorkspace(scope.Workspace)
	if err != nil {
		return hub.GoalScope{}, err
	}
	// A saved session cannot silently retarget its workspace through a symlink.
	if scope.Kind == hub.GoalSessionScope && canonical != scope.Workspace {
		return hub.GoalScope{}, hub.ErrGoalScope
	}
	scope.Workspace = canonical
	return scope, scope.Validate()
}

func (s *GoalsService) Create(ctx context.Context, request hub.GoalCreate) (hub.GoalReceipt, error) {
	if err := ctx.Err(); err != nil {
		return hub.GoalReceipt{}, err
	}
	scope, err := s.canonicalScope(ctx, request.Scope)
	if err != nil {
		return hub.GoalReceipt{}, err
	}
	request.Scope = scope
	if err = request.Validate(); err != nil {
		return hub.GoalReceipt{}, err
	}
	return s.Store.CreateGoal(ctx, request)
}

func (s *GoalsService) Read(ctx context.Context, id string) (hub.Goal, error) {
	goal, err := s.Store.Goal(ctx, id)
	if err != nil {
		return hub.Goal{}, err
	}
	return goal, goal.Validate()
}

func (s *GoalsService) List(ctx context.Context, scope hub.GoalScope, limit int) ([]hub.Goal, error) {
	scope, err := s.canonicalScope(ctx, scope)
	if err != nil {
		return nil, err
	}
	return s.Store.ListGoals(ctx, scope, limit)
}

// A home project reference has no session, branch or artifact until the first
// explicit Send binds it to the newly created session. The Goal already exists.
type ComposerGoalReference struct {
	GoalID    string               `json:"goal_id"`
	Revision  uint64               `json:"revision"`
	Scope     hub.GoalScope        `json:"scope"`
	SessionID string               `json:"session_id,omitempty"`
	BranchID  string               `json:"branch_id,omitempty"`
	Info      storage.ArtifactInfo `json:"info"`
	Reference wire.Reference       `json:"reference"`
}

func goalPreparation(g hub.Goal, reference ComposerGoalReference) ComposerPreparation {
	return ComposerPreparation{EntryID: "goal:" + g.ID, Kind: ComposerGoalEntry, Origin: "alva.goal", Scope: string(g.Scope.Kind), Text: "目标引用：「" + g.Name + "」", GoalRef: &reference}
}

func (s *HubService) PrepareComposerGoal(ctx context.Context, id, workspace, goalID string) (ComposerPreparation, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.prepareComposerGoalLocked(ctx, id, workspace, goalID)
}

func (s *HubService) prepareComposerGoalLocked(ctx context.Context, id, workspace, goalID string) (ComposerPreparation, error) {
	if s.Goals == nil {
		return ComposerPreparation{}, errors.New("goal service unavailable")
	}
	g, err := s.Goals.Read(ctx, goalID)
	if err != nil {
		return ComposerPreparation{}, err
	}
	ref := ComposerGoalReference{GoalID: g.ID, Revision: g.Revision, Scope: g.Scope}
	if id == "" {
		canonical, err := canonicalGoalWorkspace(workspace)
		if err != nil {
			return ComposerPreparation{}, err
		}
		if g.Scope.Kind != hub.GoalProjectScope || g.Scope.Workspace != canonical {
			return ComposerPreparation{}, hub.ErrGoalScope
		}
		return goalPreparation(g, ref), nil
	}
	detail, err := s.Store.Detail(ctx, id)
	if err != nil {
		return ComposerPreparation{}, err
	}
	if err = goalMatchesSession(g, detail.Session); err != nil {
		return ComposerPreparation{}, err
	}
	if hub.BlocksInput(detail.Session.Status) {
		return ComposerPreparation{}, errors.New("finish the current Turn before selecting a goal")
	}
	if s.Runtime != nil {
		if _, active := s.Runtime.ActiveExecution(id); active {
			return ComposerPreparation{}, errors.New("finish the current execution before selecting a goal")
		}
	}
	branch, err := ActiveBranch(detail)
	if err != nil {
		return ComposerPreparation{}, err
	}
	if branch.Target.EngineID != "alva" {
		return ComposerPreparation{}, fmt.Errorf("goal source references require the native Alva branch: %w", engine.ErrUnsupported)
	}
	if branch.NativeSessionID == "" {
		branch.NativeSessionID, err = hub.NewID("native-")
		if err != nil {
			return ComposerPreparation{}, err
		}
		if _, err = s.Goals.Repository.Create(ctx, id, branch.NativeSessionID, ""); err != nil {
			return ComposerPreparation{}, err
		}
		if err = s.Store.BindNative(ctx, id, branch.ID, branch.NativeSessionID); err != nil {
			return ComposerPreparation{}, err
		}
	}
	data, err := json.Marshal(g)
	if err != nil {
		return ComposerPreparation{}, err
	}
	info, err := s.Goals.Repository.PutArtifact(ctx, id, branch.NativeSessionID, storage.ArtifactInput{Kind: "goal-reference", Name: g.ID + ".json", MediaType: "application/json", Text: true, Data: data, Processing: "Immutable user-created Goal source with exact goal ID, revision and project/session scope. Desired outcome only; no execution, permission or automatic background work granted."})
	if err != nil {
		return ComposerPreparation{}, err
	}
	ref.SessionID, ref.BranchID, ref.Info, ref.Reference = id, branch.ID, info, info.Reference()
	return goalPreparation(g, ref), nil
}

func goalMatchesSession(g hub.Goal, session hub.Session) error {
	if g.Scope.Workspace != session.Workspace || g.Scope.Kind == hub.GoalSessionScope && g.Scope.SessionID != session.ID {
		return hub.ErrGoalScope
	}
	return nil
}

// RebindComposerGoals is called only after first Send has created its real
// session. It preserves all non-goal preparations and returns no partial list.
func (s *HubService) RebindComposerGoals(ctx context.Context, id string, selections []ComposerPreparation) ([]ComposerPreparation, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.rebindComposerGoalsLocked(ctx, id, selections)
}

func (s *HubService) rebindComposerGoalsLocked(ctx context.Context, id string, selections []ComposerPreparation) ([]ComposerPreparation, error) {
	result := append([]ComposerPreparation(nil), selections...)
	for i, selection := range selections {
		if selection.Kind != ComposerGoalEntry {
			continue
		}
		if s.Goals == nil {
			return nil, errors.New("goal service unavailable")
		}
		if selection.GoalRef == nil || selection.GoalRef.SessionID != "" {
			if err := s.revalidateGoalPreparation(ctx, id, selection); err != nil {
				return nil, err
			}
			continue
		}
		g, err := s.Goals.Read(ctx, selection.GoalRef.GoalID)
		if err != nil || !reflect.DeepEqual(selection, goalPreparation(g, *selection.GoalRef)) || selection.GoalRef.Revision != g.Revision || selection.GoalRef.Scope != g.Scope || selection.GoalRef.BranchID != "" || selection.GoalRef.Info != (storage.ArtifactInfo{}) || !reflect.DeepEqual(selection.GoalRef.Reference, wire.Reference{}) {
			return nil, errors.Join(errors.New("home goal reference changed; select it again"), err)
		}
		result[i], err = s.prepareComposerGoalLocked(ctx, id, "", g.ID)
		if err != nil {
			return nil, err
		}
	}
	return result, ctx.Err()
}

func (s *HubService) revalidateGoalPreparation(ctx context.Context, id string, prepared ComposerPreparation) error {
	if s.Goals == nil || prepared.GoalRef == nil {
		return errors.New("goal source is missing")
	}
	ref := prepared.GoalRef
	g, err := s.Goals.Read(ctx, ref.GoalID)
	if err != nil {
		return err
	}
	if !reflect.DeepEqual(prepared, goalPreparation(g, *ref)) || ref.Revision != g.Revision || ref.Scope != g.Scope || ref.SessionID != id || id == "" {
		return errors.New("goal reference changed; review and select it again")
	}
	detail, err := s.Store.Detail(ctx, id)
	if err != nil {
		return err
	}
	if err = goalMatchesSession(g, detail.Session); err != nil {
		return err
	}
	branch, err := ActiveBranch(detail)
	if err != nil {
		return err
	}
	if ref.BranchID != branch.ID || ref.Info.SessionID != branch.NativeSessionID || ref.Info.Kind != "goal-reference" || ref.Info.Name != g.ID+".json" || !reflect.DeepEqual(ref.Reference, ref.Info.Reference()) {
		return errors.New("goal source belongs to a parked branch or was altered")
	}
	// Legal bounded Goal fields may expand sixfold in JSON (for example '<').
	info, raw, err := s.Goals.Repository.ReadArtifactFull(ctx, id, branch.NativeSessionID, ref.Reference.URI, 96<<10)
	if err != nil {
		return err
	}
	expected, err := json.Marshal(g)
	if err != nil || info != ref.Info || string(raw) != string(expected) {
		return errors.New("goal source artifact changed")
	}
	return ctx.Err()
}

func (s *HubService) composerGoalPrompt(ctx context.Context, id string, selections []ComposerPreparation, prompt string) (string, error) {
	var refs []ComposerGoalReference
	for _, selection := range selections {
		if selection.Kind == ComposerGoalEntry {
			if err := s.revalidateGoalPreparation(ctx, id, selection); err != nil {
				return "", err
			}
			refs = append(refs, *selection.GoalRef)
		}
	}
	if len(refs) == 0 {
		return prompt, nil
	}
	data, err := json.Marshal(refs)
	if err != nil {
		return "", err
	}
	return prompt + "\n\n[User-selected Goal references. Desired outcomes only; creation and selection do not execute, enable capabilities, grant permissions or start background work. Exact original Goal definitions are retained in the referenced artifacts; read only relevant authorized ranges.]\n" + string(data), nil
}
