// INPUT: A trailing action inside an input box (reveal, clear), an accessible name and an icon.
// OUTPUT: InlineAction: a 14px icon with no fill, hover only changing color, a hit area as tall as the input and an inset focus stroke.
// POS: [L7 Kit] The shared lightweight in-field action; its focus indicator is drawn inside the box so it never stacks on the input's own border.
//go:build desktop

package kit

import (
	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/audit"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/force"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

// InlineActionProps configure InlineAction.
type InlineActionProps struct {
	// Selected marks a toggled-on action (accent color), as the reveal toggle while text is shown.
	Selected bool
	Disabled bool
	// Tooltip overrides the tooltip text; it defaults to the label.
	Tooltip string
	OnClick func()
	Key     string
}

// InlineAction draws a small icon action for the inside of an input box. label is the accessible name
// and the tooltip. Hover and pressed change only the icon color; focus draws a 2px stroke inset by
// space.1 so it never overlaps the input's own border.
func InlineAction(c *ui.Context, name icon.Name, label string, p InlineActionProps) ui.Element {
	audit.Named("InlineAction", label)
	pl, fs := pal(c), force.Take()
	b := ui.ButtonBase(keyed(c, p.Key)).Label(label).Width(theme.IconSmall + theme.Space2).Height(theme.ControlHeight).Padding(0).
		AlignItems(ui.Center).Justify(ui.Center).Shrink(0).FocusRing(false).Selected(p.Selected).Disabled(p.Disabled)
	text := col(pl.Secondary)
	switch {
	case p.Disabled:
	case fs.Pressed(b.Pressed()), fs.Hovered(b.Hovered()):
		text = col(pl.Text)
	case p.Selected:
		text = col(pl.Accent)
	}
	b.TextColor(text)
	focused := !p.Disabled && fs.Focused(b.FocusVisible())
	accent := col(pl.Accent)
	b.DrawOver(func(pt *ui.Painter, r ui.Rect) {
		if focused {
			inlineFocus(pt, ui.Rect{X: r.X + theme.Space1/2, Y: r.Y + theme.Space1, W: r.W - theme.Space1, H: r.H - 2*theme.Space1}, theme.RadiusSmall, accent)
		}
	})
	b.Children(func() { Icon(c, name, IconProps{Small: true}) })
	if p.OnClick != nil {
		action := p.OnClick
		b.OnClick(action)
	}
	tip := p.Tooltip
	if tip == "" {
		tip = label
	}
	defer tooltip(c, b, tip, false)
	return b
}
