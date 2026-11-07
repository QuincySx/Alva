// INPUT: Canonical Alva Foundation Button, Input, Select and Checkbox definitions and shared theme roles.
// OUTPUT: Native controls with the normal-density 32px geometry, typography, variants and Lucide assets.
// POS: [L7 Hub UI] Internal presentation adapters retain MyGo input, focus and accessibility behavior.
//go:build desktop

package hubdesktop

import (
	"time"

	"github.com/egoist/mygo/ui"
)

// These values map cmp_alva_button / cmp_alva_input, rather than MyGo's
// density-dependent defaults. Colors remain the committed Alva theme roles.
func workspaceControlButton(c *ui.Context, key, text, kind string, disabled ...bool) ui.Element {
	return workspaceControlIconButton(c, key, text, "", kind, disabled...)
}

func workspaceControlIconButton(c *ui.Context, key, text, icon, kind string, disabled ...bool) ui.Element {
	return workspaceControlButtonState(c, key, text, icon, kind, false, disabled...)
}

// Busy actions stay focusable and keep their name. The callback is gated here;
// service handlers still own admission and reject duplicate in-flight work.
func workspaceControlAction(c *ui.Context, key, text, icon, kind string, busy bool, action func(), disabled ...bool) ui.Element {
	return workspaceControlActionMotion(c, key, text, icon, kind, busy, c.Preferences().ReduceMotion, action, disabled...)
}
func (p *NativePage) workspaceControlAction(c *ui.Context, key, text, icon, kind string, busy bool, action func(), disabled ...bool) ui.Element {
	return workspaceControlActionMotion(c, key, text, icon, kind, busy, p.reducedMotion(c), action, disabled...)
}
func workspaceControlActionMotion(c *ui.Context, key, text, icon, kind string, busy, reduced bool, action func(), disabled ...bool) ui.Element {
	return workspaceControlButtonMotion(c, key, text, icon, kind, busy, reduced, disabled...).OnClick(func() {
		if !busy {
			action()
		}
	})
}

func workspaceControlButtonState(c *ui.Context, key, text, icon, kind string, busy bool, disabled ...bool) ui.Element {
	return workspaceControlButtonMotion(c, key, text, icon, kind, busy, c.Preferences().ReduceMotion, disabled...)
}
func workspaceControlButtonMotion(c *ui.Context, key, text, icon, kind string, busy, reduced bool, disabled ...bool) ui.Element {
	t := c.Theme()
	palette := workspacePalette(c)
	fill, color := t.Surface, t.Text
	border := t.Border
	switch kind {
	case "primary":
		fill, color = t.Text, t.Surface
		border = fill
	case "ghost":
		fill, color = ui.Transparent, t.TextMuted
		border = ui.Transparent
	case "danger":
		fill, color = ui.Transparent, t.Danger
		border = ui.Transparent
	default:
		if busy {
			fill, border = palette.subtle, palette.subtle
		}
	}
	b := ui.ButtonBase(c.Key(key)).Label(text).Height(workspaceControlHeight).Padding(0, 8).Gap(8).
		AlignItems(ui.Center).Justify(ui.Center).AlignSelf(ui.Start).Radius(8).
		Disabled(!busy && len(disabled) != 0 && disabled[0]).Background(fill).TextColor(color).FontSize(workspaceFontBody).FontWeight(400).LineHeight(1.5).FocusRing(false)
	// Disabled intent arrives before the children are built. MyGo owns input,
	// focus, accessibility and its native 0.5 disabled subtree opacity.
	if b.IsDisabled() {
		fill, border, color = palette.subtle, palette.subtle, t.TextMuted
		if kind == "ghost" {
			fill, border = ui.Transparent, ui.Transparent
		}
		b.Background(fill)
	}
	switch {
	case !busy && !b.IsDisabled() && b.Pressed():
		if kind == "primary" {
			b.Opacity(.96)
		} else {
			fill = palette.hover
			if kind == "danger" {
				fill, color = t.Danger, t.Surface
			} else if kind == "ghost" {
				color = t.Text
			}
			if kind != "ghost" && kind != "danger" {
				border = fill
			}
			b.Background(fill)
		}
	case !busy && !b.IsDisabled() && b.Hovered():
		if kind == "primary" {
			b.Opacity(.98)
		} else {
			fill = palette.hover
			if kind == "ghost" {
				fill = palette.subtle
			} else if kind == "danger" {
				fill = palette.dangerBg
			}
			if kind != "ghost" && kind != "danger" {
				border = fill
			}
			b.Background(fill)
		}
	}
	b.TextColor(color)
	if busy {
		b.Description("正在处理，请等待完成。")
	}
	// Exit transitions can retain this painting after its build pass ends.
	// Keep immutable paint state, not a frame-scoped Element, in the callback.
	focused := !b.IsDisabled() && b.FocusVisible()
	b.DrawOver(func(p *ui.Painter, r ui.Rect) {
		if focused {
			p.Stroke(r, t.Surface, 8, 4)
			p.Stroke(r, t.Accent, 8, 2)
		} else {
			p.Stroke(r, border, 8, 1)
		}
	})
	b.Children(func() {
		if icon != "" {
			if busy {
				workspaceLoadingIconMotion(c.Key("activity"), workspaceIconSmall, color, reduced)
			} else {
				ui.Icon(c, workspaceIcon(icon)).Size(workspaceIconSmall, workspaceIconSmall).Shrink(0)
			}
		}
		if text != "" {
			label := ui.Text(c, text).FontSize(workspaceFontBody).FontWeight(400).LineHeight(1.5).SingleLine()
			if busy && icon == "" {
				label.Opacity(0)
				ui.Box(c.Key("loading-overlay")).Absolute().Top(0).Right(0).Bottom(0).Left(0).Center().Children(func() {
					workspaceLoadingIconMotion(c.Key("activity"), workspaceIconSmall, color, reduced)
				})
			}
		}
	})
	return b
}

func workspaceLoadingIcon(c *ui.Context, size float32, color ui.Color) ui.Element {
	return workspaceLoadingIconMotion(c, size, color, c.Preferences().ReduceMotion)
}
func (p *NativePage) reducedMotion(c *ui.Context) bool {
	return c.Preferences().ReduceMotion || p.Service != nil && p.Service.Preferences != nil && p.Service.Preferences.Snapshot().ReducedMotion
}
func (p *NativePage) workspaceLoadingIcon(c *ui.Context, size float32, color ui.Color) ui.Element {
	return workspaceLoadingIconMotion(c, size, color, p.reducedMotion(c))
}
func workspaceLoadingIconMotion(c *ui.Context, size float32, color ui.Color, reduced bool) ui.Element {
	svg := workspaceIcon("loader-circle")
	return ui.Box(c).Label("Loading indicator").Role(ui.RoleProgress).Size(size, size).Shrink(0).
		Draw(func(p *ui.Painter, r ui.Rect) {
			if reduced {
				p.Icon(svg, r, color)
				return
			}
			// Paint-only rotation shares the unrotated SVG mask. It neither
			// rebuilds the page nor creates a timer/goroutine per control.
			angle := float32(p.Now().UnixMilli()%840) * 360 / 840
			p.Transform(ui.Rotation(angle), r.X+r.W/2, r.Y+r.H/2, func() { p.Icon(svg, r, color) })
			p.After(35 * time.Millisecond)
		})
}

func workspaceControlInput(c *ui.Context, key string, value *string) ui.Element {
	t := c.Theme()
	e := ui.TextInputBase(c.Key(key), value).Height(workspaceControlHeight).Padding(0, 8).Radius(8).
		FontSize(workspaceFontBody).FontWeight(400).LineHeight(1.5).Background(workspacePalette(c).subtle).
		TextColor(t.Text).FocusRing(false)
	if e.Focused() {
		e.Background(t.Surface)
	}
	workspaceControlInputBorder(e, t, t.Border)
	return e
}

// workspaceControlSwitch paints the Foundation Alva Switch while SwitchBase
// retains native keyboard, checked-state and accessibility semantics.
func workspaceControlSwitch(c *ui.Context, key string, value *bool, disabled bool) ui.Element {
	t := c.Theme()
	b := ui.SwitchBase(c.Key(key), value).Size(38, 24).Radius(12).Disabled(disabled).FocusRing(false)
	b.Draw(func(p *ui.Painter, r ui.Rect) {
		track, thumb, stroke := workspacePalette(c).subtle, t.TextMuted, t.Border
		x := r.X + 4
		if *value {
			x = r.X + 18
			if !disabled {
				track, thumb, stroke = t.Accent, t.AccentText, t.Accent
			}
		}
		p.Fill(r, track, 12)
		p.Stroke(r, stroke, 12, 1)
		p.Fill(ui.Rect{X: x, Y: r.Y + 4, W: 16, H: 16}, thumb, 8)
		if b.FocusVisible() {
			p.Stroke(r, t.Surface, 12, 4)
			p.Stroke(r, t.Accent, 12, 2)
		}
	})
	return b
}

// Password uses Input.Kind=Password's two 16px slots. Revealing is an
// explicit page-local choice and only ever displays the supplied draft.
func workspaceControlPassword(c *ui.Context, key string, value *string, reveal *bool, disabled bool) ui.Element {
	t := c.Theme()
	var input ui.Element
	container := ui.Row(c.Key(key+"-control")).Label("API Key password control").FillWidth().Height(workspaceControlHeight).Padding(0, 8).Gap(8).
		AlignItems(ui.Center).Radius(8).Background(workspacePalette(c).subtle).Disabled(disabled).DrawOver(func(p *ui.Painter, r ui.Rect) {
		color, width := t.Border, float32(1)
		if input.Focused() {
			color, width = t.Accent, 2
		}
		p.Stroke(r, color, 8, width)
	}).Children(func() {
		ui.Icon(c, workspaceIcon("key-round")).Label("API Key leading icon").Size(workspaceIconSmall, workspaceIconSmall).Shrink(0).TextColor(t.TextMuted)
		input = ui.TextInputBase(c.Key(key), value).Grow(1).MinWidth(0).Height(workspaceControlHeight).Padding(0).
			FontSize(workspaceFontBody).FontWeight(400).LineHeight(1.5).TextColor(t.Text).FocusRing(false)
		if !*reveal {
			input.Password()
		}
		ui.ButtonBase(c.Key(key + "-reveal")).Label("Toggle API Key visibility").Tooltip("显示或隐藏本窗 Key 草稿").
			Width(workspaceIconSmall).Height(workspaceControlHeight).Padding(0).AlignItems(ui.Center).Justify(ui.Center).TextColor(t.TextMuted).
			OnClick(func() { *reveal = !*reveal }).Children(func() {
			ui.Icon(c, workspaceIcon("eye")).Label("API Key reveal icon").Size(workspaceIconSmall, workspaceIconSmall).Shrink(0)
		})
	})
	if input.Focused() {
		container.Background(t.Surface)
	}
	return input
}

func workspaceControlTextArea(c *ui.Context, key string, value *string) ui.Element {
	t := c.Theme()
	e := ui.TextAreaBase(c.Key(key), value).Padding(12).Radius(8).
		FontSize(workspaceFontBody).FontWeight(400).LineHeight(1.5).Background(workspacePalette(c).subtle).
		TextColor(t.Text).FocusRing(false)
	if e.Focused() {
		e.Background(t.Surface)
	}
	workspaceControlInputBorder(e, t, t.Border)
	return e
}

func workspaceControlInputBorder(e ui.Element, t *ui.Theme, border ui.Color) {
	e.DrawOver(func(p *ui.Painter, r ui.Rect) {
		if e.Focused() {
			p.Stroke(r, t.Accent, 8, 2)
		} else {
			p.Stroke(r, border, 8, 1)
		}
	})
}

func workspaceControlSelect(c *ui.Context, key string, value *string, options []string) ui.Element {
	return workspaceControlSelectAppearance(c, key, value, options, false)
}

// SettingRow uses a compact value trigger; the same native popup/keyboard
// implementation serves ordinary bordered inputs and borderless value rows.
func workspaceControlValueSelect(c *ui.Context, key string, value *string, options []string) ui.Element {
	return workspaceControlSelectAppearance(c, key, value, options, true)
}

func workspaceControlSelectAppearance(c *ui.Context, key string, value *string, options []string, plain bool) ui.Element {
	t := c.Theme()
	selectParts := ui.SelectBase(c.Key(key), value)
	b := selectParts.Trigger.Height(workspaceControlHeight).Padding(0, 8).Gap(8).Radius(8).
		AlignItems(ui.Center).Justify(ui.SpaceBetween).FontSize(workspaceFontBody).FontWeight(400).LineHeight(1.5).
		Background(workspacePalette(c).subtle).TextColor(t.Text).FocusRing(false)
	if plain {
		b.Background(ui.Transparent).TextColor(t.TextMuted).DrawOver(func(p *ui.Painter, r ui.Rect) {
			if b.FocusVisible() {
				p.Stroke(r, t.Surface, 8, 4)
				p.Stroke(r, t.Accent, 8, 2)
			}
		})
	} else {
		if b.Focused() {
			b.Background(t.Surface)
		}
		workspaceControlInputBorder(b, t, t.Border)
	}
	b.Children(func() {
		ui.Text(c, *value).Grow(1).MinWidth(0).SingleLine().FontSize(workspaceFontBody).LineHeight(1.5)
		ui.Icon(c, workspaceIcon("chevron-down")).Size(workspaceIconSmall, workspaceIconSmall).Shrink(0).TextColor(t.TextMuted)
	})
	selectParts.Popup(func(panel ui.Element) {
		panel.Margin(4, 0, 0).Padding(4).Radius(8).Background(t.Surface).Border(1, t.Border)
		for _, option := range options {
			item := selectParts.Item(option).Label(option).Padding(4, 8).Gap(8).MinHeight(workspaceControlHeight).Radius(8).
				AlignItems(ui.Center).TextColor(t.Text)
			if item.Highlighted() {
				item.Background(workspacePalette(c).selection)
			} else if item.Hovered() {
				item.Background(workspacePalette(c).hover)
			}
			item.Children(func() { ui.Text(c, option).FontSize(workspaceFontBody).LineHeight(1.5).SingleLine() })
		}
	})
	return b
}

func workspaceControlCheckbox(c *ui.Context, key string, value *bool, text string) ui.Element {
	t := c.Theme()
	b := ui.CheckboxBase(c.Key(key), value).Label(text).Height(32).Gap(8).AlignItems(ui.Center)
	b.Children(func() {
		mark := ui.Box(c).Size(18, 18).Shrink(0).Radius(4).Background(t.Surface).
			Border(1, workspacePalette(c).controlBorder).AlignItems(ui.Center).Justify(ui.Center)
		if *value {
			mark.Background(t.Accent).Border(1, t.Accent).Children(func() {
				ui.Icon(c, workspaceIcon("check")).Size(workspaceIconSmall, workspaceIconSmall).TextColor(t.AccentText)
			})
		}
		ui.Text(c, text).FontSize(workspaceFontBody).LineHeight(1.5).SingleLine().MinWidth(0)
	})
	return b
}
