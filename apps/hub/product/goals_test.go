// INPUT: Real product/native SQLite, explicit Goal services and frozen composer references.
// OUTPUT: Cold creation, scope boundaries, retained source, first-Send binding and rejected tampering regressions.
// POS: [L7 Hub product tests] Goals reuse ordinary draft admission and never become another execution runtime.
package product

import (
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
			if _, err = s.SubmitComposerDraft(t.Context(), session.ID, "Review the selected maximum-size Goal", []ComposerPreparation{prepared}); err != nil {
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
	if _, err = s.SubmitComposerDraft(t.Context(), session.ID, "Review the selected goal", bound); err != nil {
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
	if _, err = s.SubmitComposerDraft(t.Context(), a.ID, "task", []ComposerPreparation{tampered}); err == nil {
		t.Fatal("forged Goal source admitted")
	}
	if _, err = s.SubmitComposerDraft(t.Context(), b.ID, "task", []ComposerPreparation{selection}); err == nil {
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
	if _, err = s.SubmitComposerDraft(t.Context(), a.ID, "task", []ComposerPreparation{selection}); err == nil {
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
