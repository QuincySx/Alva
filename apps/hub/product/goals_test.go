// INPUT: Real product/native SQLite, explicit Goal services and frozen composer references.
// OUTPUT: Cold creation, exact/cancelled receipt recovery, scope boundaries, retained source and first-Send/tampering regressions.
// POS: [L7 Hub product tests] Goals reuse ordinary draft admission and never become another execution runtime.
package product

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

func TestGoalsMaximumEncodedSourceRetainsOriginalAndSubmits(t *testing.T) {
	for _, test := range []struct {
		name, criteria string
	}{
		{name: "HTML escaping", criteria: strings.Repeat("<", 8192)},
		{name: "allowed control characters", criteria: strings.Repeat("\t\r\n", 2730) + "<\n"},
	} {
		t.Run(test.name, func(t *testing.T) {
			s, requests := fixture(t)
			enableTestGoals(t, s)
			session, err := s.Create(t.Context(), "maximum encoded Goal source", "")
			if err != nil {
				t.Fatal(err)
			}
			request := productGoalRequest(t, hub.GoalScope{Kind: hub.GoalProjectScope, Workspace: session.Workspace})
			request.RequestID, request.Name, request.CompletionCriteria = strings.Repeat(">", 128), strings.Repeat("&", 256), test.criteria
			receipt, err := s.Goals.Create(t.Context(), request)
			if err != nil {
				t.Fatal("valid maximum raw fields rejected", err)
			}
			prepared, err := s.PrepareComposerGoal(t.Context(), session.ID, "", receipt.Goal.ID)
			if err != nil {
				t.Fatal(err)
			}
			ref := prepared.GoalRef
			if ref.Info.Bytes <= 16<<10 {
				t.Fatal("fixture did not exceed the old encoded-source budget", ref.Info.Bytes)
			}
			readSource := func() {
				t.Helper()
				info, raw, err := s.Goals.Repository.ReadArtifactFull(t.Context(), session.ID, ref.Info.SessionID, ref.Reference.URI, 96<<10)
				if err != nil || info != ref.Info || int64(len(raw)) != ref.Info.Bytes {
					t.Fatal("source bytes or metadata were lost", info, err)
				}
				var original hub.Goal
				if err = json.Unmarshal(raw, &original); err != nil || original != receipt.Goal || original.Creation() != request {
					t.Fatal("JSON encoding changed the original Goal fields", err)
				}
			}
			readSource()
			if err = s.RevalidateComposerPreparation(t.Context(), session.ID, prepared); err != nil {
				t.Fatal("valid expanded JSON source could not be revalidated", err)
			}
			if len(requests) != 0 || s.Runtime.HasActive() {
				t.Fatal("Goal creation or source preparation executed")
			}
			if _, err = s.SubmitComposerDraft(t.Context(), session.ID, "Review the selected maximum-size Goal", []ComposerPreparation{prepared}, nil); err != nil {
				t.Fatal("valid expanded source prevented explicit Send", err)
			}
			actual := <-requests
			if !strings.Contains(actual.Prompt, ref.Reference.URI) || !strings.Contains(actual.Prompt, receipt.Goal.ID) || strings.Contains(actual.Prompt, request.CompletionCriteria) {
				t.Fatal("Send did not use only the retained source reference")
			}
			waitIdle(t, s, session.ID)
			readSource()
			stored, err := s.Goals.Read(t.Context(), receipt.Goal.ID)
			if err != nil || stored != receipt.Goal {
				t.Fatal("successful Send changed the Goal fact", err)
			}
		})
	}
}

func enableTestGoals(t *testing.T, s *HubService) {
	t.Helper()
	store, ok := s.Store.(hub.GoalStore)
	if !ok {
		t.Fatal("fixture did not select GoalStore")
	}
	var err error
	s.Goals, err = NewGoalsService(store, s.Store, s.Repository)
	if err != nil {
		t.Fatal(err)
	}
}

func productGoalRequest(t *testing.T, scope hub.GoalScope) hub.GoalCreate {
	t.Helper()
	id, err := hub.NewID("request-")
	if err != nil {
		t.Fatal(err)
	}
	return hub.GoalCreate{RequestID: id, Name: "Ship the workspace", CompletionCriteria: "Private completion criteria kept in the original Goal source.", Scope: scope}
}

func TestGoalsHomeCreationDoesNotCreateSessionAndBindsOnlyOnExplicitFirstSend(t *testing.T) {
	s, requests := fixture(t)
	enableTestGoals(t, s)
	workspace := t.TempDir()
	request := productGoalRequest(t, hub.GoalScope{Kind: hub.GoalProjectScope, Workspace: workspace})
	receipt, err := s.Goals.Create(t.Context(), request)
	if err != nil {
		t.Fatal(err)
	}
	home, err := s.PrepareComposerGoal(t.Context(), "", workspace, receipt.Goal.ID)
	if err != nil {
		t.Fatal(err)
	}
	sessions, err := s.Store.ListSessions(t.Context(), 100)
	if err != nil || len(sessions) != 0 || len(requests) != 0 || s.Runtime.HasActive() || home.GoalRef.Info.ID != "" {
		t.Fatal("cold home Goal created execution/native session", sessions, err)
	}
	if _, err = s.PrepareComposerGoal(t.Context(), "", t.TempDir(), receipt.Goal.ID); !errors.Is(err, hub.ErrGoalScope) {
		t.Fatal("foreign home project accepted", err)
	}
	session, err := s.Create(t.Context(), "explicit first Send", workspace)
	if err != nil {
		t.Fatal(err)
	}
	bound, err := s.RebindComposerGoals(t.Context(), session.ID, []ComposerPreparation{home})
	if err != nil || len(bound) != 1 {
		t.Fatal("first Send binding", err)
	}
	if bound[0].GoalRef.SessionID != session.ID || bound[0].GoalRef.Info.ID == "" || bound[0].GoalRef.GoalID != receipt.Goal.ID {
		t.Fatal("source identity lost", bound)
	}
	if err = s.RevalidateComposerPreparation(t.Context(), session.ID, bound[0]); err != nil {
		t.Fatal(err)
	}
	if len(requests) != 0 || s.Runtime.HasActive() {
		t.Fatal("binding executed")
	}
	if _, err = s.SubmitComposerDraft(t.Context(), session.ID, "Review the selected goal", bound, nil); err != nil {
		t.Fatal(err)
	}
	actual := <-requests
	if !strings.Contains(actual.Prompt, receipt.Goal.ID) || !strings.Contains(actual.Prompt, "alva-artifact://") || strings.Contains(actual.Prompt, request.CompletionCriteria) {
		t.Fatal("Send did not preserve reference-only Goal source", actual.Prompt)
	}
	waitIdle(t, s, session.ID)
	stored, err := s.Goals.Read(t.Context(), receipt.Goal.ID)
	if err != nil || stored != receipt.Goal {
		t.Fatal("sending changed Goal fact", stored, err)
	}
}

func TestGoalsSessionScopeAndArtifactTamperingRejectedBeforeAdmission(t *testing.T) {
	s, requests := fixture(t)
	enableTestGoals(t, s)
	a, err := s.Create(t.Context(), "Goal owner", "")
	if err != nil {
		t.Fatal(err)
	}
	b, err := s.Create(t.Context(), "other session in same project", a.Workspace)
	if err != nil {
		t.Fatal(err)
	}
	receipt, err := s.Goals.Create(t.Context(), productGoalRequest(t, hub.GoalScope{Kind: hub.GoalSessionScope, SessionID: a.ID}))
	if err != nil {
		t.Fatal(err)
	}
	if receipt.Goal.Scope.Workspace != a.Workspace {
		t.Fatal("session scope did not derive saved workspace")
	}
	if _, err = s.PrepareComposerGoal(t.Context(), b.ID, "", receipt.Goal.ID); !errors.Is(err, hub.ErrGoalScope) {
		t.Fatal("same-project foreign session accepted", err)
	}
	selection, err := s.PrepareComposerGoal(t.Context(), a.ID, "", receipt.Goal.ID)
	if err != nil {
		t.Fatal(err)
	}
	copyRef := *selection.GoalRef
	copyRef.Reference.SHA256 = "forged-source"
	tampered := selection
	tampered.GoalRef = &copyRef
	if _, err = s.SubmitComposerDraft(t.Context(), a.ID, "task", []ComposerPreparation{tampered}, nil); err == nil {
		t.Fatal("forged Goal source admitted")
	}
	if _, err = s.SubmitComposerDraft(t.Context(), b.ID, "task", []ComposerPreparation{selection}, nil); err == nil {
		t.Fatal("foreign Goal reference admitted")
	}
	if len(requests) != 0 || s.Runtime.HasActive() {
		t.Fatal("rejected source executed")
	}
	target := s.DefaultTarget
	target.ModelID = "other-model"
	if _, err = s.Store.Switch(t.Context(), a.ID, target, "explicit route switch"); err != nil {
		t.Fatal(err)
	}
	if _, err = s.SubmitComposerDraft(t.Context(), a.ID, "task", []ComposerPreparation{selection}, nil); err == nil {
		t.Fatal("parked Goal source admitted on new branch")
	}
	stored, err := s.Goals.Read(t.Context(), receipt.Goal.ID)
	if err != nil || stored != receipt.Goal {
		t.Fatal("failed Send removed Goal", err)
	}
	target.EngineID = "foreign"
	foreign, err := s.Store.Switch(t.Context(), a.ID, target, "explicit external route")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.PrepareComposerGoal(t.Context(), a.ID, "", receipt.Goal.ID); !errors.Is(err, engine.ErrUnsupported) {
		t.Fatal("foreign branch borrowed native Goal authority", err)
	}
	detail, err := s.Store.Detail(t.Context(), a.ID)
	if err != nil {
		t.Fatal(err)
	}
	for _, branch := range detail.Branches {
		if branch.ID == foreign.ID && branch.NativeSessionID != "" {
			t.Fatal("unsupported Goal selection created a native identity")
		}
	}
}

func TestGoalsInvalidScopeCreatesNothing(t *testing.T) {
	s, requests := fixture(t)
	enableTestGoals(t, s)
	request := productGoalRequest(t, hub.GoalScope{Kind: hub.GoalProjectScope})
	if _, err := s.Goals.Create(t.Context(), request); err == nil {
		t.Fatal("home Goal silently chose workspace")
	}
	request.Scope.Workspace = t.TempDir()
	request.Name = ""
	if _, err := s.Goals.Create(t.Context(), request); !errors.Is(err, hub.ErrInvalidGoal) {
		t.Fatal("missing goal name accepted", err)
	}
	list, err := s.Goals.List(t.Context(), request.Scope, 100)
	if err != nil || len(list) != 0 {
		t.Fatal("invalid creation left Goal facts", list, err)
	}
	sessions, err := s.Store.ListSessions(t.Context(), 100)
	if err != nil || len(sessions) != 0 || len(requests) != 0 {
		t.Fatal("invalid Goal created chat or execution", err)
	}
}

type goalChangedReceiptStore struct {
	hub.GoalStore
	change func(hub.Goal) hub.Goal
	reads  bool
	calls  int
	cancel context.CancelFunc
}

func (s *goalChangedReceiptStore) CreateGoal(ctx context.Context, request hub.GoalCreate) (hub.GoalReceipt, error) {
	s.calls++
	receipt, err := s.GoalStore.CreateGoal(ctx, request)
	if err == nil && !s.reads && s.change != nil {
		receipt.Goal = s.change(receipt.Goal)
	}
	if err == nil && s.cancel != nil {
		s.cancel()
	}
	return receipt, err
}
func (s *goalChangedReceiptStore) Goal(ctx context.Context, id string) (hub.Goal, error) {
	goal, err := s.GoalStore.Goal(ctx, id)
	if err == nil && s.reads {
		goal = s.change(goal)
	}
	return goal, err
}

func TestGoalsCreationRejectsChangedReceiptWithoutReplayingCommittedFact(t *testing.T) {
	for _, test := range []struct {
		name   string
		reads  bool
		change func(hub.Goal) hub.Goal
	}{
		{name: "request", change: func(g hub.Goal) hub.Goal { g.RequestID += "-foreign"; return g }},
		{name: "name", change: func(g hub.Goal) hub.Goal { g.Name += "-changed"; return g }},
		{name: "criteria", change: func(g hub.Goal) hub.Goal { g.CompletionCriteria += "-changed"; return g }},
		{name: "scope", change: func(g hub.Goal) hub.Goal { g.Scope.Workspace += "-foreign"; return g }},
		{name: "revision", change: func(g hub.Goal) hub.Goal { g.Revision++; return g }},
		{name: "identity", change: func(g hub.Goal) hub.Goal { g.ID += "-foreign"; return g }},
		{name: "persisted identity", reads: true, change: func(g hub.Goal) hub.Goal { g.ID += "-foreign"; return g }},
		{name: "persisted criteria", reads: true, change: func(g hub.Goal) hub.Goal { g.CompletionCriteria += "-changed"; return g }},
	} {
		t.Run(test.name, func(t *testing.T) {
			s, requests := fixture(t)
			enableTestGoals(t, s)
			original := s.Goals.Store
			store := &goalChangedReceiptStore{GoalStore: original, change: test.change, reads: test.reads}
			s.Goals.Store = store
			request := productGoalRequest(t, hub.GoalScope{Kind: hub.GoalProjectScope, Workspace: t.TempDir()})
			scope, err := s.Goals.canonicalScope(t.Context(), request.Scope)
			if err != nil {
				t.Fatal(err)
			}
			request.Scope = scope
			if receipt, err := s.Goals.Create(t.Context(), request); err == nil || receipt != (hub.GoalReceipt{}) {
				t.Fatal("changed creation result became a successful Goal receipt", receipt, err)
			}
			facts, err := original.ListGoals(t.Context(), request.Scope, 100)
			if err != nil || len(facts) != 1 || facts[0].Creation() != request || store.calls != 1 || s.Runtime.HasActive() || len(requests) != 0 {
				t.Fatal("bad receipt replayed execution or lost the original committed Goal", facts, store.calls, err)
			}
			s.Goals.Store = original
			recovered, err := s.Goals.Create(t.Context(), request)
			if err != nil || !recovered.Replayed || recovered.Goal != facts[0] {
				t.Fatal("explicit retry did not recover the original Goal", recovered, err)
			}
		})
	}
}

func TestGoalsReadRejectsAnotherValidGoalIdentityBeforePreparingSource(t *testing.T) {
	s, requests := fixture(t)
	enableTestGoals(t, s)
	request := productGoalRequest(t, hub.GoalScope{Kind: hub.GoalProjectScope, Workspace: t.TempDir()})
	receipt, err := s.Goals.Create(t.Context(), request)
	if err != nil {
		t.Fatal(err)
	}
	s.Goals.Store = &goalChangedReceiptStore{GoalStore: s.Goals.Store, reads: true, change: func(g hub.Goal) hub.Goal { g.ID += "-foreign"; return g }}
	if _, err = s.PrepareComposerGoal(t.Context(), "", request.Scope.Workspace, receipt.Goal.ID); err == nil {
		t.Fatal("selecting one saved Goal prepared another identity")
	}
	sessions, err := s.Store.ListSessions(t.Context(), 100)
	if err != nil || len(sessions) != 0 || s.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("rejected Goal identity created or executed a session", sessions, err)
	}
}

func TestGoalsCancelledReceiptCheckPreservesFactForExplicitRetry(t *testing.T) {
	s, requests := fixture(t)
	enableTestGoals(t, s)
	original := s.Goals.Store
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	store := &goalChangedReceiptStore{GoalStore: original, cancel: cancel}
	s.Goals.Store = store
	request := productGoalRequest(t, hub.GoalScope{Kind: hub.GoalProjectScope, Workspace: t.TempDir()})
	scope, err := s.Goals.canonicalScope(t.Context(), request.Scope)
	if err != nil {
		t.Fatal(err)
	}
	request.Scope = scope
	if receipt, err := s.Goals.Create(ctx, request); !errors.Is(err, context.Canceled) || receipt != (hub.GoalReceipt{}) {
		t.Fatal("receipt verification detached from the cancelled caller", receipt, err)
	}
	facts, err := original.ListGoals(t.Context(), scope, 100)
	if err != nil || len(facts) != 1 || facts[0].Creation() != request || store.calls != 1 || s.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("cancelled verification lost or replayed the committed Goal", facts, err)
	}
	s.Goals.Store = original
	recovered, err := s.Goals.Create(t.Context(), request)
	if err != nil || !recovered.Replayed || recovered.Goal != facts[0] {
		t.Fatal("explicit retry did not recover the fact after cancellation", recovered, err)
	}
}
