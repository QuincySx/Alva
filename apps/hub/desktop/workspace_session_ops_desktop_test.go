// INPUT: Real shared bootstrap/SQLite, MyGo headless sidebar, dialogs and transcript
// OUTPUT: Sidebar rename/pin/soft-delete flows, running-session refusal and the stopped-reply Continue action
// POS: [L7 Hub UI testing] Drives the actual native controls; management and Continue reach the product only through its narrow services
//go:build desktop

package hubdesktop

import (
	"context"
	"image/png"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/bootstrap"
	view "github.com/QuincySx/Alva/apps/ui/chat"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/egoist/mygo/ui"
)

func saveSessionScreenshot(t *testing.T, tester *ui.Tester, name string) {
	t.Helper()
	dir := os.Getenv("ALVA_SESSION_SCREENSHOT_DIR")
	if dir == "" {
		return
	}
	if err := os.MkdirAll(dir, 0700); err != nil {
		t.Fatal(err)
	}
	f, err := os.Create(filepath.Join(dir, name))
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	if err := png.Encode(f, tester.Image()); err != nil {
		t.Fatal(err)
	}
}

func openDemoApp(t *testing.T) *bootstrap.Application {
	t.Helper()
	app, err := bootstrap.Open(t.Context(), bootstrap.Config{Demo: true, DataDirectory: filepath.Join(t.TempDir(), "private"), OpenStores: bootstrap.LocalStores})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := app.Close(context.Background()); err != nil {
			t.Error(err)
		}
	})
	return app
}

func TestSidebarSessionActionsRenamePinAndSoftDelete(t *testing.T) {
	app := openDemoApp(t)
	workspace := t.TempDir()
	alpha, err := app.Service.Create(t.Context(), "alpha", workspace)
	if err != nil {
		t.Fatal(err)
	}
	beta, err := app.Service.Create(t.Context(), "beta", workspace)
	if err != nil {
		t.Fatal(err)
	}
	page := NewNativePage(app.Service)
	tester := ui.NewTester(workspaceStaticView(page), 1240, 850)
	page.refreshSessions()
	waitWorkspace(t, page, tester, func() bool { return len(page.sessions) == 2 && !page.sessionLoading })
	saveSessionScreenshot(t, tester, "sidebar-session-actions.png")

	// Rename through the row's menu and dialog; the title survives a re-read.
	if err = tester.Click("Session actions " + alpha.ID); err != nil {
		t.Fatal(err)
	}
	if got := tester.Menu(); len(got) < 3 {
		t.Fatal("row menu lacks its actions", got)
	}
	saveSessionScreenshot(t, tester, "sidebar-session-menu.png")
	if err = tester.ChooseMenuItem("重命名"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !page.sessionOps.renameOpen || !tester.HasText("重命名任务") {
		t.Fatal("rename dialog did not open")
	}
	saveSessionScreenshot(t, tester, "session-rename-dialog.png")
	page.sessionOps.renameText = "   "
	if err = tester.Click("保存"); err != nil {
		t.Fatal(err)
	}
	if page.sessionOps.renameProblem == "" {
		t.Fatal("blank title was accepted by the dialog")
	}
	page.sessionOps.renameText = "新名称"
	if err = tester.Click("保存"); err != nil {
		t.Fatal(err)
	}
	waitWorkspace(t, page, tester, func() bool {
		return !page.sessionOps.renameOpen && !page.sessionOps.working && tester.HasText("新名称")
	})
	got, err := app.Service.Conversations.Detail(t.Context(), alpha.ID)
	if err != nil || got.Session.Title != "新名称" {
		t.Fatal("rename did not persist", got.Session.Title, err)
	}

	// Pin: the session moves to the pinned group.
	if err = tester.Click("Session actions " + beta.ID); err != nil {
		t.Fatal(err)
	}
	if err = tester.ChooseMenuItem("置顶"); err != nil {
		t.Fatal(err)
	}
	waitWorkspace(t, page, tester, func() bool {
		return len(page.sessionOps.pinned) == 1 && !page.sessionOps.working && tester.HasText("置顶")
	})
	if page.sessionOps.pinned[0].ID != beta.ID {
		t.Fatal("wrong session pinned")
	}
	saveSessionScreenshot(t, tester, "sidebar-pinned-group.png")

	// Delete needs a second confirmation and is soft.
	if err = tester.Click("Session actions " + alpha.ID); err != nil {
		t.Fatal(err)
	}
	if err = tester.ChooseMenuItem("删除…"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !page.sessionOps.deleteOpen || !tester.HasText("删除任务？") || !tester.HasText("「新名称」会从列表、搜索和通知中消失，数据仍保留在本机。") {
		t.Fatal("delete confirmation missing")
	}
	saveSessionScreenshot(t, tester, "session-delete-confirm.png")
	if err = tester.Click("取消"); err != nil {
		t.Fatal(err)
	}
	if _, err = app.Service.Conversations.Detail(t.Context(), alpha.ID); err != nil {
		t.Fatal("cancel deleted the session", err)
	}
	if err = tester.Click("Session actions " + alpha.ID); err != nil {
		t.Fatal(err)
	}
	if err = tester.ChooseMenuItem("删除…"); err != nil {
		t.Fatal(err)
	}
	if err = tester.Click("删除"); err != nil {
		t.Fatal(err)
	}
	waitWorkspace(t, page, tester, func() bool { return !page.sessionOps.deleteOpen && !page.sessionOps.working && len(page.sessions) == 1 })
	if tester.HasText("新名称") {
		t.Fatal("deleted session still in the sidebar")
	}
	if raw, err := app.Store.DetailIncludingDeleted(t.Context(), alpha.ID); err != nil || raw.Session.DeletedAt == 0 {
		t.Fatal("delete must keep the data", err)
	}
}

func TestSidebarDeleteRefusesARunningSession(t *testing.T) {
	app := openDemoApp(t)
	session, err := app.Service.Create(t.Context(), "busy", t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	if _, err = app.Store.BeginTurn(t.Context(), session.ID, "still working"); err != nil {
		t.Fatal(err)
	}
	page := NewNativePage(app.Service)
	tester := ui.NewTester(workspaceStaticView(page), 1240, 850)
	page.refreshSessions()
	waitWorkspace(t, page, tester, func() bool { return len(page.sessions) == 1 && !page.sessionLoading })
	page.openSessionDelete(page.sessions[0])
	if page.sessionOps.deleteOpen || !strings.Contains(page.problem, "正在运行") {
		t.Fatal("a running session reached the delete confirmation", page.problem)
	}
	if err = app.Service.Conversations.Delete(t.Context(), session.ID); err == nil {
		t.Fatal("the service deleted a running session")
	}
}

func TestStoppedReplyOffersContinueAndShowsAStatusBlock(t *testing.T) {
	app := openDemoApp(t)
	session, err := app.Service.Create(t.Context(), "stopped", t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	claim, err := app.Store.BeginTurn(t.Context(), session.ID, "写一段很长的内容")
	if err != nil {
		t.Fatal(err)
	}
	if err = app.Store.FinishTurn(t.Context(), claim.Turn.ID, engine.Cancelled); err != nil {
		t.Fatal(err)
	}
	page := NewNativePage(app.Service)
	tester := ui.NewTester(workspaceStaticView(page), 1240, 850)
	page.refreshSessions()
	waitWorkspace(t, page, tester, func() bool { return len(page.sessions) == 1 && !page.sessionLoading })
	page.selectSession(session.ID)
	waitWorkspace(t, page, tester, func() bool {
		return !page.polling && page.detail.Session.ID == session.ID && page.workspaceStoppedReply()
	})
	if _, shown := tester.Find("Continue stopped reply"); !shown {
		t.Fatal("a stopped reply did not offer Continue", tester.Texts())
	}
	saveSessionScreenshot(t, tester, "stopped-reply-continue.png")
	if err = tester.Click("Continue stopped reply"); err != nil {
		t.Fatal(err)
	}
	waitWorkspace(t, page, tester, func() bool {
		requests, err := app.Service.PendingApprovals(t.Context(), session.ID)
		if err != nil {
			t.Fatal(err)
		}
		for _, request := range requests { // explicit test-only consent for the local echo fixture
			if err := app.Service.RespondApproval(t.Context(), session.ID, request.ID, true); err != nil {
				t.Fatal(err)
			}
		}
		return !page.busy && !page.polling && !app.Runtime.HasActive() && page.model.Status == view.Completed && tester.HasText("继续生成")
	})
	var user int
	for _, row := range page.model.Messages {
		if row.Role == "You" && strings.Contains(row.Text, "继续") {
			user++
		}
	}
	if user != 0 {
		t.Fatal("the fixed continue instruction was shown as a user message")
	}
	if _, shown := tester.Find("Continue stopped reply"); shown {
		t.Fatal("Continue is still offered after the reply was continued")
	}
	history, err := app.Service.Conversations.History(t.Context(), session.ID, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	var continued int
	for _, entry := range history {
		if entry.Origin == hub.TurnOriginContinue {
			continued++
		}
	}
	if continued != 1 {
		t.Fatal("exactly one continue Turn expected", continued)
	}
}
