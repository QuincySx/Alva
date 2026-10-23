// INPUT: Real product SQLite, native composer controls and captured typed submission selections.
// OUTPUT: Cold home intent, failed admission retention and late-ack draft isolation regressions.
// POS: [L7 Hub UI tests] Uses no external engine, OS window or invented successful execution.
//go:build desktop

package hubdesktop

import (
	"testing"

	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/egoist/mygo/ui"
)

func TestWorkspacePreparedHomeFailureRetainsCommittedIdentityAndSelection(t *testing.T) {
	service, _ := fixture(t)
	if err := service.Runtime.Close(t.Context()); err != nil {
		t.Fatal(err)
	} // Real admission rejection.
	p := NewNativePage(service)
	prepared, err := service.PrepareComposerSelection(t.Context(), "", product.ComposerSelectionRequest{EntryID: "builtin:plan"})
	if err != nil {
		t.Fatal(err)
	}
	p.composerSelections = map[string][]product.ComposerPreparation{"": {prepared}}
	p.model.Draft = prepared.Text
	tester := ui.NewTester(p.Build, 1240, 850)
	rows, err := service.Store.ListSessions(t.Context(), 8)
	if err != nil || len(rows) != 0 || service.Runtime.HasActive() {
		t.Fatal("preparing command created a session", err)
	}
	if err = tester.Click("Send"); err != nil {
		t.Fatal(err)
	}
	waitWorkspace(t, p, tester, func() bool { return p.selected != "" && p.problem != "" && !p.busy && !p.polling })
	rows, err = service.Store.ListSessions(t.Context(), 8)
	settings, settingsErr := service.Store.Settings(t.Context(), p.selected)
	if err != nil || settingsErr != nil || len(rows) != 1 || !settings.PlanMode || p.model.Draft != prepared.Text || len(p.composerSelections[p.selected]) != 1 || service.Runtime.HasActive() {
		t.Fatal("failed actual admission lost identity, typed intent or prompt", rows, settings, err, settingsErr)
	}
}

func TestWorkspacePreparedAcknowledgmentPreservesReplacementSelection(t *testing.T) {
	service, _ := fixture(t)
	p := NewNativePage(service)
	old, err := service.PrepareComposerSelection(t.Context(), "", product.ComposerSelectionRequest{EntryID: "builtin:review"})
	if err != nil {
		t.Fatal(err)
	}
	p.composerSelections = map[string][]product.ComposerPreparation{"": {old}}
	frozen := p.captureWorkspaceSelections()
	newer := old
	newer.Arguments = map[string]string{"request": "new task"}
	newer.Text += "new task"
	p.composerSelections[""] = []product.ComposerPreparation{newer}
	p.consumeWorkspaceSelections("", frozen)
	if len(p.composerSelections[""]) != 1 || p.composerSelections[""][0].Text != newer.Text {
		t.Fatal("old acknowledgement consumed the replacement draft selection")
	}
	p.consumeWorkspaceSelections("", p.captureWorkspaceSelections())
	if len(p.composerSelections[""]) != 0 {
		t.Fatal("acknowledged selection retained")
	}
}
