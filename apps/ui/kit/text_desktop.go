// INPUT: A string, a type role, a tone and weight.
// OUTPUT: Text: design-scale text (caption 11 to display 28, mono) with the design line heights.
// POS: [L7 Kit] Thin wrapper over ui.Text so sizes, weights and line heights come from tokens only.
//go:build desktop

package kit

import (
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

// TextRole is a step of the design type scale. The zero value is Body (13).
type TextRole uint8

const (
	RoleBody TextRole = iota
	RoleCaption
	RoleSubtitle
	RoleHeading
	RoleDisplay
	RoleMono
)

// TextProps configure Text.
type TextProps struct {
	Role   TextRole
	Tone   Tone
	Strong bool
	// Reading uses the 1.6 reading line height for paragraphs instead of 1.5.
	Reading bool
	// SingleLine keeps one line and truncates; zero wraps.
	SingleLine bool
	Key        string
}

func roleSize(r TextRole) float32 {
	switch r {
	case RoleCaption:
		return theme.FontSizeCaption
	case RoleSubtitle:
		return theme.FontSizeSubtitle
	case RoleHeading:
		return theme.FontSizeHeading
	case RoleDisplay:
		return theme.FontSizeDisplay
	}
	return theme.FontSizeBody
}

// Text draws s in the design font at a scale step.
func Text(c *ui.Context, s string, p TextProps) ui.Element {
	weight, line := theme.FontWeightRegular, float32(theme.LineNormal)
	if p.Strong {
		weight = theme.FontWeightStrong
	}
	if p.Reading {
		line = theme.LineReading
	}
	e := ui.Text(keyed(c, p.Key), s).FontSize(roleSize(p.Role)).FontWeight(weight).LineHeight(line)
	if p.Role == RoleMono {
		e.Font(theme.FontMono)
	}
	if p.SingleLine {
		e.SingleLine()
	}
	if color, ok := toneColor(pal(c), p.Tone); ok {
		e.TextColor(color)
	}
	return e
}
