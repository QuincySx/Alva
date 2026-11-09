// INPUT: Alva Checkbox (Off/On/Mixed x Focus/Disabled) and Switch (Off/On x Focus/Disabled) definitions, MyGo CheckboxBase/SwitchBase.
// OUTPUT: Checkbox with a Mixed state and Switch (38x24): native keyboard, checked state and accessibility with design paint and focus ring.
// POS: [L7 Kit] Mixed is a Props bool owned by the caller (e.g. a parent of partially selected children); clicking toggles *checked and the caller recomputes Mixed.
//go:build desktop

package kit

import (
	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/audit"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/force"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

// CheckboxProps configure Checkbox.
type CheckboxProps struct {
	// Mixed shows the indeterminate mark and is exposed to assistive technology as mixed.
	Mixed    bool
	Disabled bool
	Key      string
}

// Checkbox draws an 18px box with a label row of control height; label is also the accessible name.
func Checkbox(c *ui.Context, checked *bool, label string, p CheckboxProps) ui.Element {
	audit.Named("Checkbox", label)
	pl, fs := pal(c), force.Take()
	b := ui.CheckboxBase(keyed(c, p.Key), checked).Label(label).Height(theme.ControlHeight).Gap(theme.Space2).
		AlignItems(ui.Center).Disabled(p.Disabled).FocusRing(false)
	if p.Mixed {
		b.Mixed()
	}
	on := *checked || p.Mixed
	focused := !p.Disabled && fs.Focused(b.FocusVisible())
	surface, accent := col(pl.Surface), col(pl.Accent)
	b.Children(func() {
		mark := ui.Box(c.Key("mark")).Size(checkBoxSize, checkBoxSize).Shrink(0).Radius(theme.RadiusSmall).AlignItems(ui.Center).Justify(ui.Center)
		if on {
			mark.Background(accent).Border(1, accent)
		} else {
			mark.Background(surface).Border(1, col(pl.ControlBorder))
		}
		mark.DrawOver(func(pt *ui.Painter, r ui.Rect) {
			if focused {
				focusRing(pt, r, theme.RadiusSmall, surface, accent)
			}
		})
		mark.Children(func() {
			switch {
			case p.Mixed:
				ui.Box(c.Key("mixed")).Size(mixedBarW, mixedBarH).Radius(mixedBarRadius).Background(col(pl.OnAccent))
			case *checked:
				Icon(c, icon.Check, IconProps{Small: true}).TextColor(col(pl.OnAccent))
			}
		})
		Text(c, label, TextProps{SingleLine: true, Key: "label"}).MinWidth(0)
	})
	return b
}

// SwitchProps configure Switch.
type SwitchProps struct {
	Disabled bool
	Key      string
}

// Switch paints the Foundation switch (38x24, 16px thumb). label is the accessible name; the visible
// text belongs to the row that hosts the switch.
func Switch(c *ui.Context, on *bool, label string, p SwitchProps) ui.Element {
	audit.Named("Switch", label)
	pl, fs := pal(c), force.Take()
	b := ui.SwitchBase(keyed(c, p.Key), on).Label(label).Size(switchWidth, switchHeight).Radius(switchRadius).Disabled(p.Disabled).FocusRing(false)
	checked, disabled := *on, p.Disabled
	focused := !disabled && fs.Focused(b.FocusVisible())
	surface, accent := col(pl.Surface), col(pl.Accent)
	track, thumb, stroke := col(pl.Subtle), col(pl.Secondary), col(pl.Border)
	x := float32(switchInset)
	if checked {
		x = switchWidth - switchInset - switchThumb
		if !disabled {
			track, thumb, stroke = accent, col(pl.OnAccent), accent
		}
	}
	b.Draw(func(pt *ui.Painter, r ui.Rect) {
		pt.Fill(r, track, switchRadius)
		pt.Stroke(r, stroke, switchRadius, 1)
		pt.Fill(ui.Rect{X: r.X + x, Y: r.Y + (r.H-switchThumb)/2, W: switchThumb, H: switchThumb}, thumb, thumbRadius)
		if focused {
			focusRing(pt, r, switchRadius, surface, accent)
		}
	})
	return b
}
