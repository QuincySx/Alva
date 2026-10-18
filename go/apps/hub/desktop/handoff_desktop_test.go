// INPUT: Actual Team/Turn handoff transaction and headless MyGo explicit read controls
// OUTPUT: Exact durable snapshot display with no Worker-body transfer, execution or foreign-session grant
// POS: [L7 Hub product] UI evidence reading is independent of running/restoring an engine
//go:build desktop

package hubdesktop

import (
	"context"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestNativeHandoffEvidenceReadsExactSavedTeamIndexOnly(t *testing.T) {
	service, requests := fixture(t)
	ctx := context.Background()
	lead, err := service.Create(ctx, "handoff", "")
	if err != nil {
		t.Fatal(err)
	}
	team, err := service.Teams.Store.CreateTeam(ctx, lead.ID)
	if err != nil {
		t.Fatal(err)
	}
	worker, err := service.AddWorker(ctx, lead.ID, team.Team.ID, "Worker", service.DefaultTarget, true)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = service.Teams.Store.EnqueueWorker(ctx, lead.ID, team.Team.ID, worker.ID, "PRIVATE WORKER SCOPE not a handoff body", nil); err != nil {
		t.Fatal(err)
	}
	claim, err := service.Store.BeginTurn(ctx, lead.ID, "ordinary local fixture admission")
	if err != nil {
		t.Fatal(err)
	}
	if claim.Handoff == nil {
		t.Fatal("actual Team snapshot was not persisted")
	}
	p := NewNativePage(service)
	p.selected = lead.ID
	p.detail, err = service.Store.Detail(ctx, lead.ID)
	if err != nil {
		t.Fatal(err)
	}
	entries, err := service.Store.History(ctx, lead.ID, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	p.transcript.Replace(entries)
	p.applyDetail()
	tester := ui.NewTester(p.Build, 1500, 1600)
	if err = tester.Click("Handoff evidence"); err != nil {
		t.Fatal(err)
	}
	if p.handoffText != "" {
		t.Fatal("opening handoff page implicitly read detail")
	}
	if err = tester.Click("Read handoff " + claim.Handoff.ID); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, p)
	if p.problem != "" || !strings.Contains(p.handoffText, team.Team.ID) || strings.Contains(p.handoffText, "PRIVATE WORKER SCOPE") {
		t.Fatal("wrong exact snapshot", p.problem, p.handoffText)
	}
	other, err := service.Create(ctx, "foreign", "")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = service.Store.ReadHandoff(ctx, other.ID, claim.Handoff.ID); err == nil {
		t.Fatal("foreign handoff exposed")
	}
	if service.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("handoff evidence started/restored engine")
	}
}
