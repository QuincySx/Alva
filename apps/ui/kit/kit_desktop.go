// INPUT: MyGo *Base controls, the generated design tokens and the embedded Lucide icons.
// OUTPUT: Shared helpers (palette access, tones, focus ring, loader, SVG cache) behind every kit component.
// POS: [L7 Kit] Thin Alva skin over MyGo: components never hold color or size literals; they read theme.* and ask this file for the pieces every control shares.
//go:build desktop

package kit

import (
	"sync"
	"time"

	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

// Tone picks a text/icon color role. The zero value inherits the surrounding text color.
type Tone uint8

const (
	// ToneInherit keeps the color the parent sets.
	ToneInherit Tone = iota
	ToneText
	ToneSecondary
	ToneAccent
	ToneSuccess
	ToneWarning
	ToneDanger
)

// Geometry that has no design token of its own is named here once so no call site carries a number.
const (
	switchWidth  = 38
	switchHeight = 24
	switchThumb  = 16
	switchInset  = theme.Space1
	checkBoxSize = 18
	focusGap     = theme.Space1
	mixedBarW    = 8
	mixedBarH    = 2
	// mixedBarRadius, switchRadius and thumbRadius are half their boxes: fully rounded ends.
	mixedBarRadius = mixedBarH / 2
	switchRadius   = switchHeight / 2
	thumbRadius    = switchThumb / 2
	// Primary buttons dim instead of changing fill: the Foundation Primary hover and pressed opacities.
	hoverOpacity   = .98
	pressedOpacity = .96
	// mirror flips an icon horizontally (PanelLeft).
	mirror       = -1
	tipGap       = 6
	menuMinWidth = 200
	loaderLabel  = "Loading indicator"
	busyHint     = "正在处理，请等待完成。"
)

func pal(c *ui.Context) theme.Palette { return theme.Of(c) }

func col(c theme.Color) ui.Color { return theme.UI(c) }

func keyed(c *ui.Context, key string) *ui.Context {
	if key == "" {
		return c
	}
	return c.Key(key)
}

func toneColor(p theme.Palette, t Tone) (color ui.Color, ok bool) {
	switch t {
	case ToneText:
		return col(p.Text), true
	case ToneSecondary:
		return col(p.Secondary), true
	case ToneAccent:
		return col(p.Accent), true
	case ToneSuccess:
		return col(p.Success), true
	case ToneWarning:
		return col(p.Warning), true
	case ToneDanger:
		return col(p.Danger), true
	}
	return
}

// focusRing paints the design focus ring: a surface-colored gap inside a 2px accent line.
func focusRing(p *ui.Painter, r ui.Rect, radius float32, surface, accent ui.Color) {
	p.Stroke(r, surface, radius, focusGap)
	p.Stroke(r, accent, radius, theme.BorderFocus)
}

var (
	svgMu    sync.Mutex
	svgCache = map[icon.Name]*ui.SVG{}
)

// svg returns the parsed icon, or nil for the empty or an unknown name.
func svg(n icon.Name) *ui.SVG {
	if n == "" {
		return nil
	}
	svgMu.Lock()
	defer svgMu.Unlock()
	if s, ok := svgCache[n]; ok {
		return s
	}
	data, ok := icon.Data(n)
	if !ok {
		svgCache[n] = nil
		return nil
	}
	s := ui.MustParseSVG(data)
	svgCache[n] = s
	return s
}

// loader draws the busy indicator: a linear revolution every theme.MotionSpinner, static when motion
// is reduced. The rotation is paint-only, so it neither rebuilds the view nor starts a timer per control.
func loader(c *ui.Context, size float32, color ui.Color) ui.Element {
	reduced := theme.ReducedMotion(c)
	mark := svg(icon.Loader)
	return ui.Box(c).Label(loaderLabel).Role(ui.RoleProgress).Size(size, size).Shrink(0).
		Draw(func(p *ui.Painter, r ui.Rect) {
			if reduced {
				p.Icon(mark, r, color)
				return
			}
			angle := float32(p.Now().UnixMilli()%int64(theme.MotionSpinner/time.Millisecond)) * 360 / float32(theme.MotionSpinner/time.Millisecond)
			p.Transform(ui.Rotation(angle), r.X+r.W/2, r.Y+r.H/2, func() { p.Icon(mark, r, color) })
			p.After(35 * time.Millisecond)
		})
}

// popupShadow applies the single design popup shadow (shadow.popup) to a floating surface.
func popupShadow(e ui.Element, p theme.Palette) ui.Element {
	s := p.Popup
	return e.Shadow(s.X, s.Y, s.Blur, s.Spread, col(s.Color))
}

// popupSurface styles a floating panel with the design popover recipe: surface, border, card radius,
// space.2 padding and the popup shadow.
func popupSurface(e ui.Element, p theme.Palette) ui.Element {
	e.Padding(theme.Space2).Gap(theme.Space1).Radius(theme.RadiusCard).Background(col(p.Surface)).Border(1, col(p.Border))
	return popupShadow(e, p)
}
