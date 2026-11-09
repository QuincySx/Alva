// INPUT: An anchor element and tip text.
// OUTPUT: Tooltip: the design tip (surface, 1px border, 4px radius, caption text) shown after hover rest and at once on keyboard focus.
// POS: [L7 Kit] Timing, Escape and focus triggers belong to MyGo's TooltipBase; the kit only dresses it and also gives the anchor a Description for assistive technology.
//go:build desktop

package kit

import (
	"strings"

	"github.com/QuincySx/Alva/apps/ui/kit/internal/audit"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

// Tooltip attaches a tip to anchor and returns the tip element, or an absent element while it is hidden.
// The text is required; an empty text attaches nothing.
func Tooltip(c *ui.Context, anchor ui.Element, text string) ui.Element {
	return tooltip(c, anchor, text, false)
}

// tooltip is Tooltip that also keeps the busy hint: the anchor's one Description reads "tip. busy hint".
func tooltip(c *ui.Context, anchor ui.Element, text string, busy bool) ui.Element {
	if strings.TrimSpace(text) == "" {
		return ui.Element{}
	}
	audit.Named("Tooltip", text)
	pl := pal(c)
	description := text
	if busy {
		description = text + "。" + busyHint
	}
	audit.Described("Tooltip", description)
	anchor.Description(description)
	return ui.TooltipBase(c, anchor, func(tip ui.Element) {
		tip.Margin(tipGap, 0, tipGap, 0)
		tipSurface(tip, pl).Children(func() { Text(c, text, TextProps{Role: RoleCaption, Key: "tip"}) })
	})
}

// tipSurface is the design tip: surface, 1px border, 4px radius and the popup shadow.
func tipSurface(tip ui.Element, pl theme.Palette) ui.Element {
	tip.Padding(theme.Space1, theme.Space2).Radius(theme.RadiusSmall).Background(col(pl.Surface)).Border(1, col(pl.Border)).TextColor(col(pl.Text))
	return popupShadow(tip, pl)
}

// tipStatic draws the tip without an anchor, for the gallery and screenshots.
func tipStatic(c *ui.Context, text string) ui.Element {
	return tipSurface(ui.Row(c), pal(c)).Children(func() { Text(c, text, TextProps{Role: RoleCaption, Key: "tip"}) })
}
