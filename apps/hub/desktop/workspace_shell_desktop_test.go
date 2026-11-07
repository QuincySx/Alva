// INPUT: Native MyGo headless frames and durable Hub service fixtures with no engine adapter.
// OUTPUT: Workspace geometry, draft-only suggestions and real advanced-menu navigation regressions.
// POS: [L7 Hub UI] Verifies native controls without launching a window, model or external process.
//go:build desktop

package hubdesktop

import (
	"context"
	"fmt"
	"math"
	"path/filepath"
	"slices"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/egoist/mygo/ui"
)

func workspaceShellTester(page *NativePage, width, height int) *ui.Tester {
	return ui.NewTester(workspaceStaticView(page), width, height)
}

func TestWorkspaceHeaderShowsOnlyRightPanelOnHome(t *testing.T) {
	service, requests := fixture(t)
	p := NewNativePage(service)
	tester := workspaceShellTester(p, 1440, 960)
	header, _ := tester.Find("Workspace header")
	panel, panelOK := tester.Find("Open resource workspace")
	more, moreOK := tester.Find("More actions")
	title, titleOK := tester.Find("Workspace title 新任务")
	if !panelOK || moreOK || !titleOK || panel.W != 28 || panel.H != 28 ||
		panel.X+panel.W != header.X+header.W-8 || panel.Y != header.Y+4 || title.X != header.X+32 {
		t.Fatal("closed right pane must leave only its own 28px opener", header, panel, more, title)
	}
	if _, ok := tester.Find("Inspector"); ok {
		t.Fatal("Inspector is still duplicated outside More")
	}
	if !tester.HasText("本地工作台") || tester.HasText("Smallraw") {
		t.Fatal("Footer substituted a sample or host identity for product mode")
	}
	if len(requests) != 0 || service.Runtime.HasActive() {
		t.Fatal("building the formal header executed a model")
	}
}

func TestWorkspaceWelcomeIsColdAndSuggestionsEditDraft(t *testing.T) {
	service, requests := fixture(t)
	page := NewNativePage(service)
	tester := workspaceShellTester(page, 1240, 850)
	_, brandOK := tester.Find("Welcome brand")
	if !tester.HasText("今天，想完成什么？") || !brandOK {
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

func TestWorkspaceWelcomeUsesCanonicalHeadingGhostAndInsets(t *testing.T) {
	for _, dark := range []bool{false, true} {
		for _, width := range []int{960, 1240, 1440} {
			t.Run(fmt.Sprintf("dark=%v/width=%d", dark, width), func(t *testing.T) {
				service, _ := fixture(t)
				page := NewNativePage(service)
				tester := workspaceShellTester(page, width, 960)
				tester.SetDark(dark)
				heading, headingOK := tester.Find("今天，想完成什么？")
				composer, composerOK := tester.Find("Task composer")
				message, messageOK := tester.Find("Message")
				first, firstOK := tester.Find("理解这个项目")
				second, secondOK := tester.Find("整理资料")
				add, addOK := tester.Find("Add attachments")
				near := func(a, b float32) bool { return math.Abs(float64(a-b)) < .1 }
				if !headingOK || !composerOK || !messageOK || !firstOK || !secondOK || !addOK || !near(heading.H, 30) || !near(composer.H, 96.8) || !near(first.H, 32) {
					t.Fatal("canonical 24/1.5 heading, 112 editor or 36 Ghost changed", heading, composer, first)
				}
				if !near(composer.Y-heading.Y-heading.H, 16) || !near(first.Y-composer.Y-composer.H, 12) || !near(second.X-first.X-first.W, 8) {
					t.Fatal("canonical content gaps changed", heading, composer, first, second)
				}
				if !near(message.X-composer.X, 8) || !near(message.Y-composer.Y, 8) || !near(composer.Y+composer.H-add.Y-add.H, 8) {
					t.Fatal("inside border consumed the symmetric 8px content inset", composer, message, add)
				}
				image := tester.Image()
				main, _ := tester.Find("Workspace main surface")
				background := image.RGBAAt(int(main.X+main.W-8), int(main.Y+60))
				if fill := image.RGBAAt(int(first.X+4), int(first.Y+first.H/2)); fill != background {
					t.Fatal("Ghost suggestion must show the surrounding reading surface", fill, background)
				}
				search, searchOK := tester.Find("Search saved sessions")
				partners, partnersOK := tester.Find("Companions and groups")
				if !searchOK || !partnersOK || partners.Y <= search.Y || !tester.HasText("伙伴与群聊") {
					t.Fatal("sidebar order or real destination label differs from the design", search, partners)
				}
			})
		}
	}
}

func TestWorkspacePrivateLocationKeepsInternalIdentityOutOfHeader(t *testing.T) {
	service, _ := fixture(t)
	page := NewNativePage(service)
	private := filepath.Join(service.DataDirectory, "workspaces", "workspace-private-id")
	page.workspace = private
	if got := page.workspaceLocation(); got != "私有工作区" {
		t.Fatal("private workspace exposed its storage identity", got)
	}
	page.workspace = filepath.Join(service.DataDirectory, "projects", "workspace-user-project")
	if got := page.workspaceLocation(); got != "workspace-user-project" {
		t.Fatal("user-selected project was relabeled as private", got)
	}
	page.selected = "session"
	page.detail.Session = hub.Session{ID: "session", Title: "实际任务", Workspace: private}
	tester := workspaceShellTester(page, 1240, 850)
	if tester.HasText("workspace-private-id") {
		t.Fatal("actual session header exposed its private storage identity")
	}
	if _, ok := tester.Find("Workspace title 实际任务"); !ok {
		t.Fatal("session header lost its single visible title")
	}
}

func TestWorkspaceConversationKeepsCanonicalReadingWidth(t *testing.T) {
	service, _ := fixture(t)
	for _, width := range []int{960, 1240, 1440} {
		page := NewNativePage(service)
		page.selected = "session"
		page.detail.Session = hub.Session{ID: "session", Title: "真实任务"}
		tester := ui.NewTester(workspaceStaticView(page), width, 850)
		composer, ok := tester.Find("Task composer")
		// B3 uses left8 + sidebar220 + gap8, then the main body's 24px gutters.
		available := float32(width) - 244 - 48
		reading := min(float32(760), available)
		left := float32(236 + 24)
		if !ok || math.Abs(float64(composer.W-reading)) > .1 || math.Abs(float64(composer.X-left-(available-reading)/2)) > .1 {
			t.Fatal("conversation composer escaped the 760px reading column", width, composer)
		}
		history, historyOK := tester.Find("Message history")
		if !historyOK || math.Abs(float64(history.W-available)) > .1 || math.Abs(float64(history.X-left)) > .1 || width == 1440 && history.W <= composer.W {
			t.Fatal("reading width must not shrink the transcript scroll viewport", width, history, composer)
		}
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
		if !ok || sidebar.X != 8 || sidebar.Y != 52 || sidebar.W != workspaceSidebarWidth || sidebar.H != float32(size.height-60) {
			t.Fatalf("sidebar geometry = %+v, present=%v", sidebar, ok)
		}
		newTask, ok := tester.Find("New session")
		if !ok || newTask.W != 28 || newTask.H != 28 || newTask.X != 160 || newTask.Y != 12 {
			t.Fatalf("new task should remain the 28px sidebar tool: %+v", newTask)
		}
		settings, ok := tester.Find("General settings")
		if !ok || settings.Y < float32(size.height)-72 || settings.Y+settings.H > float32(size.height) || settings.W != 28 {
			t.Fatalf("footer escaped the viewport: settings=%+v", settings)
		}
		header, headerOK := tester.Find("Workspace header")
		composer, composerOK := tester.Find("Task composer")
		if !headerOK || header.X != 236 || header.Y != 8 || header.H != 36 || !composerOK || composer.H > 128 || composer.W > 760 || composer.X < sidebar.X+sidebar.W+workspaceRegionGap+24 || composer.X+composer.W > float32(size.width)-workspaceWindowInset {
			t.Fatalf("compact welcome escaped its content area: header=%+v composer=%+v", header, composer)
		}
		t.Logf("%dx%d: header=%+v composer=%+v", size.width, size.height, header, composer)
		for _, label := range []string{"Message", "Add attachments", "Permission policy", "Agent and model", "Send", "理解这个项目", "整理资料", "检查代码"} {
			box, found := tester.Find(label)
			if !found || box.X < sidebar.X+sidebar.W+workspaceRegionGap || box.X+box.W > float32(size.width)-workspaceWindowInset || box.Y < header.Y+header.H || box.Y+box.H > float32(size.height)-workspaceWindowInset {
				t.Fatalf("%s escaped the welcome viewport: %+v", label, box)
			}
		}
		if tester.HasText("Product sessions and native execution records") || tester.HasText("No old Rust data") {
			t.Fatal("engineering explanations returned to the product navigation")
		}
	}
}

func TestWorkspaceFormalShellAndWelcomeBrandMatchSavedNativeLayout(t *testing.T) {
	for _, dark := range []bool{false, true} {
		service, _ := fixture(t)
		page := NewNativePage(service)
		tester := workspaceShellTester(page, 1440, 960)
		tester.SetDark(dark)
		main, mainOK := tester.Find("Workspace main surface")
		sidebar, sidebarOK := tester.Find("Session navigation")
		brand, brandOK := tester.Find("Welcome brand")
		heading, _ := tester.Find("今天，想完成什么？")
		composer, _ := tester.Find("Task composer")
		if !mainOK || !sidebarOK || main != (ui.Rect{X: 236, Y: 52, W: 1196, H: 900}) || sidebar != (ui.Rect{X: 8, Y: 52, W: 220, H: 900}) {
			t.Fatal("formal outer padding/gap and fill surface changed", main, sidebar)
		}
		if !brandOK || brand.X != 454 || math.Abs(float64(brand.Y-386.6)) > .1 || brand.W != 32 || brand.H != 32 || heading.X != brand.X || heading.Y-brand.Y-brand.H != 12 || composer.Y-heading.Y-heading.H != 16 {
			t.Fatal("saved Avatar Agent brand or welcome grouping changed", brand, heading, composer)
		}
		pixels := tester.Image()
		canvas := pixels.RGBAAt(0, 0)
		surface := pixels.RGBAAt(600, 100)
		if surface == canvas || pixels.RGBAAt(238, 54) != canvas || pixels.RGBAAt(246, 80) != surface || pixels.RGBAAt(600, 53) != surface {
			t.Fatal("main surface lost its rounded corner or gained an outline", dark, canvas, surface)
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
	if err := workspaceTestOpenMenu(tester); err != nil {
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
