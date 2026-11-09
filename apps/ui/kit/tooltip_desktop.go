// INPUT: An anchor element and tip text.
// OUTPUT: Tooltip: the design tip (surface, 1px border, 4px radius, caption text) shown after hover rest and at once on keyboard focus.
// POS: [L7 Kit] Timing, Escape and focus triggers belong to MyGo's TooltipBase; the kit only dresses it and also gives the anchor a Description for assistive technology.
//go:build desktop

package kit

import (
	"strings"
	"sync/atomic"
	"time"

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
	if overlayDepth.Load() > 0 {
		overlayTip(c, anchor, text)
		return ui.Element{}
	}
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

// overlayDepth counts the Dialog, Popover and Menu contents being built. MyGo's TooltipBase takes Escape for itself
// while its tip shows, so inside an overlay the first Escape would hide the tip instead of closing the overlay.
// Builds are synchronous on the UI thread, so a counter is a safe scope.
var overlayDepth atomic.Int32

// inOverlay builds content with tooltips switched to the self-drawn form that never touches Escape.
func inOverlay(content func()) {
	overlayDepth.Add(1)
	defer overlayDepth.Add(-1)
	content()
}

// tipDelay is how long the pointer rests before a self-drawn tip shows; keyboard focus shows it at once.
const tipDelay = 600 * time.Millisecond

type tipState struct{ since time.Time }

// overlayTip draws the tip in MyGo's plain overlay layer, positioned above the anchor like TooltipBase would,
// but registers no Escape shortcut: Escape reaches the dialog or popover that holds the anchor.
func overlayTip(c *ui.Context, anchor ui.Element, text string) {
	st := ui.Local(anchor, "kit-tip", func() tipState { return tipState{} })
	show := anchor.FocusVisible()
	if !show && anchor.Hovered() {
		if st.since.IsZero() {
			st.since = c.Now()
		}
		if wait := tipDelay - c.Now().Sub(st.since); wait <= 0 {
			show = true
		} else {
			c.After(wait)
		}
	} else if !anchor.Hovered() {
		st.since = time.Time{}
	}
	if !show {
		return
	}
	pl := pal(c)
	ui.Overlay(c, func() {
		tip := ui.Box(c.Key("kit-tip")).Absolute().PassThrough().Role(ui.RoleTooltip)
		tip.AttachTo(anchor, ui.AnchorTop, ui.AnchorBottom).Margin(tipGap, 0, tipGap, 0)
		tipSurface(tip, pl).Children(func() { Text(c, text, TextProps{Role: RoleCaption, Key: "tip"}) })
	})
}
