// INPUT: Kit internals that composed blocks (apps/ui/kit/blocks) and gallery pages need.
// OUTPUT: Loader, LoaderColor, InlineFocus and PanelSurface: thin exported wrappers, nothing new in behavior.
// POS: [L7 Kit] The seam between kit and kit/blocks: blocks reuse the loader, focus indicator, floating surface and per-cell state instead of copying them.
//go:build desktop

package kit

import (
	"github.com/egoist/mygo/ui"
)

// Loader draws the busy indicator at size in the given tone; it is static under reduced motion.
func Loader(c *ui.Context, size float32, tone Tone) ui.Element {
	color, ok := toneColor(pal(c), tone)
	if !ok {
		color = col(pal(c).Secondary)
	}
	return loader(c, size, color)
}

// InlineFocus paints the row focus indicator: one 2px accent stroke inside the box.
func InlineFocus(p *ui.Painter, r ui.Rect, radius float32, accent ui.Color) {
	inlineFocus(p, r, radius, accent)
}

// PanelSurface styles e as a floating panel (popover recipe, shadow.popup).
func PanelSurface(c *ui.Context, e ui.Element) ui.Element { return popupSurface(e, pal(c)) }

// LoaderColor draws the busy indicator in an explicit color, for controls that paint their own fill.
func LoaderColor(c *ui.Context, size float32, color ui.Color) ui.Element {
	return loader(c, size, color)
}
