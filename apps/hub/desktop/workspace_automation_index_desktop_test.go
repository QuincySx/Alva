// INPUT: Actual saved paused schedules, home controls and explicit scope navigation.
// OUTPUT: Home-reachable global index and existing-owner chooser without implicit execution.
// POS: [L7 Hub UI tests] Uses real product persistence and no OS window or model account.
//go:build desktop

package hubdesktop

import (
	"context"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/egoist/mygo/ui"
)

type workspaceAutomationIndexReadStore struct {
	product.ServiceStore
	port hub.AutomationBrowseStore
	gate *workspaceNavigationReadGate
}

func (s *workspaceAutomationIndexReadStore) ListAutomationIndex(ctx context.Context) ([]hub.Automation, error) {
	if err := s.gate.read(ctx); err != nil {
		return nil, err
	}
	return s.port.ListAutomationIndex(ctx)
}

func TestWorkspaceAutomationIndexSettlesWhileSettingsAreVisible(t *testing.T) {
	s, _ := fixture(t)
	gate := newWorkspaceNavigationReadGate(t)
	s.Store = &workspaceAutomationIndexReadStore{ServiceStore: s.Store, port: s.Store.(hub.AutomationBrowseStore), gate: gate}
	p := NewNativePage(s)
	p.ctx = t.Context()
	tester := ui.NewTester(workspaceStaticView(p), 1440, 960)
	gate.block.Store(true)
	p.openWorkspaceAutomations()
	awaitWorkspaceRead(t, gate)
	p.openWorkspaceSettingsCategory("general")
	gate.unblock()
	awaitWorkspaceBoundary(t, tester, func() bool { return !p.workspaceAutomationIndex.loading })
	if p.pane != "preferences" || p.workspaceAutomationIndex.problem != "" {
		t.Fatal("hidden index changed navigation or failed to settle", p.pane, p.workspaceAutomationIndex.problem)
	}
	p.setPane("")
	tester.Frame()
	if p.pane != "automation-index" || !tester.HasText("还没有自动化") {
		t.Fatal("return did not show the settled real index", p.pane, tester.Texts())
	}
}

func TestWorkspaceAutomationIndexUsesRealPausedPlansFromHome(t *testing.T) {
	s, requests := fixture(t)
	owner, err := s.Create(t.Context(), "自动化所属任务", "")
	if err != nil {
		t.Fatal(err)
	}
	store := s.Store.(hub.AutomationStore)
	plan, err := store.CreateAutomation(t.Context(), owner.ID, hub.AutomationDraft{Title: "真实暂停计划", Prompt: "must remain paused", IntervalMinutes: 5})
	if err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(s)
	tester := ui.NewTester(workspaceStaticView(p), 1440, 960)
	if err := tester.Click("Automations"); err != nil {
		t.Fatal(err)
	}
	deadline := time.Now().Add(3 * time.Second)
	for p.workspaceAutomationIndex.loading && time.Now().Before(deadline) {
		tester.Frame()
		time.Sleep(time.Millisecond)
	}
	tester.Frame()
	if p.pane != "automation-index" || len(p.workspaceAutomationIndex.items) != 1 || !tester.HasText(plan.Title) || !tester.HasText("已暂停") {
		t.Fatal("home did not show the real all-owner index", p.workspaceAutomationIndex.problem, tester.Texts())
	}
	if err := tester.Click("Manage automation " + plan.ID); err != nil {
		t.Fatal(err)
	}
	settleWorkspaceNavigation(t, p, tester)
	if p.pane != "workspace-scope" || p.settingsScope.destination != "automation" || p.settingsScope.candidate.ID != owner.ID || p.selected != "" {
		t.Fatal("index did not cold verify its exact owner for explicit selection", p.settingsScope)
	}
	if s.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("browsing automation started an engine")
	}
	stored, err := store.Automation(t.Context(), owner.ID, plan.ID)
	if err != nil || stored.Enabled || stored.RunningTurnID != "" || stored.Revision != plan.Revision {
		t.Fatal("index changed execution facts", stored, err)
	}
}
