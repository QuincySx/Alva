// INPUT: Alva IconButton definitions (State incl. Selected variants), shared palette, a required accessible name.
// OUTPUT: IconButton: 28x28 icon control whose name is a positional argument, with tooltip on hover and keyboard focus.
// POS: [L7 Kit] The label is positional so a nameless icon button does not compile; Selected is a Props bool, other states are runtime.
//go:build desktop

package kit

import (
	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/audit"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/force"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

// IconButtonProps configure IconButton.
type IconButtonProps struct {
	// Selected marks a toggled-on control (pressed look, exposed as selected).
	Selected bool
	// Busy swaps the icon for the loader, keeps focus and ignores repeated activation.
	Busy     bool
	Disabled bool
	// Tooltip overrides the tooltip text; it defaults to the label.
	Tooltip string
	OnClick func()
	Key     string
}

// IconButton draws a 28x28 icon control. label is both the accessible name and the tooltip.
func IconButton(c *ui.Context, name icon.Name, label string, p IconButtonProps) ui.Element {
	audit.Named("IconButton", label)
	pl, fs := pal(c), force.Take()
	b := ui.ButtonBase(keyed(c, p.Key)).Label(label)
	b.Width(theme.ControlIcon).Height(theme.ControlIcon).Padding(0).AlignItems(ui.Center).Justify(ui.Center).
		AlignSelf(ui.Start).Radius(theme.RadiusCtrl).FocusRing(false).Selected(p.Selected)
	b.Disabled(!p.Busy && p.Disabled)
	disabled := b.IsDisabled()
	fill, text := col(pl.Transparent), col(pl.Secondary)
	switch {
	case disabled || p.Busy:
	case fs.Pressed(b.Pressed()):
		fill, text = col(pl.Hover), col(pl.Text)
	case p.Selected:
		fill, text = col(pl.AccentSoft), col(pl.Accent)
	case fs.Hovered(b.Hovered()):
		fill, text = col(pl.Subtle), col(pl.Text)
	}
	if p.Selected && (disabled || p.Busy) {
		fill = col(pl.AccentSoft)
	}
	b.Background(fill).TextColor(text)
	tip := p.Tooltip
	if tip == "" {
		tip = label
	}
	defer tooltip(c, b, tip, p.Busy)
	focused := !disabled && fs.Focused(b.FocusVisible())
	surface, accent := col(pl.Surface), col(pl.Accent)
	b.DrawOver(func(pt *ui.Painter, r ui.Rect) {
		if focused {
			focusRing(pt, r, theme.RadiusCtrl, surface, accent)
		}
	})
	b.Children(func() {
		if p.Busy {
			loader(c.Key("activity"), theme.IconSmall, text)
		} else {
			Icon(c, name, IconProps{Small: true})
		}
	})
	if p.OnClick != nil {
		busy, off, action := p.Busy, p.Disabled, p.OnClick
		b.OnClick(func() {
			if !busy && !off {
				action()
			}
		})
	}
	return b
}
