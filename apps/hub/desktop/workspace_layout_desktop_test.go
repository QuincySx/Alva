// INPUT: Real Hub fixtures, Foundation layout defaults and MyGo headless pointer/keyboard controls.
// OUTPUT: Sidebar collapse/resize, shared split limits and narrow resource return regressions.
// POS: [L7 Hub UI tests] Checks actual control geometry and preserved drafts without OS windows or model calls.
//go:build desktop

package hubdesktop

import (
	"fmt"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/egoist/mygo/ui"
)

func TestWorkspaceSidebarPointerKeyboardLimitsAndRestore(t *testing.T) {
	service := preferenceFixture(t)
	p := NewNativePage(service)
	p.model.Draft = "保留当前写作内容"
	tester := ui.NewTester(workspaceStaticView(p), 1600, 850)
	edge, found := tester.Find("Resize session navigation")
	if !found {
		t.Fatal("expanded navigation has no accessible splitter")
	}
	tester.Press(edge.X+edge.W/2, edge.Y+320)
	tester.Move(edge.X+edge.W/2+40, edge.Y+320)
	tester.Move(edge.X+edge.W/2+80, edge.Y+320)
	tester.Release(edge.X+edge.W/2+80, edge.Y+320)
	if p.layout.sidebarWidth != 300 {
		t.Fatal("moving the splitter changed its pointer origin", p.layout.sidebarWidth)
	}
	tester.Key(0, ui.KeyRight)
	if p.layout.sidebarWidth != 316 {
		t.Fatal("focused navigation splitter ignored the keyboard", p.layout.sidebarWidth)
	}
	tester.Key(0, ui.KeyEnd)
	if p.layout.sidebarWidth != hub.MaxSidebarWidth {
		t.Fatal("End did not reach the allowed maximum", p.layout.sidebarWidth)
	}
	edge, _ = tester.Find("Resize session navigation")
	tester.Press(edge.X+edge.W/2, edge.Y+320)
	tester.Move(0, edge.Y+320)
	tester.Release(0, edge.Y+320)
	if p.layout.sidebarWidth != hub.MinSidebarWidth {
		t.Fatal("pointer resize escaped the minimum", p.layout.sidebarWidth)
	}
	tester.Key(0, ui.KeyEnd)
	preferred, version := p.layout.sidebarWidth, p.layout.changed
	tester.SetSize(740, 850)
	sidebar, _ := tester.Find("Session navigation")
	composer, _ := tester.Find("Task composer")
	if sidebar.W != 236 || p.layout.sidebarWidth != preferred || p.layout.changed != version || composer.W < workspaceConversationMinWidth {
		t.Fatal("viewport clamp lost the user's width or usable composer", sidebar, composer, p.layout)
	}
	tester.SetSize(1600, 850)
	sidebar, _ = tester.Find("Session navigation")
	if sidebar.W != preferred {
		t.Fatal("widening did not restore the requested width", sidebar)
	}
	connectionClick(t, tester, "Collapse session navigation")
	sidebar, sidebarFound := tester.Find("Session navigation")
	main, mainFound := tester.Find("Workspace main surface")
	if sidebarFound || !mainFound || main.X != workspaceWindowInset || !p.layout.sidebarCollapsed || p.layout.sidebarWidth != preferred {
		t.Fatal("collapse left navigation or lost the expanded width", sidebar, main, p.layout)
	}
	for _, label := range []string{"New session", "Search / refresh sessions", "Companions and groups", "Automations", "General settings", "Resize session navigation"} {
		if _, ok := tester.Find(label); ok {
			t.Fatal("collapsed navigation retained a layout or focus target", label)
		}
	}
	expand, expandFound := tester.Find("Expand session navigation")
	if !expandFound || expand.W != 28 || expand.H != 28 || expand.X != workspaceWindowInset+3*workspaceRegionGap {
		t.Fatal("header lost the sidebar return", expand)
	}
	connectionClick(t, tester, "Expand session navigation")
	sidebar, _ = tester.Find("Session navigation")
	if sidebar.W != preferred || p.layout.sidebarCollapsed || p.model.Draft != "保留当前写作内容" || service.Runtime.HasActive() {
		t.Fatal("expanding changed the draft or execution", sidebar, p.layout)
	}
	if err := tester.Click("Resize session navigation"); err != nil {
		t.Fatal(err)
	}
	tester.Key(0, ui.KeyHome)
	if p.layout.sidebarWidth != 0 {
		t.Fatal("Home did not restore the Foundation default")
	}
	opened := 0
	p.openWindow = func(string) error { opened++; return nil }
	if err := workspaceTestMenu(tester, "新窗口"); err != nil || opened != 1 {
		t.Fatal("new window is no longer reachable from More", opened, err)
	}
	openPreferencePage(t, p, tester)
	connectionClick(t, tester, "Developer options")
	if p.pane != "developer" || service.Runtime.HasActive() {
		t.Fatal("developer destination was lost when removed from the Footer")
	}
	connectionClick(t, tester, "Back to workspace")
	if p.pane != "" || p.model.Draft != "保留当前写作内容" {
		t.Fatal("relocated navigation changed its workspace origin or draft")
	}
}

func TestWorkspaceLayoutSplitUsesVisibleNavigationAndKeepsResourceReturn(t *testing.T) {
	for _, item := range []struct {
		width     int
		sidebar   float32
		collapsed bool
		focused   bool
	}{
		{960, 0, false, true},
		{1000, 0, false, true},
		{1000, 0, true, false},
		{1240, 0, false, false},
		{1240, 432, false, true},
		{1284, 432, false, false},
		{1600, 432, false, false},
	} {
		t.Run(fmt.Sprintf("width=%d/sidebar=%g/collapsed=%v", item.width, item.sidebar, item.collapsed), func(t *testing.T) {
			p, session := resourcePageFixture(t)
			p.acceptWorkspaceNavigationDetail(session.ID, p.generation) // Fixture performed the matching Store.Detail read.
			p.changeWorkspaceSidebar(item.sidebar, item.collapsed)
			p.model.Draft = "打开资料时仍保留的草稿"
			tester := ui.NewTester(workspaceStaticView(p), item.width, 850)
			settleResource(t, p, tester, func() bool { return p.workspaceResourceSession().loaded })
			p.openWorkspaceResource(hub.WorkspaceTab{Kind: "files", Title: "项目资料", BranchID: session.ActiveBranchID})
			settleResource(t, p, tester, func() bool { return !p.resources.loading })
			s := p.workspaceResourceSession()
			s.view.Width = 900
			tester.Frame()
			panel, panelFound := tester.Find("Resource workspace")
			sidebar, _ := tester.Find("Session navigation")
			leftEdge := float32(workspaceWindowInset)
			if sidebar.W > 0 {
				leftEdge = sidebar.X + sidebar.W + workspaceRegionGap
			}
			if !panelFound || panel.X < leftEdge || panel.X+panel.W > float32(item.width)-workspaceWindowInset || panel.Y+panel.H > 850-workspaceWindowInset || s.view.Width != 900 {
				t.Fatal("resource display lost its bounds or stored choice", panel, sidebar, s.view.Width)
			}
			composer, composerFound := tester.Find("Task composer")
			if item.focused {
				if composerFound || !p.workspaceResourcesFullWidth(float32(item.width)) {
					t.Fatal("narrow pane did not enter its explicit focused view")
				}
			} else {
				if !composerFound || composer.W < workspaceConversationMinWidth || composer.X+composer.W+24 > panel.X+.5 {
					t.Fatal("split resource width squeezed the composer", composer, panel)
				}
				for _, label := range []string{"Message", "Add attachments", "Permission policy", "Agent and model", "Send"} {
					r, found := tester.Find(label)
					if !found || r.W <= 0 || r.X < composer.X || r.X+r.W > composer.X+composer.W+.5 || r.Y < composer.Y || r.Y+r.H > composer.Y+composer.H+.5 {
						t.Fatal("composer action escaped the usable split width", label, composer, r)
					}
				}
			}
			back, backFound := tester.Find("Collapse resource workspace")
			if !backFound || back.X+back.W != float32(item.width)-workspaceWindowInset-b3RightSafe(ui.TitleBar{}) || back.Y+back.H/2 != b3Top(ui.TitleBar{})+workspaceHeaderHeight/2 {
				t.Fatal("focused resource view lost its constant return action", panel, back)
			}
			connectionClick(t, tester, "Collapse resource workspace")
			composer, composerFound = tester.Find("Task composer")
			if !composerFound || !s.view.Collapsed || p.model.Draft != "打开资料时仍保留的草稿" || p.selected != session.ID || p.Service.Runtime.HasActive() {
				t.Fatal("return to chat changed draft, session or execution", composer, p.selected)
			}
			connectionClick(t, tester, "Open resource workspace")
			if s.view.Collapsed || !p.workspaceResourcesVisible() {
				t.Fatal("page header could not restore collapsed resources")
			}
		})
	}
}

func TestWorkspaceSidebarSettingsReturnRetainsLayoutAndDraft(t *testing.T) {
	service := preferenceFixture(t)
	p := NewNativePage(service)
	p.model.Draft = "设置前的手稿"
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	p.changeWorkspaceSidebar(348, true)
	tester.Frame()
	if err := workspaceTestMenu(tester, "设置"); err != nil {
		t.Fatal(err)
	}
	settlePreferences(t, p, tester)
	connectionClick(t, tester, "Back to workspace")
	sidebar, found := tester.Find("Session navigation")
	if found || p.layout.sidebarWidth != 348 || !p.layout.sidebarCollapsed || p.model.Draft != "设置前的手稿" || service.Runtime.HasActive() {
		t.Fatal("Settings return reset the current layout or draft", sidebar, p.layout)
	}
}

func TestWorkspaceSidebarLayoutAndNavigationKeepActualExecutionOwner(t *testing.T) {
	service := preferenceFixture(t)
	owner := &settingsRunningEngine{entered: make(chan struct{}), release: make(chan struct{})}
	if err := service.Runtime.Register("alva", owner); err != nil {
		t.Fatal(err)
	}
	first, err := service.Create(t.Context(), "正在执行的会话", "")
	if err != nil {
		t.Fatal(err)
	}
	other, err := service.Create(t.Context(), "另一个会话", "")
	if err != nil {
		t.Fatal(err)
	}
	_, done, err := service.Runtime.Start(t.Context(), first.ID, "explicit local layout fixture")
	if err != nil {
		t.Fatal(err)
	}
	defer func() { close(owner.release); <-done }()
	select {
	case <-owner.entered:
	case <-time.After(time.Second):
		t.Fatal("local owner did not start")
	}
	identity, active := service.Runtime.ActiveExecution(first.ID)
	if !active {
		t.Fatal("started owner has no execution identity")
	}
	p := NewNativePage(service)
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	p.selectSession(first.ID)
	waitWorkspace(t, p, tester, func() bool { return p.detail.Session.ID == first.ID && !p.chat.Busy })
	p.model.Draft = "执行中会话的下一条草稿"
	connectionClick(t, tester, "Collapse session navigation")
	if err := workspaceTestMenu(tester, "设置"); err != nil {
		t.Fatal(err)
	}
	settlePreferences(t, p, tester)
	connectionClick(t, tester, "Back to workspace")
	p.selectSession(other.ID)
	waitWorkspace(t, p, tester, func() bool { return p.detail.Session.ID == other.ID && !p.chat.Busy })
	p.model.Draft = "另一个会话的草稿"
	p.selectSession(first.ID)
	waitWorkspace(t, p, tester, func() bool { return p.detail.Session.ID == first.ID && !p.chat.Busy })
	connectionClick(t, tester, "Expand session navigation")
	if err := tester.Click("Resize session navigation"); err != nil {
		t.Fatal(err)
	}
	tester.Key(0, ui.KeyRight)
	current, active := service.Runtime.ActiveExecution(first.ID)
	if !active || current != identity || owner.starts.Load() != 1 || owner.cancels.Load() != 0 || p.model.Draft != "执行中会话的下一条草稿" || p.layout.sidebarWidth != 236 {
		t.Fatal("layout/settings/session navigation changed the execution owner or draft", current, p.model.Draft, p.layout)
	}
}
