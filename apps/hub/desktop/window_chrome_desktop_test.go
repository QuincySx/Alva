// INPUT: Native title-bar facts, frozen shell dimensions and MyGo headless Hub controls.
// OUTPUT: System-owned control options, aligned safe headers and fully hidden navigation regressions.
// POS: [L7 Hub UI testing] Static controls do not prove OS dragging, resizing or fullscreen behavior.
//go:build desktop

package hubdesktop

import (
	"fmt"
	"strings"
	"testing"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

func TestWorkspaceWindowOptionsKeepNativeControlsAndWindowEdges(t *testing.T) {
	for _, platform := range []string{"darwin", "windows", "linux"} {
		options := workspaceWindowOptions(platform)
		style := mygo.TitleBarHidden
		if platform == "darwin" {
			style = mygo.TitleBarHiddenInset
		}
		if options.TitleBarStyle != style || options.HideWindowButtons || options.Frameless || options.TitleBarHeight != 52 ||
			options.MinWidth != 960 || options.MinHeight != 680 || options.Width != 1240 || options.Height != 850 {
			t.Fatal("window no longer keeps system controls, resize edges or existing minimum size", platform, options)
		}
		if platform == "darwin" {
			if options.TrafficLightPosition == nil || *options.TrafficLightPosition != (mygo.Point{X: 16, Y: 19}) {
				t.Fatal("native traffic lights have no explicit symmetric toolbar position", options.TrafficLightPosition)
			}
		} else if options.TrafficLightPosition != nil {
			t.Fatal("macOS control placement leaked to another platform")
		}
	}
}

func TestWorkspaceNativeTitleBarFactsKeepOneAlignedShell(t *testing.T) {
	for _, bar := range []ui.TitleBar{{}, {Height: 52, Left: 86}, {Height: 52, Right: 138}, {Height: 60, Right: 138}, {Height: 80, Right: 138}} {
		for _, dark := range []bool{false, true} {
			for _, width := range []int{960, 1240, 1440} {
				for _, collapsed := range []bool{false, true} {
					t.Run(fmt.Sprintf("bar=%v/dark=%v/width=%d/collapsed=%v", bar, dark, width, collapsed), func(t *testing.T) {
						service := preferenceFixture(t)
						p := NewNativePage(service)
						p.model.Draft = "native header draft"
						p.changeWorkspaceSidebar(0, collapsed)
						tester := ui.NewTester(workspaceStaticView(p), width, 850)
						tester.SetTitleBar(bar)
						tester.SetDark(dark)
						top, paperY := b3Top(bar), b3PaperY(bar)
						mainX := float32(236)
						if collapsed {
							mainX = workspaceWindowInset
						}
						mainWidth := float32(width) - mainX - workspaceWindowInset
						main, mainOK := tester.Find("Workspace main surface")
						header, headerOK := tester.Find("Workspace header")
						region, regionOK := tester.Find("Workspace header region")
						body, bodyOK := tester.Find("New task welcome")
						panel, panelOK := tester.Find("Open resource workspace")
						more, moreOK := tester.Find("More actions")
						title, titleOK := tester.Find("Workspace title 新任务")
						_, sourceShown := tester.Find("私有工作区 · 项目")
						if !mainOK || main != (ui.Rect{X: mainX, Y: paperY, W: mainWidth, H: 850 - paperY - workspaceWindowInset}) ||
							!headerOK || header.X != mainX || header.Y != top || header.H != workspaceHeaderHeight ||
							!regionOK || region != (ui.Rect{X: mainX, Y: top, W: mainWidth, H: workspaceHeaderHeight + b3RightBandGuard(bar)}) ||
							!bodyOK || body.Y < paperY || !panelOK || moreOK || panel.W != 28 || panel.H != 28 || sourceShown {
							t.Fatal("one shell no longer follows native band and 36px content geometry", main, header, region, body, panel, more)
						}
						if !titleOK || title.Y+title.H/2 != header.Y+18 || panel.Y+panel.H/2 != header.Y+18 ||
							panel.X != b3RightToggleX(float32(width), bar) {
							t.Fatal("title and actions lost their shared center or native right clearance", header, title, panel, more)
						}
						if bar.Right > 0 && (body.Y < bar.Height || panel.X+panel.W > float32(width)-bar.Right-8) {
							t.Fatal("body or header action overlaps the full native right control band", body, more, bar)
						}
						sidebar, sidebarOK := tester.Find("Session navigation")
						_, splitterOK := tester.Find("Resize session navigation")
						if collapsed {
							expand, expandOK := tester.Find("Expand session navigation")
							if sidebarOK || splitterOK || !expandOK || expand.X != mainX+max(float32(24), bar.Left+8-mainX) || expand.Y+expand.H/2 != header.Y+18 {
								t.Fatal("collapsed navigation left a rail, splitter or unsafe toggle", sidebar, expand)
							}
							if err := tester.Click("Expand session navigation"); err != nil {
								t.Fatal(err)
							}
						} else {
							row, rowOK := tester.Find("Sidebar tools")
							toggle, toggleOK := tester.Find("Collapse session navigation")
							if !sidebarOK || !splitterOK || sidebar != (ui.Rect{X: 8, Y: paperY, W: 220, H: 850 - paperY - 8}) ||
								!rowOK || row.Y != header.Y || row.H != 36 ||
								!toggleOK || toggle.X != sidebar.X+sidebar.W-workspaceWindowInset-workspaceIconControlSize || toggle.Y+toggle.H/2 != header.Y+18 || bar.Left > 0 && toggle.X < bar.Left+8 {
								t.Fatal("sidebar row does not align or avoid the native left controls", sidebar, row, toggle)
							}
							if err := tester.Click("Collapse session navigation"); err != nil {
								t.Fatal(err)
							}
							if _, ok := tester.Find("Session navigation"); ok {
								t.Fatal("collapse left navigation in layout and focus")
							}
							if moved, ok := tester.Find("Workspace main surface"); !ok || moved.X != workspaceWindowInset || moved.Y != paperY {
								t.Fatal("collapsed navigation did not move the main paper to the left shell inset", moved)
							}
						}
						if p.model.Draft != "native header draft" || service.Runtime.HasActive() {
							t.Fatal("native header toggle changed draft or execution")
						}
						for _, fake := range []string{"Window title bar", "Close window", "Minimize window", "Zoom window", "Sidebar brand"} {
							if _, ok := tester.Find(fake); ok {
								t.Fatal("self-drawn system chrome or brand strip remains", fake)
							}
						}
					})
				}
			}
		}
	}
}

func TestWorkspaceNativeSettingsReturnAvoidsControlsAndPreservesDraft(t *testing.T) {
	for _, bar := range []ui.TitleBar{{}, {Height: 52, Left: 86}, {Height: 52, Right: 138}} {
		t.Run(fmt.Sprint(bar), func(t *testing.T) {
			service := preferenceFixture(t)
			p := NewNativePage(service)
			p.model.Draft = "return to the same draft"
			p.changeWorkspaceSidebar(348, true)
			tester := ui.NewTester(workspaceStaticView(p), 960, 680)
			tester.SetTitleBar(bar)
			if err := workspaceTestMenu(tester, "设置"); err != nil {
				t.Fatal(err)
			}
			settlePreferences(t, p, tester)
			header, ok := tester.Find("Settings header")
			nav, navOK := tester.Find("Settings navigation header")
			title, titleOK := tester.Find("设置")
			back, backOK := tester.Find("Back to workspace")
			body, bodyOK := tester.Find("Settings body")
			if !ok || !navOK || !titleOK || !backOK || !bodyOK || header.H != 36 || nav.Y != header.Y || nav.H != 36 ||
				title.Y+title.H/2 != header.Y+18 || title.X != workspaceWindowInset+b3LeftSafe(bar, workspaceWindowInset) ||
				back.X != header.X+workspaceWindowInset || back.Y != header.Y+2 || back.H != 32 ||
				body.X != header.X || body.Y != b3PaperY(bar) || bar.Right > 0 && body.Y < bar.Height {
				t.Fatal("settings return lost the shared header or native safe band", header, nav, title, back, body)
			}
			if err := tester.Click("Back to workspace"); err != nil {
				t.Fatal(err)
			}
			if p.pane != "" || p.model.Draft != "return to the same draft" || p.layout.sidebarWidth != 348 || !p.layout.sidebarCollapsed || service.Runtime.HasActive() {
				t.Fatal("settings return changed layout, draft or owner", p.pane, p.layout, p.model.Draft)
			}
			if _, ok := tester.Find("Session navigation"); ok {
				t.Fatal("settings return restored an unwanted rail")
			}
			if err := workspaceTestMenu(tester, "新任务"); err != nil {
				t.Fatal(err)
			}
			if p.selected != "" || service.Runtime.HasActive() {
				t.Fatal("More new task created or executed a session")
			}
		})
	}
}

func TestWorkspaceNativeConnectionSheetUsesOneSafeReturn(t *testing.T) {
	for _, bar := range []ui.TitleBar{{}, {Height: 52, Left: 86}, {Height: 52, Right: 138}} {
		for _, form := range []bool{false, true} {
			t.Run(fmt.Sprintf("bar=%v/form=%v", bar, form), func(t *testing.T) {
				service, store := connectionsFixture(t)
				p := NewNativePage(service)
				p.model.Draft = "workspace draft beside sheet"
				p.openConnections()
				tester := ui.NewTester(workspaceStaticView(p), 960, 680)
				tester.SetTitleBar(bar)
				settleConnections(t, p, tester)
				p.connections.mode = "sources"
				label, dialogLabel := "返回模型与连接", "Connection source dialog"
				if form {
					p.chooseConnectionSource("OpenAI")
					p.connections.draft.Name = "preserved source draft"
					label, dialogLabel = "Back from connection form", "Connection form dialog"
				}
				tester.Frame()
				header, headerOK := tester.Find("Connection sheet header")
				region, regionOK := tester.Find("Connection sheet header region")
				body, bodyOK := tester.Find("Connection sheet body")
				back, backOK := tester.Find(label)
				dialog, dialogOK := tester.Find(dialogLabel)
				if !headerOK || !regionOK || !bodyOK || !backOK || !dialogOK || header.H != 36 || back.H != 32 ||
					back.Y != header.Y+2 || back.X != workspaceWindowInset+b3LeftSafe(bar, workspaceWindowInset) || body.Y != region.Y+region.H ||
					dialog.Y < body.Y+24 || dialog.Y+dialog.H > body.Y+body.H-24 || back.Y+back.H > dialog.Y || bar.Right > 0 && body.Y < bar.Height {
					t.Fatal("connection return is duplicated in the card or overlaps native controls", header, region, body, back, dialog)
				}
				p.connections.saving = true
				tester.Frame()
				_ = tester.Click(label)
				mode := "sources"
				if form {
					mode = "form"
				}
				if p.connections.mode != mode {
					t.Fatal("sheet return bypassed the original save guard")
				}
				p.connections.saving = false
				tester.Frame()
				if err := tester.Click(label); err != nil {
					t.Fatal(err)
				}
				if form && (p.connections.mode != "sources" || p.connections.draft.Name != "preserved source draft") ||
					!form && p.connections.mode != "" || p.model.Draft != "workspace draft beside sheet" || store.writes.Load() != 0 || service.Runtime.HasActive() {
					t.Fatal("moving the existing return changed draft, persistence or execution", p.connections.mode)
				}
			})
		}
	}
}

func TestWorkspaceNativeHeaderTruncatesLongIdentityBeforeActions(t *testing.T) {
	for _, collapsed := range []bool{false, true} {
		p := NewNativePage(preferenceFixture(t))
		p.changeWorkspaceSidebar(0, collapsed)
		p.selected = "long-header-session"
		p.detail.Session.ID = p.selected
		p.detail.Session.Title = strings.Repeat("需要完整保留的任务标题", 30)
		location := strings.Repeat("long-project-", 50)
		p.detail.Session.Workspace = "/tmp/" + location
		tester := ui.NewTester(workspaceStaticView(p), 960, 680)
		tester.SetTitleBar(ui.TitleBar{Height: 52, Left: 86})
		header, _ := tester.Find("Workspace header")
		title, titleOK := tester.Find("Workspace title " + p.detail.Session.Title)
		_, sourceShown := tester.Find(location + " · 当前会话")
		panel, panelOK := tester.Find("Open resource workspace")
		more, moreOK := tester.Find("More actions")
		if !titleOK || sourceShown || !panelOK || moreOK || header.H != 36 || title.H != 19.5 ||
			title.W <= 0 || title.X+title.W > panel.X-4+.1 ||
			panel.W != 28 || panel.X != b3RightToggleX(960, ui.TitleBar{Height: 52, Left: 86}) {
			t.Fatal("long title displaced actions or expanded the single-line header", collapsed, header, title, panel, more)
		}
	}
}
