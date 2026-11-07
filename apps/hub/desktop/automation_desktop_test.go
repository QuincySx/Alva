// INPUT: Headless native schedule fields and actual paused-first private product store
// OUTPUT: Explicit CRUD, billing consent, stale-save draft retention and two-step config-only deletion regressions
// POS: [L7 Hub product] Rendering/configuration never starts a timer, model Turn or catch-up job
//go:build desktop

package hubdesktop

import (
	"context"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/egoist/mygo/ui"
)

func TestNativeAutomationStaleSavePreservesDraftAndReopensCommittedFacts(t *testing.T) {
	service, requests := fixture(t)
	ctx := t.Context()
	s, err := service.Create(ctx, "conflicting schedule edit", "")
	if err != nil {
		t.Fatal(err)
	}
	a, err := service.Automations.Store.CreateAutomation(ctx, s.ID, hub.AutomationDraft{Title: "saved", Prompt: "original prompt", IntervalMinutes: 5})
	if err != nil {
		t.Fatal(err)
	}
	page := NewNativePage(service)
	page.selected = s.ID
	page.detail, err = service.Store.Detail(ctx, s.ID)
	if err != nil {
		t.Fatal(err)
	}
	page.applyDetail()
	tester := ui.NewTester(workspaceStaticView(page), 1240, 1200)
	page.openAutomations()
	settlePage(t, tester, page)
	if err = tester.Click("Edit saved schedule"); err != nil {
		t.Fatal(err)
	}
	if err = tester.Click("Automation prompt"); err != nil {
		t.Fatal(err)
	}
	tester.Key(ui.Cmd, ui.KeyA)
	tester.Type("my unsaved recovery draft")
	a.Prompt = "committed in another window"
	if _, err = service.Automations.Store.UpdateAutomation(ctx, s.ID, a.ID, a.Revision, a.AutomationDraft); err != nil {
		t.Fatal(err)
	}
	if err = tester.Click("Save schedule paused"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if page.problem == "" || page.automation.Prompt != "my unsaved recovery draft" || page.automation.EditID != a.ID || page.automation.Revision != a.Revision {
		t.Fatal("stale save discarded or claimed to commit its draft", page.automation, page.problem)
	}
	reopened := NewNativePage(service)
	reopened.selected = s.ID
	reopened.detail = page.detail
	reopened.applyDetail()
	reopenedTester := ui.NewTester(workspaceStaticView(reopened), 1240, 1200)
	reopened.openAutomations()
	settlePage(t, reopenedTester, reopened)
	if len(reopened.automation.Entries) != 1 || reopened.automation.Entries[0].Prompt != a.Prompt || reopened.automation.Entries[0].Enabled || len(requests) != 0 || service.Runtime.HasActive() {
		t.Fatal("reopened pane restored an uncommitted draft as execution facts", reopened.automation)
	}
}

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
	tester := ui.NewTester(workspaceStaticView(p), 1600, 1800)
	if err = workspaceTestMenu(tester, "自动化"); err != nil {
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

func TestNativeAutomationDeletionReviewDoesNotApproveRefreshedRevision(t *testing.T) {
	service, requests := fixture(t)
	ctx := t.Context()
	session, err := service.Create(ctx, "revision-pinned deletion", "")
	if err != nil {
		t.Fatal(err)
	}
	a, err := service.Automations.Store.CreateAutomation(ctx, session.ID, hub.AutomationDraft{Title: "reviewed", Prompt: "original", IntervalMinutes: 5})
	if err != nil {
		t.Fatal(err)
	}
	page := NewNativePage(service)
	page.selected = session.ID
	page.detail, err = service.Store.Detail(ctx, session.ID)
	if err != nil {
		t.Fatal(err)
	}
	page.applyDetail()
	tester := ui.NewTester(workspaceStaticView(page), 1240, 1200)
	page.openAutomations()
	settlePage(t, tester, page)
	if err = tester.Click("Review schedule config deletion"); err != nil {
		t.Fatal(err)
	}
	a.Title, a.Prompt = "changed in another window", "different task"
	if _, err = service.Automations.Store.UpdateAutomation(ctx, session.ID, a.ID, a.Revision, a.AutomationDraft); err != nil {
		t.Fatal(err)
	}
	if err = tester.Click("Refresh saved schedules"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if tester.HasText("Confirm delete config only; keep session/history") {
		t.Fatal("old deletion review approved a different saved schedule revision")
	}
	if err = tester.Click("Review schedule config deletion"); err != nil {
		t.Fatal(err)
	}
	if err = tester.Click("Confirm delete config only; keep session/history"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if page.problem != "" || len(page.automation.Entries) != 0 || service.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("fresh config-only confirmation failed or executed", page.problem)
	}
}
