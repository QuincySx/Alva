// INPUT: Actual native permission controls, untouched home drafts and shared saved-session settings.
// OUTPUT: Full confirmation/cancellation, Plan reset and explicit policy persistence without model execution.
// POS: [L7 UI tests] Checks real click paths and successful-store-only presentation.
//go:build desktop

package hubdesktop

import (
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/egoist/mygo/ui"
)

func TestWorkspacePermissionConfirmationAndPlanReset(t *testing.T) {
	service, requests := fixture(t)
	p := NewNativePage(service)
	p.model.Draft = "保留草稿"
	tester := ui.NewTester(p.Build, 1240, 850)
	click := func(label string) {
		t.Helper()
		if err := tester.Click(label); err != nil {
			t.Fatal(err)
		}
	}
	click("Permission policy")
	click("完全访问")
	_, scopeVisible := tester.Find("Full permission scope")
	if p.workspacePermissionPolicy() != hub.PermissionAsk || !p.composer.confirmFull || !scopeVisible {
		t.Fatal("Full granted before scope confirmation")
	}
	click("Cancel Full permission")
	if p.workspacePermissionPolicy() != hub.PermissionAsk {
		t.Fatal("cancel changed authority")
	}
	click("完全访问")
	click("Confirm Full permission")
	if p.workspacePermissionPolicy() != hub.PermissionFull || p.composer.permissionOpen {
		t.Fatal("explicit Full choice not retained")
	}
	p.setWorkspacePlanMode(true)
	tester.Frame()
	if !p.workspacePlanEnabled() || p.workspacePermissionPolicy() != hub.PermissionAsk {
		t.Fatal("Plan retained latent Full")
	}
	p.setWorkspacePlanMode(false)
	tester.Frame()
	if p.workspacePermissionPolicy() != hub.PermissionAsk {
		t.Fatal("exiting Plan restored Full")
	}
	click("Permission policy")
	click("自动审批")
	if p.workspacePermissionPolicy() != hub.PermissionAuto || p.workspacePlanEnabled() || p.model.Draft != "保留草稿" || len(requests) != 0 || service.Runtime.HasActive() {
		t.Fatal("policy choice changed draft or executed")
	}
	p.setWorkspacePermission(hub.PermissionFull, true)
	p.workspace = t.TempDir()
	tester.Frame()
	if p.workspacePermissionPolicy() != hub.PermissionAsk {
		t.Fatal("Full confirmation followed a different workspace")
	}
}

func TestWorkspaceSavedPermissionWaitsForActualStore(t *testing.T) {
	service, requests := fixture(t)
	session, err := service.Create(t.Context(), "saved policy", "")
	if err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(service)
	p.selectSession(session.ID)
	tester := ui.NewTester(p.Build, 1240, 850)
	waitWorkspace(t, p, tester, func() bool {
		return p.detail.Session.ID == session.ID && p.planLoaded[p.workspacePlanKey()] && !p.chat.Busy
	})
	p.model.Draft = "不发送"
	p.setWorkspacePermission(hub.PermissionAuto, false)
	waitWorkspace(t, p, tester, func() bool { return !p.busy })
	stored, err := service.Store.Settings(t.Context(), session.ID)
	if err != nil || stored.PermissionPolicy != hub.PermissionAuto || p.workspacePermissionPolicy() != hub.PermissionAuto || p.model.Draft != "不发送" || len(requests) != 0 {
		t.Fatal("UI did not reflect a real cold policy commit", err)
	}
	if _, err = service.Store.BeginTurn(t.Context(), session.ID, "freeze"); err != nil {
		t.Fatal(err)
	}
	// A stale idle view may attempt the transition; the shared owner still
	// rejects it and the UI keeps its previous committed policy and draft.
	p.setWorkspacePermission(hub.PermissionFull, true)
	waitWorkspace(t, p, tester, func() bool { return !p.busy })
	if p.workspacePermissionPolicy() != hub.PermissionAuto || p.problem == "" || p.model.Draft != "不发送" {
		t.Fatal("failed store transition changed the visible policy")
	}
}
