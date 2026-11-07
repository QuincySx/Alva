// INPUT: Committed Hub targets, redacted connection metadata and MyGo virtual lists.
// OUTPUT: Whole-row ModelRow states, independent favorites/options and exact-route keyboard selection.
// POS: [L7 Hub UI] Shared row geometry follows Foundation; browsing and row actions never send.
//go:build desktop

package hubdesktop

import (
	"fmt"
	"strings"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/egoist/mygo/ui"
)

type workspaceModelRow struct {
	key, connection, source, name, model, capabilities, search string
	routes                                                     []hub.Target
	chosen                                                     int
	favorite                                                   bool
}

type workspaceModelSource struct{ key, name, icon string }

type workspaceModelPickerState struct {
	open                     bool
	saving                   bool
	sequence                 uint64
	defaults                 workspaceModelDefaultsState
	query, provider, problem string
	lastQuery, lastProvider  string
	rows                     []workspaceModelRow
	sources                  []workspaceModelSource
	visible                  []int
	active                   int
	list, providers          ui.ListState
	config                   string
	optionsPanel             ui.Handle
	inlineOptions            workspaceModelOptionsState
}

func workspaceTargetLabel(target hub.Target) string {
	if target.ModelID == "" {
		return "选择模型"
	}
	return target.ModelID
}

func workspaceTargetDetails(target hub.Target) string {
	return fmt.Sprintf("%s · %s\n%s · %s", strings.ToUpper(target.EngineID), target.ProviderID, target.AccountSource, target.BillingRoute)
}

// Dynamic connection revisions protect execution, but a model's presentation
// identity survives a revision. Static routes retain the account and billing
// boundary, so equal model names never collapse distinct connections.
func workspaceModelConnection(target hub.Target) string {
	if strings.HasPrefix(target.AccountSource, "connection:") {
		if index := strings.LastIndexByte(target.AccountSource, '@'); index >= len("connection:") {
			return target.AccountSource[:index]
		}
	}
	return target.ProviderID + "\x00" + target.AccountSource + "\x00" + target.BillingRoute
}

func workspaceModelIdentity(target hub.Target) string {
	return workspaceModelConnection(target) + "\x00" + target.ModelID
}

func workspaceModelRows(targets []hub.Target, connections []hub.ModelConnection, current, fallback hub.Target) ([]workspaceModelRow, []workspaceModelSource) {
	metadata := make(map[string]hub.ModelConnection, len(connections))
	models := make(map[string]hub.ConnectionModel)
	for _, connection := range connections {
		metadata["connection:"+connection.ID] = connection
		for _, model := range connection.Models {
			models["connection:"+connection.ID+"\x00"+model.ID] = model
		}
	}
	rows := make([]workspaceModelRow, 0, len(targets))
	sources := []workspaceModelSource{{name: "全部模型", icon: "cpu"}}
	indices := make(map[string]int, len(targets))
	seenSources := make(map[string]bool)
	for _, target := range targets {
		key, connection := workspaceModelIdentity(target), workspaceModelConnection(target)
		index, exists := indices[key]
		if !exists {
			row := workspaceModelRow{key: key, connection: connection, name: target.ModelID, model: target.ModelID,
				source: target.ProviderID + " · " + target.AccountSource, capabilities: "能力未声明", chosen: -1}
			if entry, ok := metadata[connection]; ok {
				row.source = entry.Name
				if model, exists := models[key]; exists {
					if model.Name != "" {
						row.name = model.Name
					}
					if model.ImageCapable {
						row.capabilities = "视觉 · 已声明"
					}
				}
			}
			index, indices[key] = len(rows), len(rows)
			rows = append(rows, row)
			if !seenSources[connection] {
				icon := "globe"
				if metadata[connection].Protocol == "anthropic" {
					icon = "cpu"
				}
				if metadata[connection].Protocol == "gemini" {
					icon = "sparkles"
				}
				sources = append(sources, workspaceModelSource{key: connection, name: row.source, icon: icon})
				seenSources[connection] = true
			}
		}
		duplicate := false
		for _, route := range rows[index].routes {
			duplicate = duplicate || hub.RouteKey(route) == hub.RouteKey(target)
		}
		if !duplicate {
			rows[index].routes = append(rows[index].routes, target)
		}
	}
	for i := range rows {
		row := &rows[i]
		// The actual current route wins, then compatible current/default Agent.
		for j, route := range row.routes {
			if hub.RouteKey(route) == hub.RouteKey(current) {
				row.chosen = j
				break
			}
		}
		for _, engine := range []string{current.EngineID, fallback.EngineID} {
			if row.chosen >= 0 {
				break
			}
			for j, route := range row.routes {
				if route.EngineID == engine {
					row.chosen = j
					break
				}
			}
		}
		if row.chosen < 0 && len(row.routes) == 1 {
			row.chosen = 0
		}
		row.search = strings.ToLower(row.name + "\n" + row.model + "\n" + row.source)
	}
	return rows, sources
}

func (s *workspaceModelPickerState) filter() {
	s.visible = s.visible[:0]
	query := strings.Fields(strings.ToLower(s.query))
	for i, row := range s.rows {
		if s.provider == "favorites" && !row.favorite || s.provider != "" && s.provider != "favorites" && row.connection != s.provider {
			continue
		}
		matches := true
		for _, word := range query {
			matches = matches && strings.Contains(row.search, word)
		}
		if matches {
			s.visible = append(s.visible, i)
		}
	}
	s.active, s.config = 0, ""
	s.lastQuery, s.lastProvider = s.query, s.provider
	s.list.ScrollTo(0, ui.Start)
}

func (p *NativePage) moveWorkspaceModel(delta int, absolute bool) {
	s := &p.composer.modelPicker
	if len(s.visible) == 0 {
		return
	}
	if absolute {
		s.active = min(max(delta, 0), len(s.visible)-1)
	} else {
		s.active = min(max(s.active+delta, 0), len(s.visible)-1)
	}
	s.list.ScrollIntoView(s.active)
	p.composer.targetFocus.RequestFocus()
}

func (p *NativePage) chooseWorkspaceModel(index int) {
	s := &p.composer.modelPicker
	if !p.composer.targetOpen || p.workspacePickerBlocked() || index < 0 || index >= len(s.rows) {
		return
	}
	row := s.rows[index]
	if row.chosen < 0 || row.chosen >= len(row.routes) {
		s.problem = "请先为这个模型选择可用 Agent。"
		return
	}
	target := row.routes[row.chosen]
	if problem := p.workspaceSessionModelOptionsProblem(); problem != "" {
		s.problem = problem
		return
	}
	if problem := p.workspaceGoalTargetProblem(target); problem != "" {
		s.problem = problem
		return
	}
	if p.workspaceTargetOptions(target).ValidateTarget(target) != nil {
		s.problem = "这个 Agent 不支持已保存的模型参数，请先在参数设置中调整。"
		return
	}
	if p.Service.ValidateTarget(target) != nil {
		s.problem = "这条连接已更改或停用。请关闭后重新选择，草稿已保留。"
		return
	}
	current := p.workspaceComposerTarget()
	p.composer.closePopovers()
	if hub.RouteKey(current) == hub.RouteKey(target) {
		return
	}
	if p.selected == "" {
		p.composer.target = target
	} else {
		p.switchBranch(target)
	}
}

func (p *NativePage) buildWorkspaceTargetMenu(c *ui.Context, anchor ui.Element, current hub.Target) {
	s := &p.composer.modelPicker
	if !p.composer.targetOpen {
		s.open = false
		return
	}
	if !s.open {
		s.sequence++
		s.open, s.query, s.provider, s.problem, s.config = true, "", "", "", ""
		var connections []hub.ModelConnection
		if p.Service.Connections != nil {
			connections = p.Service.Connections.Snapshot()
		}
		s.rows, s.sources = workspaceModelRows(p.Service.AvailableTargets(), connections, current, p.Service.DefaultTarget)
		if p.Service.Connections != nil {
			s.sources = append(s.sources[:1], append([]workspaceModelSource{{key: "favorites", name: "收藏", icon: "star"}}, s.sources[1:]...)...)
			for i := range s.rows {
				row := &s.rows[i]
				pref := p.Service.Connections.ModelPreference(row.routes[0])
				row.favorite = pref.Favorite
				if row.key != workspaceModelIdentity(current) && pref.PreferredEngine != "" {
					for j, route := range row.routes {
						if route.EngineID == pref.PreferredEngine {
							row.chosen = j
							break
						}
					}
				}
			}
		}
		s.filter()
		for i, index := range s.visible {
			if s.rows[index].key == workspaceModelIdentity(current) {
				s.active = i
				break
			}
		}
		s.list.ScrollTo(s.active, ui.Center)
	}
	if s.query != s.lastQuery || s.provider != s.lastProvider {
		s.filter()
	}
	_, height := c.Size()
	bounds := anchor.Bounds()
	available := max(0, max(bounds.Y, height-bounds.Y-bounds.H)-10)
	ui.PopoverBase(c.Key("models"), anchor, &p.composer.targetOpen, func(panel ui.Element) {
		panel.Label("Composer menu").Bind(&s.optionsPanel).AttachTo(anchor, ui.AnchorTopRight, ui.AnchorBottomRight).
			Width(640).MaxWidthPercent(95).Margin(0, 0, 6, 0).Height(min(560, available)).MinHeight(0).Padding(16).Gap(16).
			Radius(16).Background(c.Theme().Surface).
			Shadow(0, 6, 20, 0, ui.RGBA(0, 0, 0, 0.10))
		panel.DrawOver(func(p *ui.Painter, r ui.Rect) { p.Stroke(r, c.Theme().Border, 16, 1) })
		ui.Row(c.Key("search-row")).FillWidth().Height(workspaceControlHeight).Shrink(0).Gap(8).AlignItems(ui.Center).Children(func() {
			var input ui.Element
			box := ui.Row(c.Key("model-search-box")).Grow(1).MinWidth(0).Height(workspaceControlHeight).Padding(0, 12).Gap(8).AlignItems(ui.Center).
				Radius(8).Background(c.Theme().Surface).Border(1, workspacePalette(c).controlBorder).Children(func() {
				ui.Icon(c, workspaceIcon("search")).Size(16, 16).TextColor(c.Theme().TextMuted)
				input = ui.TextInputBase(c.Key("search"), &s.query).Label("Search models").Placeholder("搜索模型…").
					Grow(1).MinWidth(0).FillHeight().Padding(0).FontSize(workspaceFontBody).LineHeight(1.5).FocusRing(false).
					Background(ui.Transparent).OnChange(func() { s.filter() }).OnTextKey(func(k ui.TextKey) bool {
					if k.Mods != 0 {
						return false
					}
					switch k.Key {
					case ui.KeyDown:
						p.moveWorkspaceModel(0, false)
						return true
					case ui.KeyEnter:
						if len(s.visible) > 0 {
							p.chooseWorkspaceModel(s.visible[s.active])
						}
						return true
					}
					return false
				})
			})
			box.DrawOver(func(p *ui.Painter, r ui.Rect) {
				if input.Focused() {
					p.Stroke(r, c.Theme().Accent, 8, 2)
				}
			})
			workspaceIconAction(c, "close-models", "Close model picker", "x").OnClick(func() { p.composer.closePopovers() })
		})
		ui.Row(c.Key("body")).FillWidth().Grow(1).MinHeight(0).Gap(12).Children(func() {
			ui.List(c.Key("providers"), &s.providers, len(s.sources)).Label("Model connections").Width(40).MinHeight(0).FillHeight().Gap(12).Rows(func(row ui.ListRow) {
				source := s.sources[row.Index]
				button := ui.ButtonBase(row.Context.Key(source.key)).Label("Filter models: "+source.name).
					Size(workspaceIconControlSize, workspaceIconControlSize).Padding(6).Radius(8).AlignItems(ui.Center).Justify(ui.Center).
					Selected(s.provider == source.key).TextColor(c.Theme().Text).OnClick(func() { s.provider = source.key; s.filter() })
				if s.provider == source.key {
					button.Background(workspacePalette(c).selection)
				} else if button.Hovered() || button.FocusVisible() {
					button.Background(workspacePalette(c).hover)
				}
				button.Tooltip(source.name).Children(func() {
					ui.Icon(row.Context, workspaceIcon(source.icon)).Size(workspaceIconSize, workspaceIconSize).TextColor(c.Theme().Text)
				})
			})
			ui.Column(c.Key("results")).Grow(1).MinWidth(0).MinHeight(0).FillHeight().Children(func() {
				if len(s.visible) == 0 {
					message := "没有匹配的模型"
					if len(s.rows) == 0 {
						message = "还没有可用模型，请先配置连接。"
					}
					ui.Text(c, message).Role(ui.RoleStatus).FontSize(workspaceFontBody).LineHeight(1.5).TextColor(c.Theme().TextMuted).Padding(8).FillWidth()
					return
				}
				s.list.Key = func(i int) any { return s.rows[s.visible[i]].key }
				list := ui.List(c.Key("model-list"), &s.list, len(s.visible)).Label("Configured agents and models").
					FillWidth().Grow(1).MinHeight(0).Gap(4)
				list.OnShortcut(0, ui.KeyHome, func() { p.moveWorkspaceModel(0, true) }).
					OnShortcut(0, ui.KeyEnd, func() { p.moveWorkspaceModel(len(s.visible)-1, true) }).
					OnShortcut(0, ui.KeyUp, func() { p.moveWorkspaceModel(-1, false) }).
					OnShortcut(0, ui.KeyDown, func() { p.moveWorkspaceModel(1, false) })
				list.Rows(func(item ui.ListRow) { p.buildWorkspaceModelRow(item.Context, item.Index, current) })
			})
		})
		if s.problem != "" {
			ui.Text(c, s.problem).Role(ui.RoleStatus).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(c.Theme().Danger).FillWidth()
			workspaceControlButton(c, "reload-model-preferences", "重新加载", "secondary", s.saving).Label("Reload model preferences").OnClick(func() { p.reloadWorkspaceModelPreferences(false) })
		}
		ui.ButtonBase(c.Key("configure")).Label("Configure models").FillWidth().Height(workspaceControlHeight).Shrink(0).Padding(0, 12).Gap(8).AlignItems(ui.Center).Radius(8).
			Justify(ui.Start).OnClick(func() { p.composer.closePopovers(); p.openConnections() }).Children(func() {
			ui.Icon(c, workspaceIcon("settings")).Size(16, 16)
			ui.Text(c, "配置模型").FontSize(workspaceFontBody)
		})
	})
}

func (p *NativePage) buildWorkspaceModelRow(c *ui.Context, visible int, current hub.Target) {
	s := &p.composer.modelPicker
	index := s.visible[visible]
	row := &s.rows[index]
	selected := false
	label := "Choose Agent · " + row.model
	if row.chosen >= 0 {
		target := row.routes[row.chosen]
		selected = hub.RouteKey(target) == hub.RouteKey(current)
		label = "Use " + target.EngineID + " · " + target.ModelID
	}
	// MyGo dispatches a click to the nearest interactive child. The row owns
	// the full surface; its two actions keep their own pointer/keyboard input.
	button := ui.ButtonBase(c.Key(row.key)).Role(ui.RoleListItem).Label(label).Selected(selected).
		FillWidth().MinWidth(0).Padding(8, 12).Gap(8).Radius(8).
		AlignItems(ui.Center).Justify(ui.Start).TextColor(c.Theme().Text).FocusRing(false).
		Disabled(p.workspacePickerBlocked()).OnClick(func() { p.chooseWorkspaceModel(index) })
	if visible == s.active {
		button.Bind(&p.composer.targetFocus)
	}
	if selected {
		button.Background(workspacePalette(c).subtle)
	} else if button.Hovered() || button.FocusWithin() || s.config == row.key {
		button.Background(workspacePalette(c).hover)
	}
	button.DrawOver(func(painter *ui.Painter, bounds ui.Rect) {
		if button.FocusVisible() {
			painter.Stroke(bounds, c.Theme().Accent, 8, 2)
		}
	})
	button.OnShortcut(0, ui.KeyHome, func() { p.moveWorkspaceModel(0, true) }).
		OnShortcut(0, ui.KeyEnd, func() { p.moveWorkspaceModel(len(s.visible)-1, true) }).
		OnShortcut(0, ui.KeyUp, func() { p.moveWorkspaceModel(visible-1, true) }).
		OnShortcut(0, ui.KeyDown, func() { p.moveWorkspaceModel(visible+1, true) })
	button.Children(func() {
		ui.Column(c.Key("copy")).Grow(1).MinWidth(0).Gap(4).Children(func() {
			ui.Text(c, row.name).FontSize(workspaceFontBody).FontWeight(600).LineHeight(1.5).TextColor(c.Theme().Text).SingleLine().MinWidth(0)
			ui.Text(c, row.source+" · "+row.capabilities).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(c.Theme().TextMuted).SingleLine().MinWidth(0)
		})
		favorite := workspaceModelRowAction(c.Key("favorite"), "star", "Favorite model: "+row.model, "收藏模型").
			Checked(row.favorite).Disabled(s.saving || p.Service.Connections == nil).
			OnClick(func() { p.toggleWorkspaceModelFavorite(row.routes[0]) })
		if row.favorite {
			favorite.TextColor(c.Theme().Accent)
		}
		if p.Service.Connections == nil {
			favorite.Description("当前未装配模型偏好存储，收藏不可用。")
		}
		config := workspaceModelRowAction(c.Key("agent"), "sliders-horizontal", "Model options: "+row.model, "选择兼容 Agent").
			Expanded(s.config == row.key).OnClick(func() {
			if s.config == row.key {
				s.config = ""
			} else {
				s.config = row.key
			}
		})
		p.buildWorkspaceModelOptions(c, config, row)
	})
}

// Alva ModelRow uses the existing IconButton geometry:32px target,20px
// glyph. It shares the row surface instead of painting separate hover boxes.
func workspaceModelRowAction(c *ui.Context, icon, label, hint string) ui.Element {
	button := ui.ButtonBase(c).Label(label).Tooltip(hint).Size(workspaceIconControlSize, workspaceIconControlSize).Padding(0).Radius(8).
		AlignItems(ui.Center).Justify(ui.Center).TextColor(c.Theme().TextMuted)
	if button.Hovered() || button.FocusVisible() {
		button.TextColor(c.Theme().Text)
	}
	button.Children(func() { ui.Icon(c, workspaceIcon(icon)).Size(workspaceIconSize, workspaceIconSize).Shrink(0) })
	return button
}
