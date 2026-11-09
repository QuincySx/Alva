// INPUT: Alva MenuItem (Default/Hover/Selected/Disabled/Danger/Focus/SelectedFocus/SelectedDisabled), the popover recipe, MyGo PopoverBase and FocusGroup.
// OUTPUT: Menu: a popover menu with roving focus (Up/Down/Home/End), type-ahead, Escape close, focus return and disabled-item skipping.
// POS: [L7 Kit] MyGo 0.7.4 has no MenuBase; items are ButtonBase rows in a vertical FocusGroup, so Tab and arrows share one stop. Its own PopoverMenu is the documented fallback if this regresses.
//go:build desktop

package kit

import (
	"strings"
	"time"

	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/audit"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/force"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

// MenuItem is one row of a Menu. Key identifies it in Menu's result.
type MenuItem struct {
	Key, Label, Description, Shortcut string
	Icon                              icon.Name
	Selected, Danger, Disabled        bool
	// Separator draws a divider instead of an item; the other fields are ignored.
	Separator bool
}

// MenuProps configure Menu.
type MenuProps struct {
	// Label is the accessible name of the menu.
	Label string
	// Width fixes the panel width; zero uses the design minimum.
	Width float32
}

// typeAheadReset is how long typed letters keep extending the match.
const typeAheadReset = time.Second

type menuState struct {
	handles []ui.Handle
	trigger ui.Handle
	typed   string
	last    time.Time
	// wasOpen and focusInside remember the previous open pass; returnFocus is set only by Escape, Tab or
	// choosing an item, so a press outside that moved the focus elsewhere is never overridden.
	wasOpen, focusInside, returnFocus bool
}

// typeAhead returns the index of the next enabled item whose label starts with the typed text, after
// current when one character repeats. It works on runes, never bytes.
func typeAhead(items []MenuItem, typed string, current int) int {
	r := []rune(strings.ToLower(typed))
	if len(r) == 0 {
		return -1
	}
	start, prefix, repeated := 0, string(r), true
	for _, x := range r {
		repeated = repeated && x == r[0]
	}
	if repeated {
		start, prefix = current+1, string(r[0])
	}
	for i := range items {
		idx := (start + i) % len(items)
		it := items[idx]
		if !it.Separator && !it.Disabled && strings.HasPrefix(strings.ToLower(it.Label), prefix) {
			return idx
		}
	}
	return -1
}

// Menu shows items in a popover below anchor while *open is true and returns the Key of the item
// chosen this frame ("" otherwise). The menu closes itself when an item is chosen. Escape, Tab (which
// is consumed) and choosing return the focus to the trigger; a press outside closes the menu without
// touching the focus, which belongs to whatever was pressed. State lives with the anchor element, so
// it disappears with it.
func Menu(c *ui.Context, anchor ui.Element, open *bool, items []MenuItem, p MenuProps) (chosen string) {
	audit.Named("Menu", p.Label)
	pl := pal(c)
	m := ui.Local(anchor, "kit-menu", func() menuState { return menuState{} })
	if len(m.handles) != len(items) {
		m.handles = make([]ui.Handle, len(items))
	}
	anchor.Bind(&m.trigger)
	inside := false
	ui.PopoverBase(c, anchor, open, func(panel ui.Element) {
		popupSurface(panel, pl).Margin(theme.Space1, 0, 0).Role(ui.RoleMenu).Label(p.Label).FocusGroup(ui.Vertical)
		if p.Width > 0 {
			panel.Width(p.Width)
		} else {
			panel.MinWidth(menuMinWidth)
		}
		first := -1
		reserve := false
		for i, it := range items {
			reserve = reserve || it.Icon != ""
			if it.Separator || it.Disabled || (first >= 0 && !it.Selected) {
				continue
			}
			if first < 0 || it.Selected {
				first = i
			}
		}
		for i, it := range items {
			if it.Separator {
				ui.Box(c.Key(i)).FillWidth().Height(1).Background(col(pl.Border)).Shrink(0)
				continue
			}
			audit.Named("MenuItem", it.Label)
			row := ui.ButtonBase(c.Key(i)).Label(it.Label).Role(ui.RoleMenuItem).FillWidth().MinHeight(theme.ControlRow).
				Padding(theme.Space1, theme.Space2).Gap(theme.Space2).Radius(theme.RadiusCtrl).AlignItems(ui.Center).FocusRing(false).
				Disabled(it.Disabled).Bind(&m.handles[i])
			if it.Description != "" {
				row.Description(it.Description)
			}
			if i == first && !m.wasOpen {
				row.AutoFocus()
			}
			inside = inside || row.Focused()
			if fill, ok := menuRowFill(pl, it, row.Pressed(), row.Hovered()); ok {
				row.Background(fill)
			}
			row.TextColor(menuRowText(pl, it))
			focused := !it.Disabled && row.FocusVisible()
			accent := col(pl.Accent)
			row.DrawOver(func(pt *ui.Painter, r ui.Rect) {
				if focused {
					inlineFocus(pt, r, theme.RadiusCtrl, accent)
				}
			})
			index := i
			row.HandleInput(func(ev ui.InputEvent) bool {
				if ev.Kind == ui.InputKeyDown {
					switch ev.Key {
					case ui.KeyEscape:
						m.returnFocus = true // the popover closes itself on Escape
					case ui.KeyTab:
						m.returnFocus = true
						*open = false
						return true
					}
					return false
				}
				if ev.Kind != ui.InputText || ev.Text == "" || ev.Mods&(ui.Cmd|ui.Ctrl|ui.Alt) != 0 {
					return false
				}
				if ev.Time.Sub(m.last) > typeAheadReset {
					m.typed = ""
				}
				m.typed, m.last = m.typed+ev.Text, ev.Time
				target := typeAhead(items, m.typed, index)
				if target < 0 && m.typed != ev.Text {
					// The extended text matches nothing: start over with the letter just typed.
					m.typed = ev.Text
					target = typeAhead(items, m.typed, index)
				}
				if target >= 0 {
					m.handles[target].Focus(c)
				}
				return true
			})
			row.Children(func() { menuRowContent(c, it, reserve) })
			if row.Clicked() {
				chosen = it.Key
				m.returnFocus = true
				*open = false
			}
		}
	})
	switch {
	case *open:
		m.wasOpen, m.focusInside = true, inside
	case m.wasOpen:
		// Closed this pass: return the focus only for Escape, Tab or a choice.
		m.wasOpen = false
		if m.returnFocus && m.focusInside {
			m.trigger.Focus(c)
		}
		m.focusInside, m.returnFocus, m.typed = false, false, ""
	}
	return chosen
}

// inlineFocus paints the focus indicator of rows and inline actions: one 2px accent stroke inside the
// box, which is the design's Focus stroke for list rows (the surface gap of focusRing is clipped there).
func inlineFocus(p *ui.Painter, r ui.Rect, radius float32, accent ui.Color) {
	p.Stroke(r, accent, radius, theme.BorderFocus)
}

// menuRowFill is the row background of an item state: Selected and pressed use the hover role, hover the subtle one.
func menuRowFill(pl theme.Palette, it MenuItem, pressed, hovered bool) (fill ui.Color, ok bool) {
	switch {
	case it.Disabled && !it.Selected:
	case pressed && !it.Disabled, it.Selected:
		fill, ok = col(pl.Hover), true
	case hovered && !it.Disabled:
		fill, ok = col(pl.Subtle), true
	}
	return
}

func menuRowText(pl theme.Palette, it MenuItem) ui.Color {
	if it.Danger {
		return col(pl.Danger)
	}
	return col(pl.Text)
}

// menuRowContent builds icon slot, label/description column and shortcut of one row.
func menuRowContent(c *ui.Context, it MenuItem, reserve bool) {
	if it.Icon != "" {
		Icon(c, it.Icon, IconProps{Small: true})
	} else if reserve {
		ui.Box(c.Key("icon-slot")).Size(theme.IconSmall, theme.IconSmall).Shrink(0)
	}
	ui.Column(c.Key("copy")).Grow(1).MinWidth(0).Children(func() {
		Text(c, it.Label, TextProps{SingleLine: true, Key: "label"})
		if it.Description != "" {
			Text(c, it.Description, TextProps{Role: RoleCaption, Tone: ToneSecondary, SingleLine: true, Key: "description"})
		}
	})
	if it.Shortcut != "" {
		Text(c, it.Shortcut, TextProps{Role: RoleCaption, Tone: ToneSecondary, SingleLine: true, Key: "shortcut"})
	}
}

// menuItemStatic draws one item in a given state without a popover, for the gallery and screenshots.
func menuItemStatic(c *ui.Context, it MenuItem, state force.State) ui.Element {
	pl := pal(c)
	row := ui.Row(c).Width(menuMinWidth+theme.Space8).MinHeight(theme.ControlRow).Padding(theme.Space1, theme.Space2).Gap(theme.Space2).
		Radius(theme.RadiusCtrl).AlignItems(ui.Center).Disabled(it.Disabled)
	if fill, ok := menuRowFill(pl, it, state == force.Pressed, state == force.Hover); ok {
		row.Background(fill)
	}
	row.TextColor(menuRowText(pl, it))
	focused := state == force.Focus && !it.Disabled
	accent := col(pl.Accent)
	row.DrawOver(func(pt *ui.Painter, r ui.Rect) {
		if focused {
			pt.Stroke(r, accent, theme.RadiusCtrl, theme.BorderFocus)
		}
	})
	return row.Children(func() { menuRowContent(c, it, it.Icon != "") })
}
