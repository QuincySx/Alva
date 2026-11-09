// INPUT: Alva DialogHeader/DialogFooter definitions, the popover and dialog block recipes, MyGo PopoverBase and DialogBase.
// OUTPUT: Popover, Dialog (centered) and Sheet (right edge) with DialogHeader and DialogFooter; every floating surface uses shadow.popup.
// POS: [L7 Kit] Focus lock, Escape, backdrop dismissal and focus return are MyGo's DialogBase/PopoverBase; the kit supplies the look and the Esc/focus tests.
//go:build desktop

package kit

import (
	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/audit"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

const (
	dialogWidth = 480
	scrimAlpha  = .72
	// dialogMargins is the margin on both sides of a dialog together.
	dialogMargins = 2 * theme.Space6
)

// PopoverProps configure Popover.
type PopoverProps struct {
	// Label names the panel for assistive technology.
	Label string
	// Width fixes the panel width; zero hugs the content.
	Width float32
}

// Popover shows content in a floating panel below anchor while *open is true. Escape or a press outside closes it and MyGo
// returns the focus to the element that had it; where there is no room below it flips above.
func Popover(c *ui.Context, anchor ui.Element, open *bool, p PopoverProps, content func()) {
	audit.Named("Popover", p.Label)
	pl := pal(c)
	ui.PopoverBase(c, anchor, open, func(panel ui.Element) {
		popupSurface(panel, pl).Margin(theme.Space1, 0, 0).Label(p.Label)
		if p.Width > 0 {
			panel.Width(p.Width)
		}
		panel.Children(func() { inOverlay(content) })
	})
}

// DialogProps configure Dialog and Sheet.
type DialogProps struct {
	// Label names the dialog for assistive technology.
	Label string
	// Width fixes the panel width; zero uses the design dialog width (or the panel width for a Sheet).
	Width float32
	// Sheet docks the panel to the right edge at full height instead of centering it.
	Sheet bool
}

// Dialog shows content over a scrim while *open is true. Focus stays inside, Escape or a click on the scrim closes it,
// and the focus returns to the element that opened it.
func Dialog(c *ui.Context, open *bool, p DialogProps, content func()) {
	audit.Named("Dialog", p.Label)
	pl := pal(c)
	w, h := c.Size()
	// A dialog keeps a space.6 margin on every side.
	fitW, fitH := w-dialogMargins, h-dialogMargins
	ui.DialogBase(c, open, func(backdrop, panel ui.Element) {
		backdrop.Background(col(pl.Scrim).Alpha(scrimAlpha))
		panel.Label(p.Label).Background(col(pl.Surface)).Border(1, col(pl.Border)).Padding(theme.Space6)
		popupShadow(panel, pl)
		if p.Sheet {
			backdrop.Row().Justify(ui.End).AlignItems(ui.Stretch)
			panel.Width(theme.SizePanel(false)).Radius(0).MaxWidth(w)
			if p.Width > 0 {
				panel.Width(p.Width)
			}
		} else {
			// A dialog never outgrows the window: it keeps a space.6 margin and its content scrolls instead.
			panel.Radius(theme.RadiusCard).Width(dialogWidth).MaxWidth(fitW).MaxHeight(fitH)
			if p.Width > 0 {
				panel.Width(p.Width)
			}
		}
		panel.Children(func() {
			ui.Scroll(c.Key("kit-dialog-body")).Grow(1).MinHeight(0).Children(func() {
				ui.Column(c.Key("kit-dialog-content")).FillWidth().Gap(theme.Space4).Children(func() { inOverlay(content) })
			})
		})
	})
}

// DialogHeader is the dialog title block: title, optional description and a close action when onClose is set.
func DialogHeader(c *ui.Context, title, description string, onClose func()) ui.Element {
	return ui.Row(c).FillWidth().Gap(theme.Space4).AlignItems(ui.Start).Children(func() {
		ui.Column(c.Key("copy")).Grow(1).MinWidth(0).Gap(theme.Space1).Children(func() {
			Text(c, title, TextProps{Role: RoleSubtitle, Strong: true, Key: "title"})
			if description != "" {
				Text(c, description, TextProps{Tone: ToneSecondary, Key: "desc"})
			}
		})
		if onClose != nil {
			IconButton(c, icon.Close, "关闭", IconButtonProps{OnClick: onClose, Key: "close"})
		}
	})
}

// DialogFooter right-aligns the dialog actions (Buttons) with space.2 between them.
func DialogFooter(c *ui.Context, actions func()) ui.Element {
	return ui.Row(c).FillWidth().Justify(ui.End).Gap(theme.Space2).Children(actions)
}
