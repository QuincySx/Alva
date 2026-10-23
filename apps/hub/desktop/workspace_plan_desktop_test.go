// INPUT: Real product/native SQLite service, actual MyGo Plan menu controls and bound asynchronous settings replies.
// OUTPUT: Cold home intent, saved-session persistence, draft preservation and running/waiting/external disabled regressions.
// POS: [L7 native UI tests] Plan controls never execute an Agent or grant tool authority.
//go:build desktop

package hubdesktop

import (
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/egoist/mygo/ui"
)

func planMenuTester(p *NativePage) *ui.Tester {
	return ui.NewTester(func(c *ui.Context) {
		for {
			select {
			case update := <-p.updates:
				update()
			default:
				ui.Column(c).FillWidth().Children(func() { p.buildWorkspacePlanMenuItem(c) })
				return
			}
		}
	}, 400, 180)
}

func TestWorkspacePlanHomeMenuStoresOnlyLocalIntent(t *testing.T) {
	service, requests := fixture(t)
	p := NewNativePage(service)
	p.model.Draft = "保留草稿和光标"
	tester := planMenuTester(p)
	if err := tester.Click("Plan mode"); err != nil {
		t.Fatal(err)
	}
	if !p.workspacePlanEnabled() || p.model.Draft != "保留草稿和光标" || p.busy {
		t.Fatal("home choice was not retained independently of the draft")
	}
	if err := tester.Click("Plan mode"); err != nil {
		t.Fatal(err)
	}
	if p.workspacePlanEnabled() {
		t.Fatal("home Plan choice did not toggle off")
	}
	sessions, err := service.Store.ListSessions(t.Context(), 8)
	if err != nil || len(sessions) != 0 || len(requests) != 0 || service.Runtime.HasActive() {
		t.Fatal("Plan menu created a session or executed", err)
	}
	p.composer.target = hub.Target{EngineID: "external"}
	tester.Frame()
	if err = tester.Click("Plan mode"); err != nil {
		t.Fatal(err)
	}
	if p.workspacePlanEnabled() || p.workspacePlanCanChange() {
		t.Fatal("external route accepted native-only Plan intent")
	}
}

func TestWorkspacePlanSavedChoiceUsesSharedServiceAndFreezesActiveRecipes(t *testing.T) {
	service, requests := fixture(t)
	p := NewNativePage(service)
	session, err := service.Create(t.Context(), "saved plan", "")
	if err != nil {
		t.Fatal(err)
	}
	p.selected = session.ID
	p.detail, err = service.Store.Detail(t.Context(), session.ID)
	if err != nil {
		t.Fatal(err)
	}
	p.model.Draft = "规划草稿不会被保存策略清空"
	tester := planMenuTester(p)
	if p.workspacePlanCanChange() {
		t.Fatal("unloaded policy offered a misleading editable default")
	}
	p.acceptWorkspacePlanSettings(session.ID, session.ActiveBranchID, false)
	tester.Frame()
	if err = tester.Click("Plan mode"); err != nil {
		t.Fatal(err)
	}
	deadline := time.Now().Add(5 * time.Second)
	for p.busy && time.Now().Before(deadline) {
		tester.Frame()
		time.Sleep(time.Millisecond)
	}
	if p.busy || p.problem != "" || !p.workspacePlanEnabled() {
		t.Fatal("actual saved Plan edit did not settle", p.problem)
	}
	value, err := service.Store.Settings(t.Context(), session.ID)
	if err != nil || !value.PlanMode {
		t.Fatal("menu only changed presentation state", err)
	}
	if p.model.Draft != "规划草稿不会被保存策略清空" || len(requests) != 0 || service.Runtime.HasActive() {
		t.Fatal("policy edit lost draft or executed")
	}
	for _, status := range []engine.Outcome{hub.Running, engine.Waiting} {
		p.detail.Session.Status = status
		tester.Frame()
		if err = tester.Click("Plan mode"); err != nil {
			t.Fatal(err)
		}
		if !p.workspacePlanEnabled() || p.workspacePlanCanChange() || p.busy {
			t.Fatal("active execution recipe was offered or changed", status)
		}
	}
	p.detail.Session.Status = ""
	p.acceptWorkspacePlanSettings(session.ID, "previous-branch", false)
	if !p.workspacePlanEnabled() {
		t.Fatal("stale branch settings overwrote selected policy")
	}
	p.detail.Session.ActiveBranchID = "new-branch"
	if p.workspacePlanEnabled() || p.workspacePlanCanChange() {
		t.Fatal("new branch borrowed a previously loaded policy snapshot")
	}
}
