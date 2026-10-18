// INPUT: Headless native schedule fields and actual paused-first private product store
// OUTPUT: Explicit CRUD, pinned billing consent, edit-pauses and two-step config-only deletion regressions
// POS: [L7 Hub product] Rendering/configuration never starts a timer, model Turn or catch-up job
//go:build desktop

package hubdesktop

import (
	"context"
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestNativeAutomationPausedCRUDAndIndependentBillingConsent(t *testing.T) {
	service, requests := fixture(t)
	ctx := context.Background()
	s, err := service.Create(ctx, "ordinary schedules", "")
	if err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(service)
	p.selected = s.ID
	p.detail, err = service.Store.Detail(ctx, s.ID)
	if err != nil {
		t.Fatal(err)
	}
	p.applyDetail()
	tester := ui.NewTester(p.Build, 1600, 1800)
	if err = tester.Click("Automations"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, p)
	for _, field := range []struct{ label, value string }{{"Automation title", "Explicit local review"}, {"Automation prompt", "Read only the saved task reference when due"}, {"Automation interval minutes", "5"}} {
		if err = tester.Click(field.label); err != nil {
			t.Fatal(err)
		}
		tester.Key(ui.Cmd, ui.KeyA)
		tester.Type(field.value)
	}
	if err = tester.Click("Save schedule paused"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, p)
	if p.problem != "" || len(p.automation.Entries) != 1 || p.automation.Entries[0].Enabled || p.automation.Entries[0].NextRunAt != 0 {
		t.Fatal("schedule create started or self-approved", p.problem)
	}
	if err = tester.Click("Confirm pinned account/billing and enable schedule"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, p)
	if p.problem != "" || !p.automation.Entries[0].Enabled || p.automation.Entries[0].NextRunAt <= 0 {
		t.Fatal("explicit schedule consent not saved", p.problem)
	}
	if err = tester.Click("Edit saved schedule"); err != nil {
		t.Fatal(err)
	}
	if err = tester.Click("Automation title"); err != nil {
		t.Fatal(err)
	}
	tester.Key(ui.Cmd, ui.KeyA)
	tester.Type("Edited and paused")
	if err = tester.Click("Save schedule paused"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, p)
	if p.problem != "" || p.automation.Entries[0].Enabled || p.automation.Entries[0].NextRunAt != 0 || p.automation.Entries[0].Title != "Edited and paused" {
		t.Fatal("editing did not pause actual schedule", p.problem)
	}
	if err = tester.Click("Review schedule config deletion"); err != nil {
		t.Fatal(err)
	}
	before, err := service.Automations.Store.ListAutomations(ctx, s.ID)
	if err != nil || len(before) != 1 {
		t.Fatal("review already deleted schedule", err)
	}
	if err = tester.Click("Confirm delete config only; keep session/history"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, p)
	if p.problem != "" || len(p.automation.Entries) != 0 {
		t.Fatal("explicit config-only deletion failed", p.problem)
	}
	if _, err = service.Store.Detail(ctx, s.ID); err != nil {
		t.Fatal("schedule deletion removed session", err)
	}
	if service.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("schedule UI/configuration executed a model")
	}
}
