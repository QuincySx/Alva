// INPUT: Current Hub page projections, formal SmallPen workspace regions and native MyGo controls.
// OUTPUT: Collapsible/resizable task navigation, session header, centered welcome and explicit advanced action menus.
// POS: [L7 Hub UI] Presentation only; opening a region never creates sessions or starts execution.
//go:build desktop

package hubdesktop

import (
	"path/filepath"
	"strings"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/egoist/mygo/ui"
)

const (
	workspaceSidebarWidth float32 = hub.DefaultSidebarWidth
	workspaceReadingWidth float32 = 760
)

func workspaceAction(c *ui.Context, key, label, icon string, selected bool) ui.Element {
	t := c.Theme()
	button := ui.ButtonBase(c.Key(key)).Label(label).FillWidth().MinWidth(0).
		Padding(4, 8).Gap(8).AlignItems(ui.Center).Radius(8).TextColor(t.Text).Selected(selected)
	switch {
	case selected:
		button.Background(t.Surface)
	case button.Pressed():
		button.Background(t.SurfacePressed)
	case button.Hovered():
		button.Background(t.SurfaceHover)
	}
	button.Children(func() {
		color := t.TextMuted
		if selected {
			color = t.Text
		}
		ui.Icon(c, workspaceIcon(icon)).Size(workspaceIconSize, workspaceIconSize).TextColor(color).Shrink(0)
		ui.Text(c, label).TextColor(t.Text).FontSize(workspaceFontBody).LineHeight(1.5).Grow(1).MinWidth(0).SingleLine()
	})
	return button
}

func workspaceIconAction(c *ui.Context, key, label, icon string) ui.Element {
	t := c.Theme()
	button := ui.ButtonBase(c.Key(key)).Label(label).Tooltip(label).Size(workspaceIconControlSize, workspaceIconControlSize).
		Padding(6).Radius(8).AlignItems(ui.Center).Justify(ui.Center).TextColor(t.TextMuted)
	if button.Hovered() {
		button.Background(t.SurfaceHover)
	}
	button.Children(func() { ui.Icon(c, workspaceIcon(icon)).Size(workspaceIconSize, workspaceIconSize) })
	return button
}

func (p *NativePage) buildWorkspaceSidebar(c *ui.Context) {
	if p.layout.sidebarCollapsed {
		return
	}
	viewport, _ := c.Size()
	width := p.workspaceSidebarDisplayedWidth(viewport)
	ui.Column(c.Key("workspace-sidebar")).Label("Session sidebar").Width(width).Shrink(0).
		FillHeight().Gap(workspaceRegionGap).
		Transition(ui.ElementTransition{Duration: workspacePaneMotionDuration, Position: true, Size: true,
			Enter: &ui.Motion{X: -width, Opacity: 1},
			Exit:  &ui.Motion{X: -width, Opacity: 0}}).Children(func() {
		workspaceColumnToolbar(c, "sidebar-tools", "Sidebar tools", workspaceLeftSafeInset(c, workspaceWindowInset), workspaceWindowInset, func() {
			ui.Spacer(c)
			workspaceIconAction(c, "workspace-search", "Search saved sessions", "search").Tooltip("搜索任务").
				OnClick(p.openWorkspaceSearch)
			workspaceIconAction(c, "workspace-new-task", "新任务", "plus").Label("New session").Tooltip("新任务").
				Disabled(p.busy).OnClick(p.startWorkspaceTask)
			ui.Box(c).Size(workspaceIconControlSize, workspaceIconControlSize).Shrink(0) // The shell owns the moving toggle above both panes.
		})
		workspaceAuxiliarySurface(c, "session-navigation", "Session navigation").FillWidth().Children(func() {
			ui.Column(c.Key("workspace-navigation")).Label("Navigation").Fill().Padding(8).Gap(2).Children(func() {
				workspaceAction(c, "workspace-companions", "伙伴与群聊", "users-round", p.pane == "companions").Height(workspaceNavigationRowHeight).Padding(0, 8).
					Label("Companions and groups").Disabled(p.busy || p.Service.Companions == nil).OnClick(p.openWorkspaceCompanions)
				workspaceAction(c, "workspace-automations", "自动化", "calendar", p.pane == "automation-index" || p.pane == "automations").
					Height(workspaceNavigationRowHeight).Padding(0, 8).Label("Automations").Disabled(p.busy || p.Service.Automations == nil).OnClick(p.openWorkspaceAutomations)
				p.buildWorkspaceNavigation(c)
				ui.Row(c).Gap(8).Padding(4).AlignItems(ui.Center).Children(func() {
					p.buildWorkspaceLocalIdentity(c)
					workspaceIconAction(c, "workspace-preferences", "General settings", "settings").
						Tooltip("设置").Disabled(p.Service.Preferences == nil).OnClick(p.openPreferences)
				})
			})
		})
		p.buildWorkspaceSidebarResize(c, viewport)
	})
}

// The product has no declared human profile source. A local-mode label is a
// product mode, never an inferred OS user, CLI account or design sample name.
func (p *NativePage) buildWorkspaceLocalIdentity(c *ui.Context) {
	t := c.Theme()
	ui.Box(c).Label("Local mode avatar").Size(28, 28).Shrink(0).Radius(14).
		Background(workspacePalette(c).selection).AlignItems(ui.Center).Justify(ui.Center).Children(func() {
		ui.Icon(c, workspaceIcon("circle-user-round")).Size(workspaceIconSize, workspaceIconSize).TextColor(t.Accent)
	})
	ui.Text(c, "本地工作台").FontSize(workspaceFontBody).LineHeight(1.5).Grow(1).MinWidth(0).SingleLine()
}

func (p *NativePage) buildWorkspaceSession(c *ui.Context, session hub.Session) {
	p.buildWorkspaceNavigationSession(c, session, false)
}

func (p *NativePage) buildWorkspaceNavigationSession(c *ui.Context, session hub.Session, recent bool) {
	t := c.Theme()
	selected := p.selected == session.ID
	button := ui.ButtonBase(c.Key(session.ID)).Label(p.sessionLabel(session)).Tooltip(p.sessionLabel(session)).
		FillWidth().MinWidth(0).MinHeight(workspaceNavigationRowHeight).Padding(4, 8).Gap(8).AlignItems(ui.Center).Radius(8).
		Selected(selected).Disabled(p.busy)
	if selected {
		button.Background(t.Surface)
	} else if button.Hovered() {
		button.Background(t.SurfaceHover)
	}
	button.OnClick(func() { p.selectSession(session.ID) }).Children(func() {
		color := t.TextMuted
		if selected {
			color = t.Text
		}
		status := engine.Outcome(strings.TrimPrefix(p.sessionLabel(session), session.Title+" · "))
		icon := "message-square"
		if recent {
			icon = "clock"
		}
		ui.Icon(c, workspaceIcon(icon)).Size(workspaceIconSize, workspaceIconSize).TextColor(color).Shrink(0)
		labelColor := t.Text
		if recent && !selected {
			labelColor = t.TextMuted
		}
		ui.Text(c, session.Title).TextColor(labelColor).FontSize(workspaceFontBody).LineHeight(1.5).SingleLine().Grow(1).MinWidth(0)
		switch status {
		case hub.Running:
			ui.Icon(c, workspaceIcon("loader-circle")).Size(12, 12).TextColor(t.Accent)
		case engine.Waiting:
			ui.Box(c).Size(6, 6).Radius(3).Background(t.Warning)
		case engine.Failed, engine.Interrupted:
			ui.Icon(c, workspaceIcon("triangle-alert")).Size(12, 12).TextColor(t.Danger)
		}
	})
}

func (p *NativePage) workspaceTitle() string {
	switch p.pane {
	case "automation-index":
		return "自动化"
	case "companions":
		return "伙伴与群聊"
	case "workspace-scope":
		return "选择任务"
	}
	if p.selected == "" {
		return "新任务"
	}
	if p.detail.Session.ID == p.selected && p.detail.Session.Title != "" {
		return p.detail.Session.Title
	}
	for _, session := range p.sessions {
		if session.ID == p.selected {
			return session.Title
		}
	}
	return "会话"
}

func (p *NativePage) workspaceLocation() string {
	path := p.workspace
	if p.selected != "" && p.detail.Session.ID == p.selected {
		path = p.detail.Session.Workspace
	}
	if path == "" {
		return "私有工作区"
	}
	if p.Service.DataDirectory != "" && filepath.Dir(filepath.Clean(path)) == filepath.Join(p.Service.DataDirectory, "workspaces") {
		return "私有工作区"
	}
	return filepath.Base(filepath.Clean(path))
}

func (p *NativePage) buildWorkspaceHeader(c *ui.Context) {
	t := c.Theme()
	viewport, _ := c.Size()
	windowX := workspaceWindowInset
	if !p.layout.sidebarCollapsed {
		windowX += p.workspaceSidebarDisplayedWidth(viewport) + workspaceRegionGap
	}
	left := workspaceLeftSafeInset(c, windowX)
	if p.layout.sidebarCollapsed {
		toggle := max(workspaceWindowInset+3*workspaceRegionGap, c.TitleBar().Left+workspaceWindowInset)
		left = max(left, toggle-windowX)
	}
	right := workspaceRightSafeInset(c)
	reserveRight := !p.workspaceResourcesVisible()
	workspaceColumnToolbar(c, "workspace-header", "Workspace header", left, right, func() {
		if p.layout.sidebarCollapsed {
			ui.Box(c).Size(workspaceIconControlSize, workspaceIconControlSize).Shrink(0) // Reserve room before the title for the shell toggle.
		}
		if p.pane == "workspace-scope" {
			workspaceIconAction(c, "scope-cancel", "Cancel scope selection", "chevron-left").Tooltip("返回").Disabled(p.busy).OnClick(p.cancelWorkspaceScopeSelection)
		}
		ui.Row(c.Key("workspace-title")).Grow(1).MinWidth(0).Gap(8).AlignItems(ui.Center).
			Transition(ui.ElementTransition{Duration: workspacePaneMotionDuration, Position: true}).Children(func() {
			title := p.workspaceTitle()
			icon := "message-square"
			if p.selected == "" {
				icon = "folder"
			}
			ui.Icon(c, workspaceIcon(icon)).Size(workspaceIconSize, workspaceIconSize).TextColor(t.TextMuted).Shrink(0)
			location := p.workspaceLocation()
			if p.selected == "" {
				ui.ButtonBase(c.Key("workspace-title-button")).Label("Workspace title " + title).Tooltip("选择项目文件夹 · " + location).MinWidth(0).Shrink(1).
					Padding(0).Gap(4).AlignItems(ui.Center).Justify(ui.Start).Disabled(p.busy || p.window == nil).
					OnClick(p.chooseWorkspace).Children(func() {
					ui.Text(c, title).FontSize(workspaceFontBody).FontWeight(600).LineHeight(1.5).SingleLine().MinWidth(0).Shrink(1)
					ui.Icon(c, workspaceIcon("chevron-down")).Size(12, 12).TextColor(t.TextMuted).Shrink(0)
				})
			} else {
				ui.Text(c, title).Label("Workspace title " + title).Tooltip(location).FontSize(workspaceFontBody).FontWeight(600).
					LineHeight(1.5).SingleLine().MinWidth(0).Shrink(1)
			}
		})
		if p.workspaceComposerIsDemo() {
			ui.Text(c, "演示模式").FontSize(workspaceFontCaption).TextColor(t.TextMuted).SingleLine()
		}
		if reserveRight {
			ui.Box(c).Size(workspaceIconControlSize, workspaceIconControlSize).Shrink(0)
		}
	})
}

// Pane controls live in the unanimated shell. Only the left control changes
// position, from the navigation's trailing corner to the native controls' right.
func (p *NativePage) buildWorkspacePaneControls(c *ui.Context) {
	viewport, _ := c.Size()
	left := workspaceWindowInset + p.workspaceSidebarDisplayedWidth(viewport) - workspaceWindowInset - workspaceIconControlSize
	label, hint := "Collapse session navigation", "收起导航"
	if p.layout.sidebarCollapsed {
		left = max(workspaceWindowInset+3*workspaceRegionGap, c.TitleBar().Left+workspaceWindowInset)
		label, hint = "Expand session navigation", "展开导航"
	}
	workspaceIconAction(c, "workspace-toggle-sidebar", label, "panel-left").
		Absolute().Left(left).Top(workspaceTopInset(c) + (workspaceHeaderHeight-workspaceIconControlSize)/2).
		Transition(ui.ElementTransition{Duration: workspacePaneMotionDuration, Position: true}).
		Tooltip(hint).OnClick(p.toggleWorkspaceSidebar)
	p.buildWorkspaceResourceToggle(c, workspaceTopInset(c), workspaceWindowInset)
}

// A single shell-owned control keeps the same hit target while panes animate.
// The resource-only test harness supplies its own insets without a window shell.
func (p *NativePage) buildWorkspaceResourceToggle(c *ui.Context, top, inset float32) {
	label, hint, action := "Open resource workspace", "展开资料栏", p.openWorkspacePanel
	if p.workspaceResourcesVisible() {
		label, hint, action = "Collapse resource workspace", "收起资料栏", p.collapseWorkspaceResources
	}
	right := workspaceRightSafeInset(c)
	workspaceIconAction(c, "workspace-resources", label, "panel-right").
		Absolute().Right(inset + right).Top(top + (workspaceHeaderHeight-workspaceIconControlSize)/2).
		Tooltip(hint).OnClick(action)
}

func (p *NativePage) openWorkspacePanel() {
	p.resources.suppressed = false
	if p.selected == "" || p.Service.WorkspaceViews == nil {
		p.resources.emptyOpen = true
		return
	}
	p.openWorkspaceResources()
}

func (p *NativePage) workspaceComposerIsDemo() bool {
	target := p.workspaceComposerTarget()
	return p.Service.Demo && target.EngineID == "alva" && hub.RouteKey(target) == hub.RouteKey(p.Service.DefaultTarget)
}

func (p *NativePage) buildWorkspaceMenu(m *ui.Menu) {
	if m.Item("新任务").Disabled(p.busy).Chosen() {
		p.startWorkspaceTask()
	}
	if m.Item("新窗口").Disabled(p.openWindow == nil).Chosen() {
		if err := p.openWindow(p.selected); err != nil {
			p.problem = err.Error()
		}
	}
	if m.Item("设置").Disabled(p.Service.Preferences == nil).Chosen() {
		p.openPreferences()
	}
	if m.Item("开发者").Chosen() {
		p.openDeveloper()
	}
	if p.selected == "" {
		if m.Item("选择项目文件夹").Disabled(p.busy || p.window == nil).Chosen() {
			p.chooseWorkspace()
		}
		if m.Item("使用私有工作区").Disabled(p.busy || p.workspace == "").Chosen() {
			p.workspace = ""
		}
		return
	}
	m.Separator()
	for _, action := range []struct {
		label    string
		disabled bool
		open     func()
	}{
		{"对话", false, func() { p.setPane("") }},
		{"会话设置", p.paneBusy, p.openSettings},
		{"等待中的任务", p.paneBusy, p.openWaits},
		{"Inspector", p.paneBusy, p.openInspector},
		{"路由与故障切换", p.paneBusy, func() { p.setPane("routing") }},
		{"交接记录", p.paneBusy, func() { p.setPane("handoffs"); p.handoffText = "" }},
		{"媒体资料", p.paneBusy, p.openMedia},
		{"项目技能", p.paneBusy, p.openSkills},
		{"文件与任务", p.Service.WorkspaceViews == nil || p.detail.Session.ID != p.selected, p.openWorkspaceResources},
	} {
		if m.Item(action.label).Disabled(action.disabled).Chosen() {
			action.open()
		}
	}
	if p.Service.Teams != nil && m.Item("协作").Disabled(p.paneBusy).Chosen() {
		p.openTeam()
	}
	if p.Service.Automations != nil && m.Item("自动化").Disabled(p.paneBusy).Chosen() {
		p.openAutomations()
	}
	m.Separator()
	if m.Item("更早的消息").Disabled(p.busy || p.polling || p.transcript.First == 0).Chosen() {
		p.older()
	}
	if m.Item("最新消息").Disabled(p.busy || p.polling).Chosen() {
		p.loadTail()
	}
}

func (p *NativePage) buildWorkspaceWelcome(c *ui.Context) {
	t := c.Theme()
	ui.Column(c).Label("New task welcome").Grow(1).FillWidth().MinHeight(0).Padding(24).
		AlignItems(ui.Center).Justify(ui.Center).Children(func() {
		ui.Column(c).FillWidth().MaxWidth(workspaceReadingWidth).Gap(12).Children(func() {
			workspaceBrandMark(c, "Welcome brand", 32, workspaceFontHeading).Border(1, t.Border)
			ui.Text(c, "今天，想完成什么？").Role(ui.RoleHeading).Level(1).
				FontSize(workspaceFontHeading).FontWeight(600).LineHeight(1.5).TextColor(t.Text).Margin(0, 0, 4)
			if p.problem != "" {
				ui.Text(c, p.problem).Role(ui.RoleStatus).FontSize(workspaceFontBody).TextColor(t.Danger)
			}
			ui.Column(c).FillWidth().Gap(12).Children(func() {
				p.buildWorkspaceComposer(c)
				ui.Row(c).Gap(8).Wrap().Children(func() {
					for _, suggestion := range []struct{ label, icon, draft string }{
						{"理解这个项目", "folder", "请帮我理解这个项目的结构与主要功能。"},
						{"整理资料", "book-open", "请帮我整理资料，提炼重点并形成清晰的结构。"},
						{"检查代码", "code-xml", "请检查代码，找出潜在问题并说明改进建议。"},
					} {
						workspaceControlIconButton(c, suggestion.label, suggestion.label, suggestion.icon, "ghost", p.busy).OnClick(func() { p.model.Draft = suggestion.draft })
					}
				})
			})
		})
	})
}

func workspaceBrandMark(c *ui.Context, label string, size, fontSize float32) ui.Element {
	t := c.Theme()
	return ui.Box(c).Label(label).Size(size, size).Shrink(0).Radius(8).Background(t.Accent).
		AlignItems(ui.Center).Justify(ui.Center).Children(func() {
		ui.Text(c, "a").FontSize(fontSize).FontWeight(600).TextColor(t.AccentText).LineHeight(1.5)
	})
}
