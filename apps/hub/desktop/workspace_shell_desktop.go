// INPUT: Current Hub page projections, formal SmallPen workspace regions and native MyGo controls.
// OUTPUT: Quiet task navigation, session header, centered welcome and explicit advanced action menus.
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

const workspaceSidebarWidth float32 = 220

func workspaceAction(c *ui.Context, key, label, icon string, selected bool) ui.Element {
	t := c.Theme()
	button := ui.ButtonBase(c.Key(key)).Label(label).FillWidth().MinWidth(0).
		Padding(8, 12).Gap(8).AlignItems(ui.Center).Radius(8).TextColor(t.Text).Selected(selected)
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
			color = t.Accent
		}
		ui.Icon(c, workspaceIcon(icon)).Size(20, 20).TextColor(color).Shrink(0)
		ui.Text(c, label).TextColor(t.Text).FontSize(14).LineHeight(1.5).Grow(1).MinWidth(0).SingleLine()
	})
	return button
}

func workspaceIconAction(c *ui.Context, key, label, icon string) ui.Element {
	t := c.Theme()
	button := ui.ButtonBase(c.Key(key)).Label(label).Tooltip(label).Size(32, 32).
		Padding(6).Radius(8).AlignItems(ui.Center).Justify(ui.Center).TextColor(t.TextMuted)
	if button.Hovered() {
		button.Background(t.SurfaceHover)
	}
	button.Children(func() { ui.Icon(c, workspaceIcon(icon)).Size(20, 20) })
	return button
}

func (p *NativePage) buildWorkspaceSidebar(c *ui.Context) {
	t := c.Theme()
	ui.Column(c).Label("Session navigation").Width(workspaceSidebarWidth).Shrink(0).
		FillHeight().Padding(12).Gap(4).Background(workspacePalette(c).sidebar).Children(func() {
		ui.Row(c).Height(44).Padding(4).Gap(8).AlignItems(ui.Center).Children(func() {
			ui.Box(c).Size(32, 32).Radius(8).Background(t.Accent).Children(func() {
				ui.Text(c, "a").FontSize(24).FontWeight(600).TextColor(t.AccentText).
					Fill().TextAlign(ui.Center).LineHeight(1.25)
			})
			ui.Text(c, "Alva").FontSize(16).FontWeight(600).Grow(1)
			workspaceIconAction(c, "workspace-window", "New window", "panels-top-left").
				Disabled(p.openWindow == nil).OnClick(func() {
				if err := p.openWindow(p.selected); err != nil {
					p.problem = err.Error()
				}
			})
		})
		workspaceAction(c, "workspace-new-task", "新任务", "plus", p.selected == "" && p.pane == "").Height(36).Padding(6, 12).
			Label("New session").Disabled(p.busy).OnClick(p.startWorkspaceTask)
		ui.Row(c).Gap(4).AlignItems(ui.Center).Padding(2, 4).Radius(8).
			Background(t.Surface.Alpha(.65)).Children(func() {
			search := ui.TextInput(c.Key("session-search"), &p.sessionQuery).
				Label("Find session title/workspace").Placeholder("搜索任务").FontSize(14).
				Grow(1).MinWidth(0).Border(0, ui.Transparent).Background(ui.Transparent).Padding(8, 4)
			if search.Submitted() && !p.sessionLoading {
				p.refreshSessions()
			}
			workspaceIconAction(c, "workspace-search", "Search / refresh sessions", "search").
				Disabled(p.sessionLoading).OnClick(p.refreshSessions)
		})
		ui.Text(c, "最近").FontSize(12).TextColor(t.TextMuted).Padding(12, 12, 4)
		p.list.Key = func(i int) any { return p.sessions[i].ID }
		p.list.Label = func(i int) string { return p.sessionLabel(p.sessions[i]) }
		if len(p.sessions) == 0 {
			label := "还没有任务"
			if p.sessionLoading {
				label = "正在加载…"
			} else if p.sessionLoadedQuery != "" {
				label = "没有找到匹配的任务"
			}
			ui.Text(c, label).FontSize(12).TextColor(t.TextMuted).Padding(4, 12)
		}
		ui.List(c.Key("workspace-session-list"), &p.list, len(p.sessions), func(i int) {
			p.buildWorkspaceSession(c, p.sessions[i])
		}).Grow(1).MinHeight(0).Gap(4)
		if len(p.sessionBack) > 0 || p.sessionPage.HasMore {
			ui.Row(c).Gap(8).AlignItems(ui.Center).Children(func() {
				workspaceIconAction(c, "workspace-previous-sessions", "Previous sessions", "chevron-left").
					Disabled(p.sessionLoading || p.sessionLoadedQuery != p.sessionQuery || len(p.sessionBack) == 0).
					OnClick(func() {
						last := len(p.sessionBack) - 1
						cursor := p.sessionBack[last]
						p.sessionBack = p.sessionBack[:last]
						p.loadSessions(cursor)
					})
				ui.Spacer(c)
				workspaceIconAction(c, "workspace-more-sessions", "More sessions", "chevron-right").
					Disabled(p.sessionLoading || p.sessionLoadedQuery != p.sessionQuery || !p.sessionPage.HasMore).
					OnClick(func() {
						p.sessionBack = append(p.sessionBack, p.sessionCursor)
						p.loadSessions(p.sessionPage.Next)
					})
			})
		}
		ui.Row(c).Gap(8).Padding(8, 4, 4).AlignItems(ui.Center).Children(func() {
			ui.Text(c, "工作台").FontSize(14).TextColor(t.TextMuted).Grow(1)
			workspaceIconAction(c, "workspace-preferences", "General settings", "settings").
				Tooltip("设置").Disabled(p.Service.Preferences == nil).OnClick(p.openPreferences)
		})
	})
}

func (p *NativePage) buildWorkspaceSession(c *ui.Context, session hub.Session) {
	t := c.Theme()
	selected := p.selected == session.ID
	button := ui.ButtonBase(c.Key(session.ID)).Label(p.sessionLabel(session)).Tooltip(p.sessionLabel(session)).
		FillWidth().MinWidth(0).MinHeight(36).Padding(6, 12).Gap(8).AlignItems(ui.Center).Radius(8).
		Selected(selected).Disabled(p.busy)
	if selected {
		button.Background(t.Surface)
	} else if button.Hovered() {
		button.Background(t.SurfaceHover)
	}
	button.OnClick(func() { p.selectSession(session.ID) }).Children(func() {
		color := t.TextMuted
		if selected {
			color = t.Accent
		}
		ui.Icon(c, workspaceIcon("message-square")).Size(20, 20).TextColor(color).Shrink(0)
		ui.Text(c, session.Title).TextColor(t.Text).FontSize(14).LineHeight(1.5).SingleLine().Grow(1).MinWidth(0)
		status := engine.Outcome(strings.TrimPrefix(p.sessionLabel(session), session.Title+" · "))
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
	return filepath.Base(filepath.Clean(path))
}

func (p *NativePage) buildWorkspaceHeader(c *ui.Context) {
	t := c.Theme()
	ui.Row(c).Label("Workspace header").Height(52).Shrink(0).Padding(0, 24).
		Gap(12).AlignItems(ui.Center).Children(func() {
		ui.Column(c).Grow(1).MinWidth(0).Gap(0).Children(func() {
			ui.Text(c, p.workspaceTitle()).FontSize(16).FontWeight(600).LineHeight(1.5).SingleLine()
			suffix := " · 项目"
			if p.selected != "" {
				suffix = " · 当前会话"
			}
			if p.selected == "" {
				workspace := ui.ButtonBase(c).Label("Choose workspace").Tooltip("选择项目文件夹").Padding(0).Gap(4).AlignItems(ui.Center).Disabled(p.busy || p.window == nil).OnClick(p.chooseWorkspace)
				workspace.Children(func() {
					ui.Text(c, p.workspaceLocation()+suffix).FontSize(12).LineHeight(1.5).TextColor(t.TextMuted).SingleLine()
					ui.Icon(c, workspaceIcon("chevron-down")).Size(12, 12).TextColor(t.TextMuted)
				})
			} else {
				ui.Text(c, p.workspaceLocation()+suffix).FontSize(12).LineHeight(1.5).TextColor(t.TextMuted).SingleLine()
			}
		})
		if p.workspaceComposerIsDemo() {
			ui.Text(c, "演示模式").FontSize(12).TextColor(t.TextMuted).SingleLine()
		}
		if p.selected != "" {
			if p.pane != "" && p.pane != "preferences" && p.pane != "developer" {
				workspaceIconAction(c, "workspace-back-chat", "Chat", "message-square").OnClick(func() { p.setPane("") })
			}
			workspaceIconAction(c, "workspace-inspector", "Inspector", "panels-top-left").
				Disabled(p.paneBusy).OnClick(p.openInspector)
		}
		workspaceIconAction(c, "workspace-more", "More actions", "ellipsis").Menu(p.buildWorkspaceMenu)
	})
}

func (p *NativePage) workspaceComposerIsDemo() bool {
	target := p.workspaceComposerTarget()
	return p.Service.Demo && target.EngineID == "alva" && hub.RouteKey(target) == hub.RouteKey(p.Service.DefaultTarget)
}

func (p *NativePage) buildWorkspaceMenu(m *ui.Menu) {
	if m.Item("新窗口").Disabled(p.openWindow == nil).Chosen() {
		if err := p.openWindow(p.selected); err != nil {
			p.problem = err.Error()
		}
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
		ui.Column(c).FillWidth().MaxWidth(760).Gap(16).Children(func() {
			ui.Text(c, "今天，想完成什么？").Role(ui.RoleHeading).Level(1).
				FontSize(32).FontWeight(600).LineHeight(1.5).TextColor(t.Text)
			if p.problem != "" {
				ui.Text(c, p.problem).Role(ui.RoleStatus).FontSize(14).TextColor(t.Danger)
			}
			ui.Column(c).FillWidth().Gap(12).Children(func() {
				p.buildWorkspaceComposer(c)
				ui.Row(c).Gap(12).Wrap().Children(func() {
					for _, suggestion := range []struct{ label, icon, draft string }{
						{"理解这个项目", "folder", "请帮我理解这个项目的结构与主要功能。"},
						{"整理资料", "book-open", "请帮我整理资料，提炼重点并形成清晰的结构。"},
						{"检查代码", "code-xml", "请检查代码，找出潜在问题并说明改进建议。"},
					} {
						button := ui.ButtonBase(c.Key(suggestion.label)).Label(suggestion.label).
							Padding(8, 12).Gap(8).Radius(8).Background(workspacePalette(c).subtle).AlignItems(ui.Center).Disabled(p.busy)
						if button.Hovered() {
							button.Background(t.SurfaceHover)
						}
						button.OnClick(func() { p.model.Draft = suggestion.draft }).Children(func() {
							ui.Icon(c, workspaceIcon(suggestion.icon)).Size(16, 16).TextColor(t.TextMuted)
							ui.Text(c, suggestion.label).FontSize(14).LineHeight(1.5)
						})
					}
				})
			})
		})
	})
}
