// INPUT: A persisted home Goal, actual first Send, complete local demo assembly and a closed-owner failure fixture.
// OUTPUT: Saved/reopened/reselected reference-only execution and pre-Create scope rejection preserving drafts and original facts.
// POS: [L7 Hub UI tests] Exercises the real shared first-send chain without an OS window or external model account.
//go:build desktop

package hubdesktop

import (
	"context"
	"encoding/json"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/apps/hub/product"
	view "github.com/QuincySx/Alva/apps/ui/chat"
	"github.com/egoist/mygo/ui"
)

func TestWorkspaceGoalFirstSendBindsSourceAndFailureRetainsDraft(t *testing.T) {
	p, tester := goalPage(t)
	p.openWorkspaceGoal()
	tester.Frame()
	enterGoal(t, tester)
	connectionClick(t, tester, "Create goal")
	waitWorkspace(t, p, tester, func() bool { return !p.goal.saving })
	if p.goal.receipt == nil {
		t.Fatal(p.goal.problem)
	}
	goal := p.goal.receipt.Goal
	if err := p.Service.Runtime.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	connectionClick(t, tester, "Send")
	waitWorkspace(t, p, tester, func() bool { return p.selected != "" && !p.busy && !p.polling && p.problem != "" })
	id := p.selected
	if len(p.composerSelections[id]) != 1 || p.model.Draft != "keep the original message" {
		t.Fatal("failed admission lost Goal or prompt")
	}
	rebound, err := p.Service.RebindComposerGoals(t.Context(), id, p.captureWorkspaceSelections())
	if err != nil || len(rebound) != 1 || rebound[0].GoalRef.SessionID != id || rebound[0].GoalRef.Reference.URI == "" {
		t.Fatal("home Goal did not bind to actual session/source", err)
	}
	connectionClick(t, tester, "Send")
	waitWorkspace(t, p, tester, func() bool { return !p.busy && !p.polling })
	sessions, err := p.Service.Store.ListSessions(t.Context(), 8)
	goals, goalErr := p.Service.Goals.List(t.Context(), goal.Scope, 8)
	if err != nil || goalErr != nil || len(sessions) != 1 || len(goals) != 1 || goals[0].ID != goal.ID || p.selected != id {
		t.Fatal("failed Send retry duplicated business identity", err, goalErr)
	}
}

func TestWorkspaceGoalExternalAgentCannotStartPartialFlow(t *testing.T) {
	p, tester := goalPage(t)
	foreign := p.Service.DefaultTarget
	foreign.EngineID = "claude"
	p.Service.Targets = append(p.Service.Targets, foreign)
	p.composer.target = foreign
	p.openWorkspaceGoal()
	if p.goal.open {
		t.Fatal("external Agent opened an unusable Goal form")
	}
	p.composer.target = p.Service.DefaultTarget
	p.openWorkspaceGoal()
	tester.Frame()
	enterGoal(t, tester)
	connectionClick(t, tester, "Create goal")
	waitWorkspace(t, p, tester, func() bool { return !p.goal.saving })
	p.composer.target = foreign
	if p.workspaceTargetProblem() == "" {
		t.Fatal("home Goal allowed an incompatible route")
	}
	p.sendWorkspaceDraft()
	rows, err := p.Service.Store.ListSessions(t.Context(), 8)
	if err != nil || len(rows) != 0 || p.busy || p.selected != "" || len(p.composerSelections[""]) != 1 {
		t.Fatal("known incompatible Goal created a partial chat", err)
	}
}

func TestWorkspaceGoalChangedHomeProjectRejectsBeforeSessionCreateAndAllowsReselection(t *testing.T) {
	p, tester := goalPage(t)
	p.openWorkspaceGoal()
	tester.Frame()
	enterGoal(t, tester)
	connectionClick(t, tester, "Create goal")
	waitWorkspace(t, p, tester, func() bool { return !p.goal.saving })
	selected := p.composerSelections[""][0]
	goalA := p.goal.receipt.Goal
	p.workspace = t.TempDir()
	p.sendWorkspaceDraft()
	waitWorkspace(t, p, tester, func() bool { return !p.busy && p.problem != "" })
	rows, err := p.Service.Store.ListSessions(t.Context(), 8)
	if err != nil || len(rows) != 0 || p.selected != "" || p.model.Draft != "keep the original message" || len(p.composerSelections[""]) != 1 || p.composerSelections[""][0].GoalRef.GoalID != goalA.ID || p.Service.Runtime.HasActive() {
		t.Fatal("scope failure created a project-B session or lost the original draft", rows, err)
	}
	connectionClick(t, tester, "Remove prepared reference "+selected.Text)
	goalB := savePageGoal(t, p, hub.GoalScope{Kind: hub.GoalProjectScope, Workspace: p.workspace}, "Project B replacement")
	p.openSavedWorkspaceGoals()
	waitWorkspace(t, p, tester, func() bool { return !p.goal.loading })
	connectionClick(t, tester, "Select saved goal "+goalB.Name)
	waitWorkspace(t, p, tester, func() bool { return !p.goal.preparing })
	if len(p.composerSelections[""]) != 1 || p.composerSelections[""][0].GoalRef.GoalID != goalB.ID || p.model.Draft != "keep the original message" {
		t.Fatal("invalid reference could not be removed and reselected", p.goal.problem)
	}
	if original, err := p.Service.Goals.Read(t.Context(), goalA.ID); err != nil || original != goalA {
		t.Fatal("scope rejection removed the original Goal", err)
	}
}

func TestWorkspaceGoalHomeFormSendRunsActualDemoAndRetainsSource(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "private")
	app, err := bootstrap.Open(t.Context(), bootstrap.Config{Demo: true, DataDirectory: directory, OpenStores: bootstrap.LocalStores})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := app.Close(context.Background()); err != nil {
			t.Error(err)
		}
	})
	p := NewNativePage(app.Service)
	p.workspace = t.TempDir()
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	connectionClick(t, tester, "Message")
	tester.Type("Review the explicitly selected Goal")
	connectionClick(t, tester, "Add attachments")
	connectionClick(t, tester, "New Goal")
	enterGoal(t, tester)
	connectionClick(t, tester, "Create goal")
	waitWorkspace(t, p, tester, func() bool { return !p.goal.saving })
	if p.goal.receipt == nil || p.goal.open || len(p.composerSelections[""]) != 1 {
		t.Fatal("actual Goal form did not create and prepare the home reference", p.goal.problem)
	}
	goal := p.goal.receipt.Goal
	sessions, err := app.Store.ListSessions(t.Context(), 8)
	if err != nil || len(sessions) != 0 || app.Runtime.HasActive() || p.composerSelections[""][0].GoalRef.Reference.URI != "" {
		t.Fatal("Goal creation executed or created a chat/native source before Send", err)
	}
	// Reopen with a fresh window draft. The saved Goal must be discoverable and
	// selected again through the same cold browser before explicit Send.
	if err = app.Close(context.Background()); err != nil {
		t.Fatal(err)
	}
	app, err = bootstrap.Open(t.Context(), bootstrap.Config{Demo: true, DataDirectory: directory, OpenStores: bootstrap.LocalStores})
	if err != nil {
		t.Fatal(err)
	}
	p = NewNativePage(app.Service)
	p.workspace, p.model.Draft = goal.Scope.Workspace, "Review the explicitly selected Goal"
	tester = ui.NewTester(workspaceStaticView(p), 1240, 850)
	p.openSavedWorkspaceGoals()
	waitWorkspace(t, p, tester, func() bool { return !p.goal.loading })
	if len(p.goal.rows) != 1 || p.goal.rows[0] != goal || len(p.composerSelections[""]) != 0 || app.Runtime.HasActive() {
		t.Fatal("reopened saved-goal browser lost the original or executed", p.goal.problem)
	}
	connectionClick(t, tester, "Select saved goal "+goal.Name)
	waitWorkspace(t, p, tester, func() bool { return !p.goal.preparing })
	sessions, err = app.Store.ListSessions(t.Context(), 8)
	if err != nil || len(sessions) != 0 || len(p.composerSelections[""]) != 1 || p.composerSelections[""][0].GoalRef.GoalID != goal.ID || app.Runtime.HasActive() {
		t.Fatal("reselection created a session or executed", err)
	}
	connectionClick(t, tester, "Send")
	waitWorkspace(t, p, tester, func() bool {
		if p.selected == "" {
			return false
		}
		approvals, err := app.Service.PendingApprovals(t.Context(), p.selected)
		if err != nil {
			t.Fatal(err)
		}
		// Only this deterministic demo tool call receives explicit test consent.
		for _, approval := range approvals {
			if err = app.Service.RespondApproval(t.Context(), p.selected, approval.ID, true); err != nil {
				t.Fatal(err)
			}
		}
		return !p.busy && !p.polling && !app.Runtime.HasActive() && p.model.Status == view.Completed && len(p.model.Messages) >= 2
	})
	sessions, err = app.Store.ListSessions(t.Context(), 8)
	if err != nil || len(sessions) != 1 || p.model.Draft != "" || len(p.composerSelections[p.selected]) != 0 {
		t.Fatal("successful Goal Send lost its acknowledgement or duplicated a session", err)
	}
	detail, err := app.Store.Detail(t.Context(), p.selected)
	if err != nil {
		t.Fatal(err)
	}
	branch, err := product.ActiveBranch(detail)
	if err != nil {
		t.Fatal(err)
	}
	artifacts, err := app.Repository.PageArtifacts(t.Context(), p.selected, branch.NativeSessionID, "goal-reference", "", 0, 0, 8)
	if err != nil || len(artifacts.Entries) != 1 || artifacts.HasMore {
		t.Fatal("first Send did not retain one actual Goal source artifact", artifacts, err)
	}
	source := artifacts.Entries[0].Info
	history, err := app.Store.History(t.Context(), p.selected, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	turns, inputs := map[string]bool{}, 0
	for _, entry := range history {
		if entry.TurnID != "" {
			turns[entry.TurnID] = true
		}
		if entry.Input != "" {
			inputs++
			if !strings.Contains(entry.Input, "Review the explicitly selected Goal") || !strings.Contains(entry.Input, source.Reference().URI) || !strings.Contains(entry.Input, goal.ID) || strings.Contains(entry.Input, goal.CompletionCriteria) {
				t.Fatal("actual admitted prompt did not use the retained Goal reference", entry.Input)
			}
		}
	}
	if inputs != 1 || len(turns) != 1 || turns[goal.ID] {
		t.Fatal("Goal fact became an extra execution instead of a reference", inputs, turns)
	}
	settings, err := app.Store.Settings(t.Context(), p.selected)
	if err != nil || settings.PermissionPolicy.Effective() != hub.PermissionAsk || settings.PlanMode {
		t.Fatal("Goal Send changed execution permissions or Plan", err)
	}
	readOriginal := func(repositoryApp *bootstrap.Application) {
		t.Helper()
		stored, err := repositoryApp.Service.Goals.Read(t.Context(), goal.ID)
		if err != nil || stored != goal {
			t.Fatal("execution changed the desired-outcome fact", err)
		}
		info, raw, err := repositoryApp.Repository.ReadArtifactFull(t.Context(), p.selected, branch.NativeSessionID, source.Reference().URI, 96<<10)
		if err != nil || info != source {
			t.Fatal("Goal source did not survive execution/reopen", err)
		}
		var original hub.Goal
		if err = json.Unmarshal(raw, &original); err != nil || original != goal {
			t.Fatal("retained artifact lost original Goal fields", err)
		}
	}
	readOriginal(app)
	if err = app.Close(context.Background()); err != nil {
		t.Fatal(err)
	}
	reopened, err := bootstrap.Open(t.Context(), bootstrap.Config{Demo: true, DataDirectory: directory, OpenStores: bootstrap.LocalStores})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := reopened.Close(context.Background()); err != nil {
			t.Error(err)
		}
	})
	readOriginal(reopened)
	if reopened.Runtime.HasActive() {
		t.Fatal("reopening a completed Goal reference executed work")
	}
}
