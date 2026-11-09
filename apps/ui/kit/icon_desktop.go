// INPUT: icon.Name, optional tone and size role.
// OUTPUT: Icon: a Lucide glyph at the design 16px (or 14px small) size that follows the text color unless a tone is given.
// POS: [L7 Kit] Only one icon family exists (Lucide 1.61.0); PanelLeft is Panel mirrored.
//go:build desktop

package kit

import (
	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

// IconProps configure Icon. The zero value is the 16px icon in the inherited text color.
type IconProps struct {
	// Small selects the 14px size used inside controls and rows.
	Small bool
	Tone  Tone
	// Label names a meaningful icon for assistive technology; decorative icons leave it empty.
	Label string
	Key   string
}

func iconSize(small bool) float32 {
	if small {
		return theme.IconSmall
	}
	return theme.IconSize
}

// Icon draws a design icon. The empty or an unknown name draws an empty slot of the same size so
// layouts do not jump.
func Icon(c *ui.Context, name icon.Name, p IconProps) ui.Element {
	size := iconSize(p.Small)
	mark := svg(name)
	var e ui.Element
	if mark == nil {
		e = ui.Box(keyed(c, p.Key)).Size(size, size).Shrink(0)
	} else {
		e = ui.Icon(keyed(c, p.Key), mark).Size(size, size).Shrink(0)
		if icon.FlipX(name) {
			e.Scale(mirror, 1)
		}
	}
	if p.Label != "" {
		e.Label(p.Label)
	}
	if color, ok := toneColor(pal(c), p.Tone); ok {
		e.TextColor(color)
	}
	return e
}
