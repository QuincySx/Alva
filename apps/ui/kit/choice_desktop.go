// INPUT: Alva Radio, Segment, Tab and ToggleButton definitions, MyGo RadioBase, SegmentedBase, TabsBase and ToggleBase.
// OUTPUT: Radio, Segmented, Tabs and ToggleButton with native keyboard (arrows, Home/End, Space), design paint and focus indicators.
// POS: [L7 Kit] Selection and toggle controls; Disabled and Busy are Props, Hover/Pressed/Focus come from the runtime or internal/force.
//go:build desktop

package kit

import (
	"strconv"
	"sync/atomic"

	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/audit"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/force"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

const radioSize = checkBoxSize

// RadioProps configure Radio.
type RadioProps struct {
	Disabled bool
	Key      string
}

var radioGroupDepth atomic.Int32

// RadioGroup is the only valid home of Radio: one Tab stop for the whole group, arrows and Home/End choose, and
// assistive technology sees a named radio group. row lays the radios out horizontally.
func RadioGroup(c *ui.Context, label string, row bool, radios func()) ui.Element {
	audit.Named("RadioGroup", label)
	g := ui.RadioGroup(c, func() {
		radioGroupDepth.Add(1)
		defer radioGroupDepth.Add(-1)
		radios()
	}).Label(label)
	if row {
		g.Row()
	}
	return g
}

// Radio draws an 18px circle with a label row of control height; it selects value into *selected.
func Radio[T comparable](c *ui.Context, selected *T, value T, label string, p RadioProps) ui.Element {
	audit.Named("Radio", label)
	if radioGroupDepth.Load() == 0 {
		audit.Named("Radio used outside RadioGroup", "") // a nameless entry fails the audit test
	}
	pl, fs := pal(c), force.Take()
	b := ui.RadioBase(keyed(c, p.Key), selected, value).Label(label).Height(theme.ControlHeight).Gap(theme.Space2).AlignItems(ui.Center).Disabled(p.Disabled).FocusRing(false)
	on := *selected == value
	focused := !p.Disabled && fs.Focused(b.FocusVisible())
	surface, accent := col(pl.Surface), col(pl.Accent)
	return b.Children(func() {
		mark := ui.Box(c.Key("mark")).Size(radioSize, radioSize).Shrink(0).Center().Radius(theme.RadiusPill).Background(surface)
		if on {
			mark.Border(theme.BorderFocus, accent)
		} else {
			mark.Border(1, col(pl.ControlBorder))
		}
		mark.DrawOver(func(pt *ui.Painter, r ui.Rect) {
			if focused {
				focusRing(pt, r, theme.RadiusPill, surface, accent)
			}
		})
		mark.Children(func() {
			if on {
				ui.Box(c.Key("dot")).Size(radioDot, radioDot).Radius(theme.RadiusPill).Background(accent)
			}
		})
		Text(c, label, TextProps{SingleLine: true, Key: "label"}).MinWidth(0)
	})
}

const radioDot = theme.Space2

// segmentLook returns fill, text and weight for one segment or tab.
func segmentLook(pl theme.Palette, selected, disabled, hovered bool) (fill, text ui.Color) {
	fill, text = col(pl.Transparent), col(pl.Secondary)
	switch {
	case disabled && selected:
		fill, text = col(pl.Surface), col(pl.Secondary)
	case disabled:
	case selected:
		fill, text = col(pl.Surface), col(pl.Text)
	case hovered:
		text = col(pl.Text)
	}
	return
}

// SegmentedProps configure Segmented.
type SegmentedProps struct {
	// Label names the control for assistive technology.
	Label string
	// Disabled marks segments by index that cannot be chosen.
	Disabled []bool
	Key      string
}

func isOff(off []bool, i int) bool { return i < len(off) && off[i] }

// Segmented draws a track of segments choosing *selected. Disabled segments are shown but are not part of the radio group,
// so arrows skip them, as with disabled Select options.
func Segmented(c *ui.Context, selected *int, labels []string, p SegmentedProps) ui.Element {
	audit.Named("Segmented", p.Label)
	pl, fs := pal(c), force.Take()
	parts := ui.SegmentedBase(keyed(c, p.Key), selected, len(labels))
	track := parts.Track.Label(p.Label).Padding(theme.Space1).Gap(theme.Space1).Radius(theme.RadiusCtrl).Background(col(pl.Subtle)).AlignSelf(ui.Start).AlignItems(ui.Center)
	accent := col(pl.Accent)
	track.Children(func() {
		for i, label := range labels {
			audit.Named("Segment", label)
			seg := parts.Segment(i).Label(label).Height(theme.ControlRow).Padding(0, theme.Space3).Radius(theme.RadiusSmall + theme.Space1).AlignItems(ui.Center).Justify(ui.Center).
				FocusRing(false).Disabled(isOff(p.Disabled, i))
			fill, text := segmentLook(pl, i == *selected, isOff(p.Disabled, i), fs.Hovered(seg.Hovered()))
			seg.Background(fill).TextColor(text)
			focused := !isOff(p.Disabled, i) && fs.Focused(seg.FocusVisible())
			seg.DrawOver(func(pt *ui.Painter, r ui.Rect) {
				if focused {
					inlineFocus(pt, r, theme.RadiusCtrl, accent)
				}
			})
			seg.Children(func() { Text(c, label, TextProps{SingleLine: true, Strong: i == *selected, Key: "l"}) })
		}
	})
	return track
}

// segmentStatic draws one segment in a given state, for the gallery matrix.
func segmentStatic(c *ui.Context, label string, selected, disabled bool, state force.State) ui.Element {
	pl := pal(c)
	fill, text := segmentLook(pl, selected, disabled, state == force.Hover)
	accent, focused := col(pl.Accent), state == force.Focus && !disabled
	row := ui.Row(c).Height(theme.ControlRow).Padding(0, theme.Space3).Radius(theme.RadiusCtrl).AlignItems(ui.Center).Justify(ui.Center).Background(fill).TextColor(text).Disabled(disabled)
	row.DrawOver(func(pt *ui.Painter, r ui.Rect) {
		if focused {
			inlineFocus(pt, r, theme.RadiusCtrl, accent)
		}
	})
	return row.Children(func() { Text(c, label, TextProps{SingleLine: true, Strong: selected, Key: "l"}) })
}

// TabItem is one tab of Tabs.
type TabItem struct {
	Label    string
	Icon     icon.Name
	Busy     bool
	Disabled bool
}

// TabsProps configure Tabs.
type TabsProps struct {
	Label string
	Key   string
}

// Tabs draws a tab list choosing *selected; the arrows, Home and End move the choice and the focus. Disabled tabs are shown but not chosen.
func Tabs(c *ui.Context, selected *int, tabs []TabItem, p TabsProps) ui.Element {
	audit.Named("Tabs", p.Label)
	pl, fs := pal(c), force.Take()
	// TabsBase walks every tab, so disabled ones are drawn but are not part of it: it chooses among the
	// enabled tabs by position and the choice is mapped back to the caller's index.
	var enabled []int
	position := -1 // no valid selection: TabsBase must not mark the first enabled tab as chosen
	for i, tab := range tabs {
		if !tab.Disabled {
			if i == *selected {
				position = len(enabled)
			}
			enabled = append(enabled, i)
		}
	}
	parts := ui.TabsBase(keyed(c, p.Key), &position, len(enabled))
	list := parts.List.Label(p.Label).Gap(theme.Space1).AlignSelf(ui.Start).AlignItems(ui.Center)
	accent := col(pl.Accent)
	slot := 0
	list.Children(func() {
		for i, tab := range tabs {
			audit.Named("Tab", tab.Label)
			if tab.Disabled {
				tabStaticBox(c, i, tab, i == *selected, pl)
				continue
			}
			el := parts.Tab(slot).Label(tab.Label).Height(theme.ControlHeight).Padding(0, theme.Space3).Gap(theme.Space2).Radius(theme.RadiusCtrl).AlignItems(ui.Center).
				FocusRing(false)
			slot++
			_, text := segmentLook(pl, i == *selected, false, fs.Hovered(el.Hovered()))
			if i == *selected {
				el.Background(col(pl.AccentSoft))
				text = col(pl.Accent)
			}
			el.TextColor(text)
			focused := fs.Focused(el.FocusVisible())
			el.DrawOver(func(pt *ui.Painter, r ui.Rect) {
				if focused {
					inlineFocus(pt, r, theme.RadiusCtrl, accent)
				}
			})
			selectedTab := i == *selected
			el.Children(func() {
				if tab.Icon != "" {
					Icon(c, tab.Icon, IconProps{Small: true, Key: "g"})
				}
				Text(c, tab.Label, TextProps{SingleLine: true, Strong: selectedTab, Key: "l"})
				if tab.Busy {
					loader(c.Key("busy"), theme.IconSmall, text)
				}
			})
		}
	})
	if list.Changed() && position >= 0 && position < len(enabled) {
		*selected = enabled[position]
	}
	return list
}

// tabStaticBox is a disabled tab inside a live Tabs: drawn like the design, not focusable.
func tabStaticBox(c *ui.Context, index int, tab TabItem, selected bool, pl theme.Palette) ui.Element {
	row := ui.Row(c.Key("disabled-"+strconv.Itoa(index))).Label(tab.Label).Height(theme.ControlHeight).Padding(0, theme.Space3).Radius(theme.RadiusCtrl).AlignItems(ui.Center).Disabled(true).TextColor(col(pl.Secondary))
	if selected {
		row.Background(col(pl.AccentSoft))
	}
	return row.Children(func() { Text(c, tab.Label, TextProps{SingleLine: true, Key: "l"}) })
}

// tabStatic draws one tab in a given state, for the gallery matrix.
func tabStatic(c *ui.Context, tab TabItem, selected bool, state force.State) ui.Element {
	pl := pal(c)
	_, text := segmentLook(pl, selected, tab.Disabled, state == force.Hover)
	row := ui.Row(c).Height(theme.ControlHeight).Padding(0, theme.Space3).Gap(theme.Space2).Radius(theme.RadiusCtrl).AlignItems(ui.Center).Disabled(tab.Disabled)
	if selected && !tab.Disabled {
		row.Background(col(pl.AccentSoft))
		text = col(pl.Accent)
	}
	if selected && tab.Disabled {
		row.Background(col(pl.AccentSoft))
	}
	row.TextColor(text)
	accent, focused := col(pl.Accent), state == force.Focus && !tab.Disabled
	row.DrawOver(func(pt *ui.Painter, r ui.Rect) {
		if focused {
			inlineFocus(pt, r, theme.RadiusCtrl, accent)
		}
	})
	return row.Children(func() {
		Text(c, tab.Label, TextProps{SingleLine: true, Strong: selected, Key: "l"})
		if tab.Busy {
			loader(c.Key("busy"), theme.IconSmall, text)
		}
	})
}

// ToggleButtonProps configure ToggleButton.
type ToggleButtonProps struct {
	Icon     icon.Name
	Disabled bool
	Key      string
}

// ToggleButton is a Button-sized control that keeps its pressed state in *on.
func ToggleButton(c *ui.Context, on *bool, label string, p ToggleButtonProps) ui.Element {
	audit.Named("ToggleButton", label)
	pl, fs := pal(c), force.Take()
	b := ui.ToggleBase(keyed(c, p.Key), on).Label(label).Height(theme.ControlHeight).Padding(0, theme.Space2).Gap(theme.Space2).Radius(theme.RadiusCtrl).
		AlignItems(ui.Center).Justify(ui.Center).AlignSelf(ui.Start).FocusRing(false).Disabled(p.Disabled)
	fill, border, text := col(pl.Surface), col(pl.Border), col(pl.Text)
	if *on {
		fill, border, text = col(pl.AccentSoft), col(pl.Accent), col(pl.Accent)
	}
	switch {
	case p.Disabled:
		fill, border, text = col(pl.Subtle), col(pl.Subtle), col(pl.Secondary)
	case fs.Pressed(b.Pressed()), fs.Hovered(b.Hovered()):
		if !*on {
			fill, border = col(pl.Hover), col(pl.Hover)
		}
	}
	b.Background(fill).TextColor(text)
	focused := !p.Disabled && fs.Focused(b.FocusVisible())
	surface, accent := col(pl.Surface), col(pl.Accent)
	b.DrawOver(func(pt *ui.Painter, r ui.Rect) {
		if focused {
			focusRing(pt, r, theme.RadiusCtrl, surface, accent)
		} else {
			pt.Stroke(r, border, theme.RadiusCtrl, 1)
		}
	})
	return b.Children(func() {
		if p.Icon != "" {
			Icon(c, p.Icon, IconProps{Small: true, Key: "g"})
		}
		Text(c, label, TextProps{SingleLine: true, Key: "l"})
	})
}
