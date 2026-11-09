// INPUT: Alva Button definitions (Kind x State x Content), the shared palette and MyGo ButtonBase input/focus/accessibility.
// OUTPUT: Button: 32px control with Primary/Secondary/Ghost/Danger kinds, Busy (stable width, name and focus) and gated OnClick.
// POS: [L7 Kit] Hover/Pressed/Focus come from the runtime (or internal/force in the gallery); only Disabled and Busy are Props.
//go:build desktop

package kit

import (
	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/audit"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/force"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

// ButtonKind is the design Kind axis. Secondary is the zero value, the design default.
type ButtonKind uint8

const (
	Secondary ButtonKind = iota
	Primary
	Ghost
	Danger
)

// ButtonProps configure Button. The zero value is an enabled Secondary text button.
type ButtonProps struct {
	Kind ButtonKind
	// Icon is the leading icon; while Busy the loader replaces it.
	Icon icon.Name
	// TrailingIcon sits after the label; while Busy and without a leading Icon the loader replaces it.
	TrailingIcon icon.Name
	// IconOnly shows just the icon: the label stays as the accessible name and tooltip.
	IconOnly bool
	// Busy keeps width, name and focus, exposes a busy state and ignores repeated activation.
	Busy bool
	// Disabled removes the control from input and Tab order. Busy takes priority while it lasts.
	Disabled bool
	// Tooltip overrides the tooltip text; IconOnly buttons default to the label.
	Tooltip string
	// OnClick runs on activation unless Busy or Disabled. Use it instead of Clicked(), which MyGo cannot gate.
	OnClick func()
	Key     string
}

type buttonLook struct {
	fill, border, text ui.Color
	opacity            float32
}

// buttonState resolves the look of a kind in a state; the rules are the Foundation Button variants.
func buttonLook0(p theme.Palette, kind ButtonKind, hovered, pressed, busy, disabled bool) buttonLook {
	l := buttonLook{fill: col(p.Surface), border: col(p.Border), text: col(p.Text), opacity: 1}
	transparent := col(p.Transparent)
	switch kind {
	case Primary:
		l.fill, l.border, l.text = col(p.Text), col(p.Text), col(p.Surface)
	case Ghost:
		l.fill, l.border, l.text = transparent, transparent, col(p.Secondary)
	case Danger:
		l.fill, l.border, l.text = transparent, transparent, col(p.Danger)
	default:
		if busy {
			l.fill, l.border = col(p.Subtle), col(p.Subtle)
		}
	}
	switch {
	case disabled:
		l.fill, l.border, l.text = col(p.Subtle), col(p.Subtle), col(p.Secondary)
		if kind == Ghost {
			l.fill, l.border = transparent, transparent
		}
	case busy:
	case pressed:
		switch kind {
		case Primary:
			l.opacity = pressedOpacity
		case Ghost:
			l.fill, l.text = col(p.Hover), col(p.Text)
		case Danger:
			l.fill, l.text = col(p.Danger), col(p.Surface)
		default:
			l.fill, l.border = col(p.Hover), col(p.Hover)
		}
	case hovered:
		switch kind {
		case Primary:
			l.opacity = hoverOpacity
		case Ghost:
			l.fill = col(p.Subtle)
		case Danger:
			l.fill = col(p.DangerBg)
		default:
			l.fill, l.border = col(p.Hover), col(p.Hover)
		}
	}
	return l
}

// Button draws a design Button. label is the visible text and the accessible name.
func Button(c *ui.Context, label string, p ButtonProps) ui.Element {
	audit.Named("Button", label)
	pl, fs := pal(c), force.Take()
	b := ui.ButtonBase(keyed(c, p.Key)).Label(label)
	b.Height(theme.ControlHeight).Padding(0, theme.Space2).Gap(theme.Space2).
		AlignItems(ui.Center).Justify(ui.Center).AlignSelf(ui.Start).Radius(theme.RadiusCtrl).
		FontSize(theme.FontSizeBody).FontWeight(theme.FontWeightRegular).LineHeight(theme.LineNormal).FocusRing(false)
	if p.IconOnly {
		b.Width(theme.ControlHeight).Padding(0)
	}
	b.Disabled(!p.Busy && p.Disabled)
	disabled := b.IsDisabled()
	look := buttonLook0(pl, p.Kind, fs.Hovered(b.Hovered()), fs.Pressed(b.Pressed()), p.Busy, disabled)
	b.Background(look.fill).TextColor(look.text)
	if look.opacity != 1 {
		b.Opacity(look.opacity)
	}
	if tip := p.Tooltip; tip != "" || p.IconOnly {
		if tip == "" {
			tip = label
		}
		defer tooltip(c, b, tip, p.Busy)
	} else if p.Busy {
		b.Description(busyHint)
	}
	// Exit transitions can retain this painting after its build pass ends, so the callback keeps
	// immutable paint state rather than a frame-scoped Element.
	focused := !disabled && fs.Focused(b.FocusVisible())
	surface, accent, border := col(pl.Surface), col(pl.Accent), look.border
	b.DrawOver(func(pt *ui.Painter, r ui.Rect) {
		if focused {
			focusRing(pt, r, theme.RadiusCtrl, surface, accent)
		} else {
			pt.Stroke(r, border, theme.RadiusCtrl, 1)
		}
	})
	leading, trailing := p.Icon, p.TrailingIcon
	b.Children(func() {
		slot := func(name icon.Name, loading bool, key string) {
			if loading {
				loader(c.Key(key), theme.IconSmall, look.text)
			} else {
				Icon(c, name, IconProps{Small: true})
			}
		}
		switch {
		case p.IconOnly:
			slot(leading, p.Busy, "activity")
			return
		case leading != "":
			slot(leading, p.Busy, "activity")
		}
		if label != "" {
			text := ui.Text(c, label).FontSize(theme.FontSizeBody).FontWeight(theme.FontWeightRegular).LineHeight(theme.LineNormal).SingleLine()
			if p.Busy && leading == "" && trailing == "" {
				text.Opacity(0)
				ui.Box(c.Key("loading-overlay")).Absolute().Top(0).Right(0).Bottom(0).Left(0).Center().Children(func() {
					loader(c.Key("activity"), theme.IconSmall, look.text)
				})
			}
		}
		if trailing != "" {
			slot(trailing, p.Busy && leading == "", "activity-trailing")
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
