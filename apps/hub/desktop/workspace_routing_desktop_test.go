// INPUT: Actual native route controls and committed temporary connection revisions.
// OUTPUT: Advanced fallback uses shared admission while stale saved branches remain readable.
// POS: [L7 Hub UI testing] Retained history selection cannot turn an unavailable account into execution.
//go:build desktop

package hubdesktop

import (
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/egoist/mygo/ui"
)

func TestWorkspaceAdvancedRoutingRejectsStaleFallbackAndRetainsHistory(t *testing.T) {
	s, _ := connectionsFixture(t)
	entry, err := s.Connections.Save(t.Context(), hub.ConnectionUpdate{Name: "history account", Protocol: "openai_chat", Endpoint: "http://127.0.0.1:1/v1/chat/completions", Auth: "none", Enabled: true, Models: []hub.ConnectionModel{{ID: "retained-model", Enabled: true}}})
	if err != nil {
		t.Fatal(err)
	}
	session, err := s.Create(t.Context(), "retained route", "")
	if err != nil {
		t.Fatal(err)
	}
	target := entry.Target(entry.Models[0])
	branch, err := s.SwitchTarget(t.Context(), session.ID, target, "configured")
	if err != nil {
		t.Fatal(err)
	}
	other := s.DefaultTarget
	other.EngineID = "other-engine" // the retained branch is the connection's engine branch
	if _, err = s.Store.Switch(t.Context(), session.ID, other, "return"); err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(s)
	p.selected = session.ID
	p.pane = "routing"
	p.detail, err = s.Store.Detail(t.Context(), session.ID)
	if err != nil {
		t.Fatal(err)
	}
	p.applyDetail()
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	if _, err = s.Connections.SetEnabled(t.Context(), entry.ID, entry.Revision, false); err != nil {
		t.Fatal(err)
	}
	// Exercise a click handler captured before the shared connection revision
	// changed; a fresh Build already removes the unavailable route from its list.
	p.setFallback(&target)
	waitWorkspace(t, p, tester, func() bool { return !p.busy })
	detail, err := s.Store.Detail(t.Context(), session.ID)
	if err != nil || len(detail.Fallback) != 0 || p.problem == "" {
		t.Fatal("advanced fallback accepted a stale route", detail.Fallback, p.problem, err)
	}
	if err = tester.Click("Use " + target.EngineID + " · " + target.ModelID); err != nil {
		t.Fatal(err)
	}
	waitWorkspace(t, p, tester, func() bool { return !p.busy && p.detail.Session.ActiveBranchID == branch.ID })
	if p.workspaceTargetProblem() == "" || s.Runtime.HasActive() {
		t.Fatal("history selection granted execution on a disabled connection")
	}
}
