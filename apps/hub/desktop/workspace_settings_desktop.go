// INPUT: Existing workspace panes, minimal media index locations and native MyGo controls.
// OUTPUT: Shared Settings navigation and one return destination retained across category changes.
// POS: [L7 Hub UI] Navigation preserves drafts and execution owners while releasing hidden media bodies.
//go:build desktop

package hubdesktop

import (
	"github.com/QuincySx/Alva/platform/storage"
	"github.com/egoist/mygo/ui"
)

const workspaceSettingsNavWidth float32 = 208

type workspaceMediaLocation struct {
	branch, mode, source, selection string
	cursor, highWater, request      uint64
	back                            []uint64
}

type workspaceReturnState struct {
	pane, selected string
	generation     uint64
	media          workspaceMediaLocation
	restoreMedia   bool
}

func workspaceSettingsPane(name string) bool {
	switch name {
	case "preferences", "developer", "connections", "gateway", "settings-scope",
		"settings-plugins", "settings-rules", "settings-data", "settings-security",
		"settings", "plugins", "tools", "commands", "media-settings", "browser-settings", "skills":
		return true
	}
	return false
}

func (p *NativePage) workspaceSettingsCategory() string {
	switch p.pane {
	case "preferences":
		return "general"
	case "connections":
		return "models"
	case "developer":
		return "developer"
	case "gateway":
		return "gateway"
	case "settings-scope":
		return p.settingsScope.destination
	case "settings-plugins", "settings", "plugins", "tools", "commands", "media-settings", "browser-settings":
		return "plugins"
	case "settings-rules", "skills":
		return "rules"
	case "settings-data":
		return "data"
	case "settings-security":
		return "security"
	}
	return ""
}

func (p *NativePage) openWorkspaceSettingsCategory(category string) {
	switch category {
	case "general":
		p.openPreferences()
	case "models":
		if p.Service.Connections != nil {
			p.openConnections()
		}
	case "developer":
		p.openDeveloper()
	case "gateway":
		p.setPane("gateway")
	case "plugins", "rules", "data", "security":
		p.setPane("settings-" + category)
	}
}

func (p *NativePage) isWorkspaceSettings() bool {
	return workspaceSettingsPane(p.pane)
}

// Settings categories share a single origin; switching categories never creates
// another return step or reconstructs an Inspector page from its first cursor.
func (p *NativePage) workspacePaneDestination(name string) string {
	if name != p.pane && (workspaceSettingsPane(name) || p.isWorkspaceSettings()) {
		p.composer.closePopovers()
	}
	if workspaceSettingsPane(name) {
		if !p.isWorkspaceSettings() {
			p.composer.closePopovers()
			p.workspaceReturn = workspaceReturnState{pane: p.pane, selected: p.selected, generation: p.generation}
			if p.pane == "media" {
				v := &p.media
				p.workspaceReturn.media = workspaceMediaLocation{branch: v.Branch, mode: v.Mode, source: v.SourceURI, selection: v.SelectionID,
					cursor: v.Cursor, highWater: v.Page.HighWater, request: v.Request, back: append([]uint64(nil), v.Back...)}
			}
		}
		return name
	}
	if !p.isWorkspaceSettings() {
		return name
	}
	origin := p.workspaceReturn
	p.workspaceReturn = workspaceReturnState{}
	if name != "" || origin.selected != p.selected || origin.generation != p.generation {
		return name
	}
	if origin.pane == "media" {
		v := origin.media
		p.media = MediaPane{Branch: v.branch, Mode: v.mode, SourceURI: v.source, SelectionID: v.selection,
			Cursor: v.cursor, Back: v.back, Request: v.request + 1, Page: storage.ArtifactIndexPage{HighWater: v.highWater}}
		origin.restoreMedia = true
		p.workspaceReturn = origin
	}
	return origin.pane
}

func (p *NativePage) buildWorkspaceSettingsCategory(c *ui.Context) {
	category := p.workspaceSettingsCategory()
	ui.Text(c, workspaceScopeLabel(category)).FontSize(workspaceFontHeading).FontWeight(600).LineHeight(1.5)
	if category == "data" {
		ui.Text(c, "数据目录").FontSize(workspaceFontBody).LineHeight(1.5)
		directory := p.Service.DataDirectory
		if directory == "" {
			directory = "当前应用未声明数据目录。"
		}
		ui.Text(c, directory).Label("Application data directory " + directory).FontSize(workspaceFontBody).LineHeight(1.5).TextColor(c.Theme().TextMuted)
	}
	ready := p.workspaceNavigationDetailReady() && !p.chat.Busy
	ui.Row(c).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
		label := "尚未选择任务"
		if ready {
			label = "当前任务 · " + p.workspaceTitle()
		} else if p.selected != "" {
			label = "当前任务正在读取…"
		}
		ui.Text(c, label).FontSize(workspaceFontBody).LineHeight(1.5).Grow(1).MinWidth(0).SingleLine()
		workspaceControlButton(c, "settings-choose-session", "选择任务", "secondary", p.busy || p.paneBusy).
			Label("Choose settings session").OnClick(func() { p.openWorkspaceSessionChooser(category) })
	})
	if !ready {
		ui.Text(c, "此分类的操作绑定已保存的任务，请先选择任务。").FontSize(workspaceFontBody).LineHeight(1.5).TextColor(c.Theme().TextMuted)
		return
	}
	switch category {
	case "plugins":
		ui.Text(c, "配置在下一轮生效；启用主机能力仍需单独确认。").FontSize(workspaceFontBody).LineHeight(1.5).TextColor(c.Theme().TextMuted)
		for _, item := range []struct {
			key, title, icon string
			open             func()
		}{
			{"mcp", "MCP 与原生 Agent", "plug", p.openSettings},
			{"aep", "扩展插件", "plug", p.openAEP},
			{"tools", "工具", "sliders-horizontal", p.openTools},
			{"commands", "Hooks 与诊断", "code-xml", p.openCommands},
			{"browser", "浏览器工具", "globe", p.openBrowserSettings},
			{"media", "媒体工具", "folder", p.openMediaSettings},
		} {
			workspaceAction(c, "settings-open-"+item.key, item.title, item.icon, false).Height(workspaceControlHeight).Padding(0, 12).
				Disabled(p.paneBusy).OnClick(item.open)
		}
	case "rules":
		ui.Text(c, "项目 Skills 与显式项目规则，使用当前任务已保存的工作区。").FontSize(workspaceFontBody).LineHeight(1.5).TextColor(c.Theme().TextMuted)
		workspaceControlIconButton(c, "settings-open-skills", "管理规则与 Skills", "book-open", "secondary", p.paneBusy).
			Label("Manage project rules and Skills").OnClick(p.openSkills)
	case "data":
		workspaceControlIconButton(c, "settings-open-records", "查看并导出会话记录", "folder", "secondary", p.paneBusy).
			Label("View and export session records").OnClick(p.openInspector)
	case "security":
		label, description := p.workspacePermissionDescription(p.workspaceComposerTarget().EngineID)
		ui.Text(c, label).FontSize(workspaceFontSubtitle).LineHeight(1.5)
		ui.Text(c, description).FontSize(workspaceFontBody).LineHeight(1.5).TextColor(c.Theme().TextMuted)
		anchor := workspaceControlIconButton(c, "settings-permission", "修改下一轮权限", "shield-check", "secondary", !p.workspacePlanCanChange()).
			Label("Settings permission policy").OnClick(func() { p.composer.permissionOpen = !p.composer.permissionOpen })
		p.buildNativePermissionMenu(c, anchor)
	}
}

// An explicit return reads only its prior metadata page. If a read from before
// Settings is still settling, keep the location until that owner releases it.
func (p *NativePage) restoreWorkspacePane() {
	origin := p.workspaceReturn
	if !origin.restoreMedia {
		return
	}
	if p.pane != "media" || p.selected != origin.selected || p.generation != origin.generation {
		p.workspaceReturn = workspaceReturnState{}
		return
	}
	if p.paneBusy {
		return
	}
	p.workspaceReturn = workspaceReturnState{}
	p.loadMedia(origin.media.cursor, origin.media.highWater)
}

func (p *NativePage) buildWorkspaceSettings(c *ui.Context) {
	t := c.Theme()
	if p.pane == "connections" && p.connections.mode != "" {
		ui.Column(c).Fill().Padding(workspaceTopInset(c), workspaceWindowInset, workspaceWindowInset, workspaceWindowInset).Background(workspacePalette(c).scrim).Children(func() {
			workspaceHeaderRegion(c, "connection-sheet-header", "Connection sheet header", workspaceWindowInset, func() {
				busy := p.connections.saving || p.connections.loading
				if p.connections.mode == "form" {
					label := "返回选择供应商"
					if p.connections.draft.ID != "" {
						label = "返回模型与连接"
					}
					connectionButton(c, "connection-form-back", label, "chevron-left", busy).
						Label("Back from connection form").AlignSelf(ui.Center).OnClick(p.backConnectionForm)
				} else {
					connectionButton(c, "connections-back-manager", "返回模型与连接", "chevron-left", busy).AlignSelf(ui.Center).OnClick(p.cancelConnectionDraft)
				}
			})
			ui.Column(c).Label("Connection sheet body").FillWidth().Grow(1).MinHeight(0).Padding(24).
				AlignItems(ui.Center).Justify(ui.Center).Children(func() {
				p.buildApprovals(c)
				p.buildQuestions(c)
				p.buildConnections(c)
			})
		})
		return
	}
	workspaceShell(c).Children(func() {
		ui.Column(c.Key("settings-navigation-column")).Label("Settings navigation column").Width(workspaceSettingsNavWidth).Shrink(0).
			FillHeight().Gap(workspaceRegionGap).Children(func() {
			workspaceColumnToolbar(c, "settings-navigation-header", "Settings navigation header", workspaceLeftSafeInset(c, workspaceWindowInset), workspaceWindowInset, func() {
				ui.Text(c, "设置").FontSize(workspaceFontSubtitle).FontWeight(600).LineHeight(1.5).SingleLine()
			})
			workspaceAuxiliarySurface(c, "settings-navigation-surface", "Settings navigation").FillWidth().Children(func() {
				ui.Column(c.Key("settings-navigation")).Label("Settings categories").Fill().Padding(8).Gap(2).Children(func() {
					for _, category := range []struct{ key, title, icon, label string }{
						{"general", "通用", "sliders-horizontal", "General settings"},
						{"models", "模型与连接", "plug", "Models and connections"},
						{"plugins", "插件与工具", "puzzle", "Plugins and tools"},
						{"rules", "规则与 Skills", "book-open", "Rules and Skills"},
						{"data", "数据与存储", "database", "Data and storage"},
						{"security", "权限与安全", "shield", "Permissions and security"},
						{"gateway", "Gateway", "globe", "Gateway settings"},
						{"developer", "开发者", "code-xml", "Developer options"},
					} {
						disabled := category.key == "general" && p.Service.Preferences == nil || category.key == "models" && p.Service.Connections == nil
						workspaceAction(c, "settings-"+category.key, category.title, category.icon, p.workspaceSettingsCategory() == category.key).
							Height(workspaceControlHeight).Padding(0, 12).Label(category.label).Disabled(disabled).
							OnClick(func() { p.openWorkspaceSettingsCategory(category.key) })
					}
				})
			})
		})
		alignment := ui.Center
		if p.pane == "preferences" {
			alignment = ui.Start
		}
		ui.Column(c.Key("settings-main-column")).Label("Settings main column").Grow(1).MinWidth(0).FillHeight().Gap(workspaceRegionGap).Children(func() {
			mainLeft := workspaceWindowInset + workspaceSettingsNavWidth + workspaceRegionGap
			workspaceColumnToolbar(c, "settings-header", "Settings header", workspaceLeftSafeInset(c, mainLeft), workspaceRightSafeInset(c), func() {
				workspaceControlIconButton(c, "settings-back-workspace", "返回工作台", "chevron-left", "ghost").
					Label("Back to workspace").AlignSelf(ui.Center).OnClick(func() { p.setPane("") })
			})
			workspaceMainSurface(c, "settings-main", "Settings main surface").AlignItems(alignment).Children(func() {
				maxWidth := float32(1120)
				gap := float32(24)
				if p.pane == "preferences" {
					maxWidth, gap = 800, 16
				}
				ui.Column(c).Label("Settings body").FillWidth().MinWidth(0).MaxWidth(maxWidth).Grow(1).MinHeight(0).Padding(24).Gap(gap).Children(func() {
					if p.playerBusy {
						ui.Button(c, "暂停本地播放").Label("Pause local video and wait for decoder cleanup").Disabled(p.playerStopping).OnClick(p.stopPlayback)
					}
					if p.problem != "" {
						ui.Text(c, p.problem).Role(ui.RoleStatus).TextColor(t.Danger).FontSize(workspaceFontBody)
					}
					p.buildApprovals(c)
					p.buildQuestions(c)
					if p.pane == "developer" {
						p.buildDeveloper(c)
						return
					}
					if p.pane == "connections" {
						p.buildConnections(c)
						return
					}
					if p.pane == "preferences" {
						p.buildPreferences(c)
						return
					}
					if p.pane == "settings-scope" {
						p.buildWorkspaceScopeChooser(c)
						return
					}
					if p.pane == "gateway" {
						p.buildGateway(c)
						return
					}
					if p.pane == "settings-plugins" || p.pane == "settings-rules" || p.pane == "settings-data" || p.pane == "settings-security" {
						p.buildWorkspaceSettingsCategory(c)
						return
					}
					workspaceControlIconButton(c, "settings-category-back", "返回"+workspaceScopeLabel(p.workspaceSettingsCategory()), "chevron-left", "ghost", p.paneBusy).
						Label("Back to settings category").OnClick(func() { p.openWorkspaceSettingsCategory(p.workspaceSettingsCategory()) })
					p.buildWorkspacePane(c)
				})
			})
		})
	})
}
