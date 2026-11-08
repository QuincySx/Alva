// INPUT: Shared committed preference owner, page-local drafts and MyGo native controls/themes.
// OUTPUT: Session-free General settings, explicit Save/Reload and application appearance projection.
// POS: [L7 Hub UI] Edits presentation preferences only; no SQL, validation policy or execution ownership.
//go:build desktop

package hubdesktop

import (
	"context"
	"errors"
	"strconv"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

type preferencesPane struct {
	base                 hub.Preferences
	theme, density, size string
	reducedMotion        bool
	loading, saving      bool
	readFailed           bool
	epoch                uint64
	problem              string
}

func (p *preferencesPane) dirty() bool {
	return p.theme != p.base.Theme || p.density != p.base.Density || p.size != strconv.Itoa(p.base.FontSize) || p.reducedMotion != p.base.ReducedMotion
}
func (p *preferencesPane) accept(value hub.Preferences) {
	p.base = value
	p.theme = value.Theme
	p.density = value.Density
	p.size = strconv.Itoa(value.FontSize)
	p.reducedMotion = value.ReducedMotion
}

// restoreDefaults changes only the draft; Save commits it like any edit.
// Density keeps its hidden saved value because it has no visible control.
func (p *preferencesPane) restoreDefaults() {
	defaults := hub.DefaultPreferences()
	p.theme = defaults.Theme
	p.size = strconv.Itoa(defaults.FontSize)
	p.reducedMotion = defaults.ReducedMotion
	p.problem = ""
}
func (p *NativePage) openPreferences() {
	p.setPane("preferences")
	if !p.preferences.dirty() || p.preferences.base.Revision == 0 {
		p.reloadPreferences()
	}
}
func (p *NativePage) reloadPreferences() {
	if p.Service.Preferences == nil || p.preferences.saving || p.preferences.loading {
		return
	}
	p.preferences.loading = true
	p.preferences.epoch++
	epoch := p.preferences.epoch
	go func() {
		value, err := p.Service.Preferences.Read(p.ctx)
		p.dispatch(func() {
			if p.preferences.epoch != epoch {
				return
			}
			p.preferences.loading = false
			if err != nil {
				p.preferences.readFailed = true
				p.preferences.problem = "Could not read saved preferences. Reload to retry."
				return
			}
			p.preferences.accept(value)
			p.preferences.readFailed = false
			p.preferences.problem = ""
		})
	}()
}
func (p *NativePage) savePreferences() {
	v := &p.preferences
	if p.Service.Preferences == nil || v.saving || v.loading || v.readFailed || v.base.Revision == 0 || !v.dirty() {
		return
	}
	size, err := strconv.Atoi(v.size)
	if err != nil {
		v.problem = "Choose a supported body size."
		return
	}
	theme, density := v.theme, v.density
	update := hub.PreferenceUpdate{ExpectedRevision: v.base.Revision, Theme: &theme, Density: &density, FontSize: &size}
	reducedMotion := v.reducedMotion
	update.ReducedMotion = &reducedMotion
	v.saving = true
	v.epoch++
	epoch := v.epoch
	go func() {
		value, err := p.Service.Preferences.Update(p.ctx, update)
		p.dispatch(func() {
			if v.epoch != epoch {
				return
			}
			v.saving = false
			if err != nil {
				switch {
				case errors.Is(err, hub.ErrPreferenceConflict):
					v.problem = "Preferences changed in another window or client. Your draft is retained; reload before saving."
				case errors.Is(err, context.Canceled):
					v.problem = "Save canceled. Your draft is retained."
				default:
					v.problem = "Could not save preferences. Your draft is retained; retry Save or reload."
				}
				return
			}
			v.accept(value)
			v.problem = ""
		})
	}()
}

// Keep the window's context and save owner alive until storage actually returns.
func (p *NativePage) guardPreferencesClose(event *mygo.CloseEvent) bool {
	if !p.preferences.saving {
		return false
	}
	event.PreventDefault()
	p.setPane("preferences")
	p.preferences.problem = "Preferences are still saving. Wait for storage to finish before closing this window."
	return true
}

func preferenceValueLabel(value string) string {
	switch value {
	case "system":
		return "跟随系统"
	case "light":
		return "浅色"
	case "dark":
		return "深色"
	case "comfortable":
		return "舒适"
	case "compact":
		return "紧凑"
	case "14", "16":
		return value + " px"
	default:
		return value
	}
}

func preferenceChoice(c *ui.Context, key, label, accessibleLabel string, value *string, options []string, disabled bool, unavailableReason string) {
	display := preferenceValueLabel(*value)
	choices := make([]string, len(options))
	for i, option := range options {
		choices[i] = preferenceValueLabel(option)
	}
	border := c.Theme().Border
	row := ui.Row(c).FillWidth().AlignItems(ui.Center).Padding(8, 0).Gap(8).
		DrawOver(func(painter *ui.Painter, r ui.Rect) {
			painter.Fill(ui.Rect{X: r.X, Y: r.Y + r.H - 1, W: r.W, H: 1}, border, 0)
		})
	if unavailableReason != "" {
		row.Label(accessibleLabel + " unavailable").Focusable().Tooltip(unavailableReason)
	}
	row.Children(func() {
		ui.Text(c, label).Label(accessibleLabel).FontSize(workspaceFontBody).LineHeight(1.5).Grow(1).MinWidth(0)
		choice := workspaceControlValueSelect(c, "preference-"+key, &display, choices).
			Label("Select "+accessibleLabel).MinWidth(0).Border(0, ui.Transparent).
			Background(ui.Transparent).TextColor(c.Theme().TextMuted).Disabled(disabled || unavailableReason != "").
			OnChange(func() {
				for i, option := range options {
					if display == choices[i] {
						*value = option
						return
					}
				}
			})
		if choice.Hovered() {
			choice.Background(c.Theme().SurfaceHover)
		}
	})
}

func preferenceProblemText(problem string) string {
	switch problem {
	case "Could not read saved preferences. Reload to retry.":
		return "无法读取已保存的偏好，请重新加载后重试。"
	case "Choose a supported body size.":
		return "请选择受支持的正文大小。"
	case "Preferences changed in another window or client. Your draft is retained; reload before saving.":
		return "偏好已在其他窗口或客户端更改。草稿已保留，请重新加载后再保存。"
	case "Save canceled. Your draft is retained.":
		return "保存已取消，草稿已保留。"
	case "Could not save preferences. Your draft is retained; retry Save or reload.":
		return "无法保存偏好，草稿已保留。请重试保存或重新加载。"
	case "Preferences are still saving. Wait for storage to finish before closing this window.":
		return "偏好仍在保存，请等待存储完成后再关闭窗口。"
	default:
		return problem
	}
}

func (p *NativePage) buildPreferences(c *ui.Context) {
	v := &p.preferences
	t := c.Theme()
	ui.Text(c, "通用").Label("General page title").FontSize(workspaceFontHeading).FontWeight(600).LineHeight(1.5)
	ui.Scroll(c).Grow(1).FillWidth().MinWidth(0).Children(func() {
		ui.Column(c).FillWidth().Gap(16).Children(func() {
			ui.Column(c).Label("Preferences appearance").FillWidth().Padding(0).Gap(12).Children(func() {
				ui.Text(c, "外观").FontSize(workspaceFontSubtitle).FontWeight(600).LineHeight(1.5)
				ui.Column(c).FillWidth().Gap(0).Children(func() {
					options := hub.PreferenceChoices()
					sizes := make([]string, len(options.FontSizes))
					for i, n := range options.FontSizes {
						sizes[i] = strconv.Itoa(n)
					}
					disabled := v.base.Revision == 0 || v.loading || v.saving
					preferenceChoice(c, "theme", "颜色主题", "Color theme", &v.theme, options.Themes, disabled, "")
					preferenceChoice(c, "size", "正文大小", "Body size", &v.size, sizes, disabled, "")
					ui.Row(c).FillWidth().AlignItems(ui.Center).Padding(8, 0).Gap(8).Children(func() {
						ui.Text(c, "减弱动效").FontSize(workspaceFontBody).LineHeight(1.5).Grow(1).MinWidth(0)
						workspaceControlSwitch(c, "preference-reduced-motion", &v.reducedMotion, disabled).Label("Reduce application motion").Tooltip("系统要求减弱动效时始终遵循系统。")
					})
				})
			})
			ui.Column(c).FillWidth().Gap(8).Children(func() {
				if v.loading {
					ui.Text(c, "正在加载偏好…").Label("Loading preferences…").Role(ui.RoleStatus).FontSize(workspaceFontBody).LineHeight(1.5).TextColor(t.TextMuted)
				}
				problem := v.problem
				if problem == "" && p.Service.Preferences != nil && v.base.Revision != 0 && v.base.Revision != p.Service.Preferences.Snapshot().Revision && v.dirty() {
					problem = "Preferences changed in another window or client. Your draft is retained; reload before saving."
				}
				if problem != "" {
					ui.Text(c, preferenceProblemText(problem)).Label(problem).Role(ui.RoleStatus).FontSize(workspaceFontBody).LineHeight(1.5).TextColor(t.Danger)
				}
				ui.Row(c).FillWidth().Gap(8).Justify(ui.End).Children(func() {
					workspaceControlButton(c, "preference-defaults", "恢复默认", "ghost", v.base.Revision == 0 || v.loading || v.saving).Label("Restore default preferences").
						Tooltip("把草稿改回默认外观，点保存后生效").OnClick(v.restoreDefaults)
					workspaceControlButton(c, "preference-reload", "重新加载", "secondary", v.loading || v.saving).Label("Reload preferences").OnClick(p.reloadPreferences)
					p.workspaceControlAction(c, "preference-save", "保存偏好", "", "primary", v.saving, p.savePreferences, !v.saving && (v.base.Revision == 0 || v.loading || v.readFailed || !v.dirty())).Label("Save preferences")
				})
			})
		})
	})
}

func (p *NativePage) observePreferences() {
	if p.Service.Preferences == nil {
		return
	}
	p.observeWorkspaceLayoutPreferences()
	if !p.preferences.loading && !p.preferences.saving && (p.preferences.base.Revision == 0 || !p.preferences.dirty()) {
		p.preferences.accept(p.Service.Preferences.Snapshot())
	}
}

func (p *NativePage) applyPreferences(c *ui.Context) {
	value := hub.DefaultPreferences()
	if p.Service.Preferences != nil {
		value = p.Service.Preferences.Snapshot()
	}
	dark := c.Theme().Dark
	if value.Theme != "system" {
		dark = value.Theme == "dark"
	}
	t := ui.LightTheme()
	if dark {
		t = ui.DarkTheme()
	}
	if dark {
		t.Background = ui.Hex("#14161B")
		t.Surface = ui.Hex("#1C1F26")
		t.SurfaceHover = ui.Hex("#252932")
		t.SurfacePressed = ui.Hex("#2C313C")
		t.Text = ui.Hex("#EAECF3")
		t.TextMuted = ui.Hex("#A6ADBA")
		t.Border = ui.Hex("#303640")
		t.Accent = ui.Hex("#AAA1FF")
		t.AccentText = ui.Hex("#231F46")
		t.Danger = ui.Hex("#E6A6B2")
		t.Warning = ui.Hex("#E4C596")
		t.Success = ui.Hex("#A0CDB3")
	} else {
		t.Background = ui.Hex("#F3F4F7")
		t.Surface = ui.Hex("#FFFFFF")
		t.SurfaceHover = ui.Hex("#F5F6F8")
		t.SurfacePressed = ui.Hex("#E9EBF0")
		t.Text = ui.Hex("#272A34")
		t.TextMuted = ui.Hex("#626B7A")
		t.Border = ui.Hex("#E2E5EB")
		t.Accent = ui.Hex("#6555CD")
		t.AccentText = ui.Hex("#FFFFFF")
		t.Danger = ui.Hex("#AC4050")
		t.Warning = ui.Hex("#856029")
		t.Success = ui.Hex("#32704F")
	}
	t.Font = "Alva UI Sans SC"
	t.AccentHover = t.Accent.Mix(t.Text, .08)
	t.AccentPressed = t.Accent.Mix(t.Text, .16)
	t.Focus = t.Accent
	t.Inverse = t.Surface
	t.InverseText = t.Text
	t.Selection = t.Accent.Alpha(.2)
	t.Radius = 8
	// One normal-density control scale; reading font size remains an explicit preference.
	t.Spacing = 4
	system := c.Preferences()
	if value.ReducedMotion || system.ReduceMotion {
		c.DisableAnimations()
	}
	scale := system.TextScale
	if scale <= 0 {
		scale = 1
	}
	t.FontSize = float32(value.FontSize) * scale
	if system.HighContrast {
		t.Border = t.Border.Mix(t.Text, .45)
		t.TextMuted = t.TextMuted.Mix(t.Text, .4)
	}
	c.SetTheme(t)
}
