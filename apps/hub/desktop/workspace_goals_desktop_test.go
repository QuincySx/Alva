// INPUT: Real product/native stores and actual MyGo Goal form controls without an OS window.
// OUTPUT: Cold scoped browsing/pages, stale-callback isolation and idempotent form/receipt association regressions.
// POS: [L7 Hub UI tests] Validates the shared Goal form without a model, GUI client or second Send path.
//go:build desktop

package hubdesktop

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"sync/atomic"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/platform/storage"
	"github.com/egoist/mygo/ui"
)

func savePageGoal(t *testing.T, p *NativePage, scope hub.GoalScope, name string) hub.Goal {
	t.Helper()
	id, err := hub.NewID("goal-fixture-request-")
	if err != nil {
		t.Fatal(err)
	}
	receipt, err := p.Service.Goals.Create(t.Context(), hub.GoalCreate{RequestID: id, Name: name, CompletionCriteria: "The original selected source is retained.", Scope: scope})
	if err != nil {
		t.Fatal(err)
	}
	return receipt.Goal
}

func TestWorkspaceSavedGoalsActualBrowseAndPagesAreColdAndSelectionKeepsDraft(t *testing.T) {
	p, tester := goalPage(t)
	scope := hub.GoalScope{Kind: hub.GoalProjectScope, Workspace: p.workspace}
	want := make(map[string]bool, 25)
	for index := range 25 {
		goal := savePageGoal(t, p, scope, fmt.Sprintf("Saved outcome %02d", index))
		want[goal.ID] = true
	}
	foreign := savePageGoal(t, p, hub.GoalScope{Kind: hub.GoalProjectScope, Workspace: t.TempDir()}, "Foreign project outcome")
	p.openWorkspaceGoal()
	tester.Frame()
	connectionClick(t, tester, "Saved goals")
	waitWorkspace(t, p, tester, func() bool { return !p.goal.loading })
	if !p.goal.browsing || len(p.goal.rows) != 20 || !p.goal.hasMore || len(p.composerSelections[""]) != 0 {
		t.Fatal("saved-goal list did not open a real bounded page", p.goal.problem)
	}
	for _, goal := range p.goal.rows {
		if !want[goal.ID] || goal.ID == foreign.ID {
			t.Fatal("foreign Goal entered project list", goal.ID)
		}
		delete(want, goal.ID)
	}
	connectionClick(t, tester, "Next saved goals")
	waitWorkspace(t, p, tester, func() bool { return !p.goal.loading })
	if len(p.goal.rows) != 5 || p.goal.hasMore || len(p.goal.previous) != 1 {
		t.Fatal("next page lost saved outcomes", p.goal.problem, len(p.goal.rows))
	}
	for _, goal := range p.goal.rows {
		if !want[goal.ID] {
			t.Fatal("page repeated a Goal", goal.ID)
		}
		delete(want, goal.ID)
	}
	if len(want) != 0 {
		t.Fatal("page hid saved Goals", want)
	}
	chosen := p.goal.rows[0]
	rows, err := p.Service.Store.ListSessions(t.Context(), 100)
	if err != nil || len(rows) != 0 || p.Service.Runtime.HasActive() || p.model.Draft != "keep the original message" {
		t.Fatal("cold pages created or executed a chat", err)
	}
	connectionClick(t, tester, "Select saved goal "+chosen.Name)
	waitWorkspace(t, p, tester, func() bool { return !p.goal.preparing })
	if p.goal.open || len(p.composerSelections[""]) != 1 || p.composerSelections[""][0].GoalRef.GoalID != chosen.ID || p.composerSelections[""][0].GoalRef.Reference.URI != "" || p.model.Draft != "keep the original message" || p.Service.Runtime.HasActive() {
		t.Fatal("selection changed the prompt, executed or lost the original identity", p.goal.problem)
	}
	rows, err = p.Service.Store.ListSessions(t.Context(), 100)
	if err != nil || len(rows) != 0 {
		t.Fatal("home saved-goal selection created a chat", err)
	}
}

func TestWorkspaceSavedGoalsSessionCanBrowseProjectAndExactSessionScopes(t *testing.T) {
	base, _ := goalPage(t)
	p := NewNativePage(base.Service)
	p.workspace = base.workspace
	a, err := p.Service.Create(t.Context(), "current Goal session", p.workspace)
	if err != nil {
		t.Fatal(err)
	}
	b, err := p.Service.Create(t.Context(), "other Goal session", a.Workspace)
	if err != nil {
		t.Fatal(err)
	}
	project := savePageGoal(t, p, hub.GoalScope{Kind: hub.GoalProjectScope, Workspace: a.Workspace}, "Shared project goal")
	current := savePageGoal(t, p, hub.GoalScope{Kind: hub.GoalSessionScope, SessionID: a.ID}, "Current session goal")
	savePageGoal(t, p, hub.GoalScope{Kind: hub.GoalSessionScope, SessionID: b.ID}, "Foreign session goal")
	p.selected = a.ID
	p.detail, err = p.Service.Store.Detail(t.Context(), a.ID)
	if err != nil {
		t.Fatal(err)
	}
	p.applyDetail()
	tester := ui.NewTester(workspaceStaticView(p), 960, 680)
	p.openSavedWorkspaceGoals()
	waitWorkspace(t, p, tester, func() bool { return !p.goal.loading })
	if len(p.goal.rows) != 1 || p.goal.rows[0] != current {
		t.Fatal("session list mixed foreign session or project scope", p.goal.rows)
	}
	connectionClick(t, tester, "Saved goal scope")
	tester.Key(0, ui.KeyEnd)
	tester.Key(0, ui.KeyEnter)
	waitWorkspace(t, p, tester, func() bool { return !p.goal.loading })
	if len(p.goal.rows) != 1 || p.goal.rows[0] != project {
		t.Fatal("session could not browse its applicable project Goals", p.goal.rows)
	}
	list, ok := tester.Find("Saved goal list")
	if !ok {
		t.Fatal("saved Goals have no independent bounded list")
	}
	for _, label := range []string{"Refresh saved goals", "Previous saved goals", "Next saved goals", "Close new goal"} {
		box, ok := tester.Find(label)
		if !ok || box.X < 0 || box.Y < 0 || box.X+box.W > 960 || box.Y+box.H > 680 || label != "Close new goal" && box.Y < list.Y+list.H {
			t.Fatal("saved-goal fixed action left the minimum viewport", label, list, box)
		}
	}
	history, err := p.Service.Store.History(t.Context(), a.ID, 0, 100)
	if err != nil || len(history) != 0 || p.Service.Runtime.HasActive() || len(p.composerSelections[a.ID]) != 0 {
		t.Fatal("cold session Goal scopes entered execution", history, err)
	}
}

type goalDelayedPages struct {
	hub.GoalStore
	hub.GoalPageStore
	workspace string
	started   chan struct{}
	release   chan struct{}
	finished  chan struct{}
}

func (s *goalDelayedPages) PageGoals(ctx context.Context, scope hub.GoalScope, cursor hub.GoalCursor, limit int) (hub.GoalPage, error) {
	if scope.Workspace == s.workspace {
		close(s.started)
		<-s.release
		defer close(s.finished)
		// Deliberately return a superseded read despite cancellation.
		ctx = context.WithoutCancel(ctx)
	}
	return s.GoalPageStore.PageGoals(ctx, scope, cursor, limit)
}

func TestWorkspaceSavedGoalsOldScopePageCannotReplaceNewProject(t *testing.T) {
	p, tester := goalPage(t)
	a := savePageGoal(t, p, hub.GoalScope{Kind: hub.GoalProjectScope, Workspace: p.workspace}, "Project A goal")
	b := savePageGoal(t, p, hub.GoalScope{Kind: hub.GoalProjectScope, Workspace: t.TempDir()}, "Project B goal")
	store := &goalDelayedPages{GoalStore: p.Service.Goals.Store, GoalPageStore: p.Service.Goals.Store.(hub.GoalPageStore), workspace: a.Scope.Workspace, started: make(chan struct{}), release: make(chan struct{}), finished: make(chan struct{})}
	p.Service.Goals.Store = store
	p.openSavedWorkspaceGoals()
	<-store.started
	p.workspace = b.Scope.Workspace
	tester.Frame() // Scope changes dismiss the old observer before reopening.
	p.openSavedWorkspaceGoals()
	waitWorkspace(t, p, tester, func() bool { return !p.goal.loading })
	if len(p.goal.rows) != 1 || p.goal.rows[0] != b {
		t.Fatal("new project did not read its own saved Goals", p.goal.problem)
	}
	close(store.release)
	<-store.finished
	tester.Frame()
	if !p.goal.open || p.goal.workspace != b.Scope.Workspace || len(p.goal.rows) != 1 || p.goal.rows[0] != b || len(p.composerSelections[""]) != 0 {
		t.Fatal("old project page covered the new scope", p.goal)
	}
}

type goalDelayedCreation struct {
	hub.GoalStore
	started chan struct{}
	release chan struct{}
}

func (s *goalDelayedCreation) CreateGoal(ctx context.Context, request hub.GoalCreate) (hub.GoalReceipt, error) {
	receipt, err := s.GoalStore.CreateGoal(ctx, request)
	close(s.started)
	<-s.release
	return receipt, err
}

func TestWorkspaceGoalLateCreationReceiptCannotBindChangedHomeProject(t *testing.T) {
	p, tester := goalPage(t)
	store := &goalDelayedCreation{GoalStore: p.Service.Goals.Store, started: make(chan struct{}), release: make(chan struct{})}
	p.Service.Goals.Store = store
	p.openWorkspaceGoal()
	tester.Frame()
	enterGoal(t, tester)
	connectionClick(t, tester, "Create goal")
	<-store.started
	projectB := t.TempDir()
	p.workspace = projectB
	tester.Frame()
	close(store.release)
	waitWorkspace(t, p, tester, func() bool { return !p.goal.saving })
	if p.goal.receipt == nil || p.goal.problem == "" || len(p.composerSelections[""]) != 0 || p.model.Draft != "keep the original message" || p.workspace != projectB {
		t.Fatal("old committed Goal receipt attached to a different project", p.goal)
	}
	rows, err := p.Service.Store.ListSessions(t.Context(), 8)
	if err != nil || len(rows) != 0 || p.Service.Runtime.HasActive() {
		t.Fatal("late Goal creation started work", err)
	}
	p.openWorkspaceGoal()
	if p.goal.workspace != projectB || p.goal.receipt != nil || p.goal.request.Name != "" {
		t.Fatal("old receipt overwrote the new project's form", p.goal)
	}
}

func goalPage(t *testing.T) (*NativePage, *ui.Tester) {
	t.Helper()
	s, _ := fixture(t)
	store, ok := s.Store.(hub.GoalStore)
	if !ok {
		t.Fatal("fixture GoalStore unavailable")
	}
	var err error
	s.Goals, err = product.NewGoalsService(store, s.Store, s.Repository)
	if err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(s)
	p.workspace = t.TempDir()
	p.model.Draft = "keep the original message"
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	return p, tester
}

func enterGoal(t *testing.T, tester *ui.Tester) {
	t.Helper()
	if err := tester.Click("Goal name"); err != nil {
		t.Fatal(err)
	}
	tester.Type("A useful scoped goal")
	if err := tester.Click("Goal completion criteria"); err != nil {
		t.Fatal(err)
	}
	tester.Type("The regression passes and the source is retained.")
}

func TestWorkspaceGoalActualFormCancelAndCreateDoNotRunOrCreateChat(t *testing.T) {
	p, tester := goalPage(t)
	p.openWorkspaceGoal()
	tester.Frame()
	enterGoal(t, tester)
	if err := tester.Click("Cancel new goal"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	scope := hub.GoalScope{Kind: hub.GoalProjectScope, Workspace: p.workspace}
	goals, err := p.Service.Goals.List(t.Context(), scope, 100)
	if err != nil || len(goals) != 0 || len(p.composerSelections[""]) != 0 {
		t.Fatal("cancel created or linked Goal", goals, err)
	}
	p.openWorkspaceGoal()
	tester.Frame()
	enterGoal(t, tester)
	if err = tester.Click("Create goal"); err != nil {
		t.Fatal(err)
	}
	waitWorkspace(t, p, tester, func() bool { return !p.goal.saving })
	if p.goal.open || p.goal.receipt == nil || len(p.composerSelections[""]) != 1 || p.model.Draft != "keep the original message" {
		t.Fatal("creation failed or changed message", p.goal.problem)
	}
	goals, err = p.Service.Goals.List(t.Context(), scope, 100)
	sessions, sessionsErr := p.Service.Store.ListSessions(t.Context(), 100)
	if err != nil || sessionsErr != nil || len(goals) != 1 || len(sessions) != 0 || p.Service.Runtime.HasActive() {
		t.Fatal("creation started chat/execution", err, sessionsErr)
	}
	p.composerSelections[""] = nil
	stored, err := p.Service.Goals.Read(t.Context(), goals[0].ID)
	if err != nil || stored != goals[0] {
		t.Fatal("removing draft reference deleted Goal", err)
	}
}

type goalFailArtifact struct{ product.GoalArtifacts }

func (r goalFailArtifact) PutArtifact(context.Context, string, string, storage.ArtifactInput) (storage.ArtifactInfo, error) {
	return storage.ArtifactInfo{}, errors.New("fixture source persistence failure")
}

func TestWorkspaceGoalCommittedReceiptAndFormSurviveSourceFailure(t *testing.T) {
	p, tester := goalPage(t)
	session, err := p.Service.Create(t.Context(), "existing session", p.workspace)
	if err != nil {
		t.Fatal(err)
	}
	p.selected = session.ID
	p.detail, err = p.Service.Store.Detail(t.Context(), session.ID)
	if err != nil {
		t.Fatal(err)
	}
	p.applyDetail()
	original := p.Service.Goals.Repository
	p.Service.Goals.Repository = goalFailArtifact{original}
	p.openWorkspaceGoal()
	tester.Frame()
	enterGoal(t, tester)
	if err = tester.Click("Create goal"); err != nil {
		t.Fatal(err)
	}
	waitWorkspace(t, p, tester, func() bool { return !p.goal.saving })
	if !p.goal.open || p.goal.receipt == nil || p.goal.problem == "" || p.goal.request.Name != "A useful scoped goal" || len(p.composerSelections[session.ID]) != 0 {
		t.Fatal("failed source lost form/real creation receipt", p.goal)
	}
	id := p.goal.receipt.Goal.ID
	p.Service.Goals.Repository = original
	if err = tester.Click("Create goal"); err != nil {
		t.Fatal(err)
	}
	waitWorkspace(t, p, tester, func() bool { return !p.goal.saving })
	goals, err := p.Service.Goals.List(t.Context(), p.goal.receipt.Goal.Scope, 100)
	if err != nil || len(goals) != 1 || goals[0].ID != id || p.goal.open || len(p.composerSelections[session.ID]) != 1 || p.Service.Runtime.HasActive() {
		t.Fatal("retry recreated or executed Goal", goals, p.goal.problem, err)
	}
}

func TestWorkspaceGoalWithoutSelectedProjectCannotCreate(t *testing.T) {
	p, tester := goalPage(t)
	p.workspace = ""
	p.openWorkspaceGoal()
	tester.Frame()
	enterGoal(t, tester)
	if _, ok := tester.Find("Goal requires selected project"); !ok {
		t.Fatal("missing explicit project scope explanation", tester.Texts())
	}
	if err := tester.Click("Create goal"); err == nil && p.goal.saving {
		t.Fatal("home Goal picked implicit scope")
	}
	sessions, err := p.Service.Store.ListSessions(t.Context(), 100)
	if err != nil || len(sessions) != 0 || p.goal.receipt != nil {
		t.Fatal("missing project created facts", err)
	}
}

func TestWorkspaceGoalMinimumWindowKeepsActionsOutsideScrollableErrors(t *testing.T) {
	base, _ := goalPage(t)
	p := NewNativePage(base.Service)
	p.workspace = base.workspace
	p.openWorkspaceGoal()
	p.goal.problem = strings.Repeat("保存失败，请检查数据库。", 80)
	p.goal.receipt = &hub.GoalReceipt{Goal: hub.Goal{ID: "goal-real-committed-identity", Revision: 1}}
	tester := ui.NewTester(workspaceStaticView(p), 960, 680)
	content, ok := tester.Find("Goal form details")
	if !ok {
		t.Fatal("goal details have no independent scroll viewport")
	}
	for _, label := range []string{"Close new goal", "Cancel new goal", "Create goal"} {
		box, ok := tester.Find(label)
		if !ok || box.X < 0 || box.Y < 0 || box.X+box.W > 960 || box.Y+box.H > 680 {
			t.Fatal("Goal action outside minimum viewport", label, box)
		}
		if label != "Close new goal" && box.Y < content.Y+content.H {
			t.Fatal("long errors covered fixed footer", label, content, box)
		}
	}
}

type goalUnknownReceipt struct {
	hub.GoalStore
	calls atomic.Int32
}

func (s *goalUnknownReceipt) CreateGoal(ctx context.Context, request hub.GoalCreate) (hub.GoalReceipt, error) {
	receipt, err := s.GoalStore.CreateGoal(ctx, request)
	if err != nil {
		return hub.GoalReceipt{}, err
	}
	if s.calls.Add(1) == 1 {
		return hub.GoalReceipt{}, errors.New("fixture creation receipt interrupted after commit")
	}
	return receipt, nil
}

func TestWorkspaceGoalUnknownReceiptWaitsForExplicitIdempotentRetry(t *testing.T) {
	p, tester := goalPage(t)
	store := &goalUnknownReceipt{GoalStore: p.Service.Goals.Store}
	p.Service.Goals.Store = store
	p.openWorkspaceGoal()
	tester.Frame()
	enterGoal(t, tester)
	requestID := p.goal.request.RequestID
	if err := tester.Click("Create goal"); err != nil {
		t.Fatal(err)
	}
	waitWorkspace(t, p, tester, func() bool { return !p.goal.saving })
	goals, err := p.Service.Goals.List(t.Context(), p.goal.request.Scope, 100)
	if err != nil || len(goals) != 1 || store.calls.Load() != 1 || p.goal.receipt != nil || !p.goal.open || p.goal.request.RequestID != requestID || len(p.composerSelections[""]) != 0 {
		t.Fatal("unknown receipt replayed or lost draft", goals, store.calls.Load(), err)
	}
	if err = tester.Click("Create goal"); err != nil {
		t.Fatal(err)
	}
	waitWorkspace(t, p, tester, func() bool { return !p.goal.saving })
	if store.calls.Load() != 2 || p.goal.receipt == nil || !p.goal.receipt.Replayed || p.goal.receipt.Goal.ID != goals[0].ID || len(p.composerSelections[""]) != 1 {
		t.Fatal("explicit retry lost the original committed Goal", p.goal)
	}
}
