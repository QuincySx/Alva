// INPUT: Canonical model-option fields and session/branch-bound committed preference observations.
// OUTPUT: Revision-safe session facts, preserved row drafts and explicit session/default scope editing.
// POS: [L7 Hub UI] Unsupported Context/vision editing stays disabled; browsing never selects or sends.
//go:build desktop

package hubdesktop

import (
	"strings"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/egoist/mygo/ui"
)

type workspaceModelOptionsState struct {
	key, effort               string
	open, effortDirty, vision bool
	sequence                  uint64
}

// Kept apart from the advanced settings editor: observing a committed option
// must never replace a user's unrelated, unsaved configuration fields.
type workspaceSessionModelOptionsState struct {
	sessionID, branchID, problem string
	value                        hub.SessionModelOptionsSnapshot
	ready                        bool
	epoch                        uint64
}

func (p *NativePage) workspaceSessionModelOptionsProblem() string {
	if p.selected == "" {
		return ""
	}
	v := &p.sessionModelOptions
	if v.ready && v.sessionID == p.selected && p.detail.Session.ID == p.selected && v.branchID == p.detail.Session.ActiveBranchID {
		return ""
	}
	message := "会话模型参数尚未读取，请重新加载后再选择或发送。草稿已保留。"
	if v.problem != "" {
		message = "会话模型参数读取失败，请重新加载后再选择或发送。草稿已保留。 " + v.problem
	}
	return message
}

func (p *NativePage) failWorkspaceSessionModelOptions(id string, generation, epoch uint64, problem string) {
	if p.selected != id || p.generation != generation || p.sessionModelOptions.epoch != epoch {
		return
	}
	p.sessionModelOptions.ready, p.sessionModelOptions.problem = false, problem
}

func (p *NativePage) acceptWorkspaceSessionModelOptions(id string, generation uint64, branch string, epoch uint64, owner string, value hub.SessionModelOptionsSnapshot) bool {
	v := &p.sessionModelOptions
	if p.selected != id || p.generation != generation || p.detail.Session.ID != id || p.detail.Session.ActiveBranchID != branch || v.epoch != epoch {
		return false
	}
	if owner != id {
		p.failWorkspaceSessionModelOptions(id, generation, epoch, "会话参数归属不匹配。")
		return false
	}
	if err := value.Validate(); err != nil {
		p.failWorkspaceSessionModelOptions(id, generation, epoch, err.Error())
		return false
	}
	if v.sessionID == id && v.value.Revision > value.Revision {
		return false
	}
	v.sessionID, v.branchID, v.value, v.ready, v.problem = id, branch, value.Clone(), true, ""
	return true
}

// Keep the options beside the entire picker. When neither side holds the
// canonical width, use the larger outside strip rather than cover model rows.
func workspaceModelOptionsWidth(viewWidth float32, picker ui.Rect) (float32, bool) {
	left := max(0, picker.X-16)
	right := max(0, viewWidth-picker.X-picker.W-16)
	if left >= 300 {
		return 300, true
	}
	if right >= 300 {
		return 300, false
	}
	if left >= right {
		return left, true
	}
	return right, false
}

func (p *NativePage) buildWorkspaceModelOptions(c *ui.Context, anchor ui.Element, row *workspaceModelRow) {
	s := &p.composer.modelPicker
	v := &s.inlineOptions
	open := s.config == row.key
	if !open {
		if s.config == "" || v.key == row.key {
			v.open = false
		}
		return
	}
	if !v.open || v.key != row.key || v.sequence != s.sequence {
		*v = workspaceModelOptionsState{key: row.key, open: true, sequence: s.sequence}
		if p.Service.Connections != nil {
			for _, connection := range p.Service.Connections.Snapshot() {
				for _, model := range connection.Models {
					if hub.ModelPreferenceKey(connection.Target(model)) == hub.ModelPreferenceKey(row.routes[0]) {
						v.vision = model.ImageCapable
					}
				}
			}
		}
	}
	if !v.effortDirty {
		v.effort = ""
		if p.workspaceSessionModelOptionsProblem() == "" {
			v.effort = p.workspaceTargetOptions(row.routes[0]).Generation.ReasoningEffort
		}
	}
	picker := c.Resolve(s.optionsPanel)
	if !picker.Valid() {
		return
	}
	width, height := c.Size()
	panelWidth, left := workspaceModelOptionsWidth(width, picker.Bounds())
	t := c.Theme()
	ui.PopoverBase(c.Key("options"), anchor, &open, func(panel ui.Element) {
		panel.Label("Model options").Width(panelWidth).MinWidth(0).MaxHeight(height - 8).
			Padding(16).Gap(0).Radius(16).Background(t.Surface).
			DrawOver(func(painter *ui.Painter, bounds ui.Rect) { painter.Stroke(bounds, t.Border, 16, 1) })
		if left {
			panel.AttachTo(picker, ui.AnchorTopLeft, ui.AnchorTopRight).Margin(0, 12, 0, 0)
		} else {
			panel.AttachTo(picker, ui.AnchorTopRight, ui.AnchorTopLeft).Margin(0, 0, 0, 12)
		}
		ui.ScrollBoth(c.Key("contents")).Label("Model options contents").FillWidth().MinWidth(0).
			MaxHeight(max(0, height-40)).Padding(0).Children(func() {
			ui.Column(c.Key("canonical-fields")).Width(268).Gap(16).Children(func() {
				ui.Row(c.Key("header")).Label("Model options header").FillWidth().MinWidth(0).Height(32).Gap(8).AlignItems(ui.Center).Children(func() {
					ui.Text(c, row.name).Label("Model options name").Grow(1).MinWidth(0).FontSize(workspaceFontSubtitle).FontWeight(600).LineHeight(1.5).SingleLine()
					favorite := workspaceComposerIconButton(c.Key("favorite"), "star", "Favorite model in options: "+row.model, "收藏模型", workspaceIconControlSize).
						Checked(row.favorite).Disabled(s.saving || p.Service.Connections == nil).
						OnClick(func() { p.toggleWorkspaceModelFavorite(row.routes[0]) })
					if row.favorite {
						favorite.TextColor(t.Accent)
					}
				})
				ui.Text(c, row.source).Label("Model options source").FillWidth().MinWidth(0).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(t.TextMuted).SingleLine()
				p.buildWorkspaceModelAgentField(c, row)
				effortTitle := "思考程度"
				if v.effort != "" && v.effort != "low" && v.effort != "medium" && v.effort != "high" {
					effortTitle += " · " + v.effort
				}
				ui.Text(c, effortTitle).Label("Model options effort title").FillWidth().FontSize(workspaceFontBody).LineHeight(1.5)
				generation := false
				if row.chosen >= 0 && row.chosen < len(row.routes) {
					generation = p.workspaceModelConfig(row.routes[row.chosen].EngineID).SupportsGeneration
				}
				ui.Row(c.Key("effort")).Label("Model effort segments").Height(36).Padding(4).Gap(4).AlignSelf(ui.Start).
					AlignItems(ui.Center).Radius(8).Background(workspacePalette(c).subtle).FocusGroup(ui.Horizontal).Children(func() {
					for _, choice := range []struct{ value, label string }{{"", "默认"}, {"low", "低"}, {"medium", "中"}, {"high", "高"}} {
						selected := v.effort == choice.value
						button := ui.ButtonBase(c.Key(choice.label)).Label("Model effort: "+choice.label).Role(ui.RoleRadio).Checked(selected).
							Height(workspaceIconControlSize).Padding(0, 8).Radius(8).AlignItems(ui.Center).Justify(ui.Center).FocusRing(false).
							Disabled(!generation || s.saving || p.Service.Connections == nil || p.workspaceSessionModelOptionsProblem() != "").
							OnClick(func() { v.effort, v.effortDirty = choice.value, true })
						color := t.Text
						if selected {
							button.Background(t.Surface)
							color = t.Accent
						}
						button.DrawOver(func(painter *ui.Painter, bounds ui.Rect) {
							if button.FocusVisible() {
								painter.Stroke(bounds, t.Accent, 8, 2)
							} else if selected {
								painter.Stroke(bounds, t.Border, 8, 1)
							}
						}).Children(func() {
							ui.Text(c, choice.label).FontSize(workspaceFontBody).LineHeight(1.5).TextColor(color).SingleLine()
						})
					}
				})
				ui.Column(c.Key("context")).Label("Model context field").FillWidth().Gap(8).Children(func() {
					ui.Text(c, "Context 上限 · token").FontSize(workspaceFontBody).LineHeight(1.5)
					unknown := "未声明"
					workspaceControlInput(c, "limit", &unknown).Label("Model context limit").FillWidth().Disabled(true)
				})
				ui.Row(c.Key("vision")).Label("Model vision setting").FillWidth().Height(40).Padding(8, 0).Gap(8).AlignItems(ui.Center).
					Tooltip("只展示模型连接已声明的视觉能力。此处不可修改。").Children(func() {
					ui.Text(c, "视觉输入").FontSize(workspaceFontBody).LineHeight(1.5).Grow(1)
					workspaceControlSwitch(c, "switch", &v.vision, true).Label("Model vision input")
				})
				workspaceControlIconButton(c, "model-settings", "模型设置", "sliders-horizontal", "ghost", s.saving || row.chosen < 0 || p.Service.Connections == nil).Label("Edit model defaults: " + row.model).
					FillWidth().OnClick(func() {
					scope := "模型默认"
					if p.selected != "" {
						scope = "当前会话"
					}
					p.openWorkspaceModelOptionScope(row.routes[row.chosen], scope)
					if v.effortDirty {
						if scope == "当前会话" {
							s.defaults.pendingEffort, s.defaults.hasPendingEffort = v.effort, true
						} else {
							s.defaults.effort = v.effort
						}
					}
					s.config = ""
				})
				scope := "模型默认 · 保存后下一轮生效"
				if p.selected != "" {
					scope = "当前会话 · 模型默认可在设置中切换"
				}
				ui.Text(c, scope).Label("Model options scope").FillWidth().FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(t.TextMuted)
				if problem := p.workspaceSessionModelOptionsProblem(); problem != "" {
					ui.Text(c, problem).Label("Session model options observation error").FillWidth().FontSize(workspaceFontCaption).TextColor(t.Danger)
					workspaceControlButton(c, "retry-session-options", "重新加载会话参数", "secondary", p.polling || p.busy).Label("Reload observed session model options").
						FillWidth().OnClick(p.loadTail)
				}
			})
		})
	})
	if !open && s.config == row.key {
		s.config, v.open = "", false
	}
}

func (p *NativePage) buildWorkspaceModelAgentField(c *ui.Context, row *workspaceModelRow) {
	s := &p.composer.modelPicker
	t := c.Theme()
	ui.Column(c.Key("agent-field")).Label("Model Agent field").FillWidth().Gap(8).Children(func() {
		ui.Text(c, "Agent").FontSize(workspaceFontBody).LineHeight(1.5)
		selected, value := "", "选择 Agent"
		if row.chosen >= 0 {
			selected, value = hub.RouteKey(row.routes[row.chosen]), strings.ToUpper(row.routes[row.chosen].EngineID)
		}
		parts := ui.SelectBase(c.Key("agent-select"), &selected)
		trigger := parts.Trigger.Label("Model Agent").FillWidth().Height(workspaceControlHeight).Padding(0, 12).Gap(8).Radius(8).
			AlignItems(ui.Center).Justify(ui.SpaceBetween).Background(t.Surface).TextColor(t.Text).FocusRing(false).
			Disabled(s.saving).OnChange(func() {
			for i, route := range row.routes {
				if hub.RouteKey(route) == selected {
					row.chosen = i
					break
				}
			}
		})
		workspaceControlInputBorder(trigger, t, workspacePalette(c).controlBorder)
		trigger.Children(func() {
			ui.Text(c, value).Grow(1).MinWidth(0).FontSize(workspaceFontBody).LineHeight(1.5).SingleLine()
			ui.Icon(c, workspaceIcon("chevron-down")).Size(16, 16).Shrink(0).TextColor(t.TextMuted)
		})
		parts.Popup(func(panel ui.Element) {
			panel.Label("Compatible agents").Margin(4, 0, 0).Padding(4).Radius(8).Background(t.Surface).
				DrawOver(func(painter *ui.Painter, bounds ui.Rect) { painter.Stroke(bounds, t.Border, 8, 1) })
			ui.Scroll(c).FillWidth().MaxHeight(216).Children(func() {
				for _, route := range row.routes {
					item := parts.Item(hub.RouteKey(route)).Label("Agent option: "+route.EngineID+" · "+row.model).
						FillWidth().Height(workspaceControlHeight).Padding(0, 12).Radius(8).AlignItems(ui.Center).TextColor(t.Text)
					if item.Highlighted() {
						item.Background(workspacePalette(c).selection)
					}
					item.Children(func() {
						ui.Text(c, strings.ToUpper(route.EngineID)).FontSize(workspaceFontBody).LineHeight(1.5).SingleLine()
					})
				}
			})
		})
	})
}
