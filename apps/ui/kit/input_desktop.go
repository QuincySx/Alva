// INPUT: Alva Input (Text/Search; Select is its own control), TextArea and Field definitions, MyGo TextInputBase/TextAreaBase.
// OUTPUT: Input, Password, TextArea: 32px subtle inputs with 1px border, 2px accent focus, danger error, read-only, disabled and busy states.
// POS: [L7 Kit] Input returns the editable element (Changed/Focused work on it); the surrounding box is a container that draws the border.
//go:build desktop

package kit

import (
	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/audit"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/force"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

const invalidMessage = "输入无效"

// InputProps configure Input. The zero value is an enabled single-line text input.
type InputProps struct {
	// Label is the accessible name and is required unless the Input sits in a Field with a Title.
	Label, Placeholder string
	// Leading and Trailing are static 14px icon slots (Search uses Leading: icon.Search).
	Leading, Trailing icon.Name
	// Invalid marks the value invalid; Problem is the message assistive technology reads with it
	// (setting it implies Invalid).
	Invalid  bool
	Problem  string
	ReadOnly bool
	Disabled bool
	// Busy shows the loader in the trailing slot; the text stays editable.
	Busy bool
	// Width fixes the width; zero fills the available width.
	Width float32
	Key   string
}

// merge applies the enclosing Field's name and states.
func (p InputProps) merge() InputProps {
	f := fieldDefaults()
	if p.Label == "" {
		p.Label = f.Title
	}
	if p.Problem == "" {
		p.Problem = f.Error
	}
	p.Invalid = p.Invalid || f.Error != ""
	p.Disabled = p.Disabled || f.Disabled
	p.ReadOnly = p.ReadOnly || f.ReadOnly
	p.Busy = p.Busy || f.Busy
	return p
}

type controlPaint struct {
	fill, border ui.Color
	width        float32
}

// controlLook is shared by Input, TextArea and Select triggers.
func controlLook(pl theme.Palette, focused, invalid, readOnly bool) controlPaint {
	l := controlPaint{fill: col(pl.Subtle), border: col(pl.Border), width: 1}
	if readOnly {
		l.fill = col(pl.Surface)
	}
	if focused {
		l.fill = col(pl.Surface)
		l.border, l.width = col(pl.Accent), theme.BorderFocus
	}
	if invalid {
		l.border = col(pl.Danger)
		if focused {
			l.width = theme.BorderFocus
		}
	}
	return l
}

func inputText(e ui.Element, pl theme.Palette) ui.Element {
	return e.FontSize(theme.FontSizeBody).FontWeight(theme.FontWeightRegular).LineHeight(theme.LineNormal).TextColor(col(pl.Text)).FocusRing(false)
}

func markInvalid(e ui.Element, invalid bool, problem string) {
	if !invalid {
		return
	}
	if problem == "" {
		problem = invalidMessage
	}
	e.Error(problem)
}

// Input draws a single-line text input editing *value and returns the editable element.
func Input(c *ui.Context, value *string, p InputProps) ui.Element {
	return inputBox(c, value, p.merge(), nil, "Input")
}

// slotBuilder lets a sibling control (the masked-text variant) own the trailing slot.
type slotBuilder func(c *ui.Context, pl theme.Palette, disabled bool)

func inputBox(c *ui.Context, value *string, p InputProps, trailing slotBuilder, kind string) ui.Element {
	audit.Named(kind, p.Label)
	pl, fs := pal(c), force.Take()
	invalid := p.Invalid || p.Problem != ""
	boxKey := ""
	if p.Key != "" {
		boxKey = p.Key + "-control"
	}
	var input ui.Element
	box := ui.Row(keyed(c, boxKey)).Height(theme.ControlHeight).Padding(0, theme.Space2).Gap(theme.Space2).
		AlignItems(ui.Center).Radius(theme.RadiusCtrl).Disabled(p.Disabled)
	if p.Width > 0 {
		box.Width(p.Width).Shrink(0)
	} else {
		box.FillWidth()
	}
	box.Children(func() {
		if p.Leading != "" {
			Icon(c, p.Leading, IconProps{Small: true, Tone: ToneSecondary})
		}
		input = inputText(ui.TextInputBase(keyed(c, p.Key), value), pl).Grow(1).MinWidth(0).Height(theme.ControlHeight).Padding(0).
			Label(p.Label).Placeholder(p.Placeholder).PlaceholderColor(col(pl.Secondary)).ReadOnly(p.ReadOnly).Disabled(p.Disabled)
		markInvalid(input, invalid, p.Problem)
		if p.Busy {
			loader(c.Key("busy"), theme.IconSmall, col(pl.Secondary))
		}
		switch {
		case trailing != nil:
			trailing(c, pl, p.Disabled)
		case p.Trailing != "" && !p.Busy:
			Icon(c, p.Trailing, IconProps{Small: true, Tone: ToneSecondary})
		}
	})
	look := controlLook(pl, fs.Focused(input.Focused()), invalid, p.ReadOnly)
	box.Background(look.fill)
	border, width := look.border, look.width
	box.DrawOver(func(pt *ui.Painter, r ui.Rect) { pt.Stroke(r, border, theme.RadiusCtrl, width) })
	return input
}

// Password draws a masked single-line input editing *value. *reveal shows the
// text; the trailing eye toggles it and carries its own accessible name. The
// key icon leads unless Props name another.
func Password(c *ui.Context, value *string, reveal *bool, p InputProps) ui.Element {
	p = p.merge()
	if p.Leading == "" {
		p.Leading = icon.Key
	}
	toggleKey := ""
	if p.Key != "" {
		toggleKey = p.Key + "-reveal"
	}
	input := inputBox(c, value, p, func(c *ui.Context, _ theme.Palette, disabled bool) {
		label := "显示内容"
		if *reveal {
			label = "隐藏内容"
		}
		InlineAction(c, icon.Eye, label, InlineActionProps{Selected: *reveal, Disabled: disabled, Key: toggleKey, OnClick: func() { *reveal = !*reveal }})
	}, "Password")
	if !*reveal {
		input.Password()
	}
	return input
}

// TextAreaProps configure TextArea. The zero value is an enabled multi-line input.
type TextAreaProps struct {
	Label, Placeholder string
	Invalid            bool
	Problem            string
	ReadOnly           bool
	Disabled           bool
	// MinLines and MaxLines grow the area with its text; zero leaves the default height.
	MinLines, MaxLines int
	Key                string
}

// TextArea draws a multi-line text input editing *value.
func TextArea(c *ui.Context, value *string, p TextAreaProps) ui.Element {
	f := fieldDefaults()
	if p.Label == "" {
		p.Label = f.Title
	}
	if p.Problem == "" {
		p.Problem = f.Error
	}
	p.Invalid = p.Invalid || f.Error != ""
	p.Disabled, p.ReadOnly = p.Disabled || f.Disabled, p.ReadOnly || f.ReadOnly
	audit.Named("TextArea", p.Label)
	pl, fs := pal(c), force.Take()
	invalid := p.Invalid || p.Problem != ""
	e := inputText(ui.TextAreaBase(keyed(c, p.Key), value), pl).Padding(theme.Space3).Radius(theme.RadiusCtrl).
		Label(p.Label).Placeholder(p.Placeholder).PlaceholderColor(col(pl.Secondary)).ReadOnly(p.ReadOnly).Disabled(p.Disabled)
	if p.MinLines > 0 || p.MaxLines > 0 {
		e.Lines(max(p.MinLines, 1), max(p.MaxLines, p.MinLines, 1))
	}
	markInvalid(e, invalid, p.Problem)
	look := controlLook(pl, fs.Focused(e.Focused()), invalid, p.ReadOnly)
	e.Background(look.fill)
	border, width := look.border, look.width
	e.DrawOver(func(pt *ui.Painter, r ui.Rect) { pt.Stroke(r, border, theme.RadiusCtrl, width) })
	return e
}
