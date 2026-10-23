// INPUT: Native MyGo headless frames and durable Hub service fixtures with no engine adapter.
// OUTPUT: Workspace geometry, draft-only suggestions and real advanced-menu navigation regressions.
// POS: [L7 Hub UI] Verifies native controls without launching a window, model or external process.
//go:build desktop

package hubdesktop

import (
	"context"
	"slices"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/egoist/mygo/ui"
)

func workspaceShellTester(page *NativePage, width, height int) *ui.Tester {
	return ui.NewTester(func(c *ui.Context) {
		page.applyPreferences(c)
		ui.Row(c).Fill().Children(func() {
			page.buildWorkspaceSidebar(c)
			ui.Column(c).Grow(1).MinWidth(0).FillHeight().Children(func() {
				page.buildWorkspaceHeader(c)
				page.buildWorkspaceWelcome(c)
			})
		})
	}, width, height)
}

func TestWorkspaceWelcomeIsColdAndSuggestionsEditDraft(t *testing.T) {
	service, requests := fixture(t)
	page := NewNativePage(service)
	tester := workspaceShellTester(page, 1240, 850)
	if !tester.HasText("今天，想完成什么？") || !tester.HasText("Alva") {
		t.Fatalf("missing formal workspace welcome: %q", tester.Texts())
	}
	if err := tester.Click("理解这个项目"); err != nil {
		t.Fatal(err)
	}
	if page.model.Draft != "请帮我理解这个项目的结构与主要功能。" {
		t.Fatalf("suggestion did not enter an editable draft: %q", page.model.Draft)
	}
	rows, err := service.Store.ListSessions(context.Background(), 128)
	if err != nil || len(rows) != 0 || service.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("welcome/suggestion created a session or executed a model")
	}
}

func TestWorkspaceSidebarKeepsGeometryAndFooter(t *testing.T) {
	service, _ := fixture(t)
	for _, size := range []struct{ width, height int }{{1240, 850}, {1000, 720}} {
		page := NewNativePage(service)
		for i := 0; i < 64; i++ {
			page.sessions = append(page.sessions, hub.Session{ID: string(rune('A' + i)), Title: "需要完整保留的长会话标题与项目上下文"})
		}
		tester := workspaceShellTester(page, size.width, size.height)
		sidebar, ok := tester.Find("Session navigation")
		if !ok || sidebar.W != workspaceSidebarWidth || sidebar.H != float32(size.height) {
			t.Fatalf("sidebar geometry = %+v, present=%v", sidebar, ok)
		}
		newTask, ok := tester.Find("New session")
		if !ok || newTask.W != 196 || newTask.Y < 56 {
			t.Fatalf("new task should fill the sidebar inside symmetric padding: %+v", newTask)
		}
		settings, ok := tester.Find("General settings")
		if !ok || settings.Y < float32(size.height)-72 || settings.Y+settings.H > float32(size.height) || settings.W != 32 {
			t.Fatalf("footer escaped the viewport: settings=%+v", settings)
		}
		header, headerOK := tester.Find("Workspace header")
		composer, composerOK := tester.Find("Task composer")
		if !headerOK || header.H != 52 || !composerOK || composer.H > 128 || composer.W > 760 || composer.X < sidebar.W+24 || composer.X+composer.W > float32(size.width)-24 {
			t.Fatalf("compact welcome escaped its content area: header=%+v composer=%+v", header, composer)
		}
		t.Logf("%dx%d: header=%+v composer=%+v", size.width, size.height, header, composer)
		for _, label := range []string{"Message", "Add attachments", "Permission policy", "Agent and model", "Send", "理解这个项目", "整理资料", "检查代码"} {
			box, found := tester.Find(label)
			if !found || box.X < sidebar.W || box.X+box.W > float32(size.width) || box.Y < header.H || box.Y+box.H > float32(size.height) {
				t.Fatalf("%s escaped the welcome viewport: %+v", label, box)
			}
		}
		if tester.HasText("Product sessions and native execution records") || tester.HasText("No old Rust data") {
			t.Fatal("engineering explanations returned to the product navigation")
		}
	}
}

func TestWorkspaceHeaderMenuUsesSupportedDestinations(t *testing.T) {
	service, requests := fixture(t)
	service.Teams = nil
	service.Automations = nil
	session, err := service.Create(context.Background(), "真实会话", "")
	if err != nil {
		t.Fatal(err)
	}
	page := NewNativePage(service)
	page.selected = session.ID
	page.detail, err = service.Store.Detail(context.Background(), session.ID)
	if err != nil {
		t.Fatal(err)
	}
	tester := workspaceShellTester(page, 1240, 850)
	if !tester.HasText("真实会话") || tester.HasText("会话设置") {
		t.Fatal("header did not retain the actual title or exposed the advanced controls outside their menu")
	}
	if err := tester.Click("More actions"); err != nil {
		t.Fatal(err)
	}
	items := tester.Menu()
	for _, label := range []string{"对话", "会话设置", "等待中的任务", "Inspector", "媒体资料", "项目技能"} {
		if !slices.Contains(items, label) {
			t.Fatalf("missing real destination %q in %q", label, items)
		}
	}
	for _, label := range []string{"协作", "自动化", "伙伴与群聊", "模型与连接"} {
		if slices.Contains(items, label) {
			t.Fatalf("unsupported destination %q pretends to work", label)
		}
	}
	if err := tester.ChooseMenuItem("交接记录"); err != nil {
		t.Fatal(err)
	}
	if page.pane != "handoffs" || service.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("opening the existing handoff pane executed or navigated to another destination")
	}
}
