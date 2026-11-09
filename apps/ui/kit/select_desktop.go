// INPUT: Alva Input.Kind=Select, SettingRow.Select and SelectOption definitions, MyGo SelectBase.
// OUTPUT: Select[T]: generic select with option descriptions, icons, disabled options, a selected check and a borderless value-row look.
// POS: [L7 Kit] Popup, arrows, Enter/Space, Escape and focus return stay in MyGo; the kit supplies options, paint and the popover recipe.
//go:build desktop

package kit

import (
	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/audit"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/force"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

// disabledOption is what assistive technology reads on an option that cannot be chosen.
const disabledOption = "不可选"

// Option is one choice of a Select.
type Option[T comparable] struct {
	Value       T
	Label       string
	Description string
	Icon        icon.Name
	Disabled    bool
}

// SelectProps configure Select. The zero value is a bordered, enabled select.
type SelectProps struct {
	// Label is the accessible name; required unless inside a Field with a Title.
	Label string
	// Placeholder shows while *selected matches no option.
	Placeholder string
	// Plain is the borderless value row used inside SettingRow.
	Plain   bool
	Invalid bool
	Problem string
	// ReadOnly shows the value in the read-only look and never opens the popup; the trigger stays focusable.
	ReadOnly bool
	Disabled bool
	// Busy swaps the chevron for the loader.
	Busy bool
	Key  string
}

// Select draws a select choosing *selected from opts and returns its trigger (Changed reports a new choice).
func Select[T comparable](c *ui.Context, selected *T, opts []Option[T], p SelectProps) ui.Element {
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
	audit.Named("Select", p.Label)
	pl, fs := pal(c), force.Take()
	invalid := p.Invalid || p.Problem != ""
	parts := ui.SelectBase(keyed(c, p.Key), selected)
	b := parts.Trigger.Label(p.Label).Height(theme.ControlHeight).Padding(0, theme.Space2).Gap(theme.Space2).Radius(theme.RadiusCtrl).
		AlignItems(ui.Center).Justify(ui.SpaceBetween).FontSize(theme.FontSizeBody).FontWeight(theme.FontWeightRegular).
		LineHeight(theme.LineNormal).FocusRing(false).Disabled(p.Disabled)
	markInvalid(b, invalid, p.Problem)
	shown, text := "", col(pl.Text)
	for _, o := range opts {
		if o.Value == *selected {
			shown = o.Label
		}
	}
	if shown == "" {
		shown, text = p.Placeholder, col(pl.Secondary)
	}
	if p.Plain {
		text = col(pl.Secondary)
	}
	b.TextColor(text)
	focused := !p.Disabled && fs.Focused(b.FocusVisible())
	surface, accent := col(pl.Surface), col(pl.Accent)
	if p.Plain {
		b.Background(col(pl.Transparent)).DrawOver(func(pt *ui.Painter, r ui.Rect) {
			if focused {
				focusRing(pt, r, theme.RadiusCtrl, surface, accent)
			}
		})
	} else {
		look := controlLook(pl, fs.Focused(b.Focused()), invalid, p.ReadOnly)
		b.Background(look.fill)
		border, width := look.border, look.width
		b.DrawOver(func(pt *ui.Painter, r ui.Rect) { pt.Stroke(r, border, theme.RadiusCtrl, width) })
	}
	b.Children(func() {
		Text(c, shown, TextProps{SingleLine: true, Key: "value"}).Grow(1).MinWidth(0)
		if p.Busy {
			loader(c.Key("busy"), theme.IconSmall, col(pl.Secondary))
		} else {
			Icon(c, icon.ChevronDown, IconProps{Small: true, Tone: ToneSecondary, Key: "chevron"})
		}
	})
	if p.ReadOnly {
		return b
	}
	parts.Popup(func(panel ui.Element) {
		popupSurface(panel, pl).Margin(theme.Space1, 0, 0)
		for i, o := range opts {
			audit.Named("SelectOption", o.Label)
			isSelected := o.Value == *selected
			content := func() { optionContent(c, o, isSelected) }
			if o.Disabled {
				// SelectBase's arrows highlight every Item, so a disabled option is shown but is not an Item:
				// the keyboard walks straight past it and it cannot be chosen.
				audit.Described("SelectOption", disabledOption)
				ui.Box(c.Key(i)).Label(o.Label).Disabled(true).FillWidth().Padding(theme.Space1, theme.Space2).Gap(theme.Space2).MinHeight(theme.ControlHeight).
					Radius(theme.RadiusCtrl).AlignItems(ui.Center).Row().TextColor(col(pl.Secondary)).Description(disabledOption).Children(content)
				continue
			}
			item := parts.Item(o.Value).Label(o.Label).Padding(theme.Space1, theme.Space2).Gap(theme.Space2).MinHeight(theme.ControlHeight).
				Radius(theme.RadiusCtrl).AlignItems(ui.Center).TextColor(col(pl.Text))
			switch {
			case item.Highlighted():
				item.Background(col(pl.AccentSoft))
			case item.Hovered():
				item.Background(col(pl.Hover))
			case isSelected:
				item.Background(col(pl.Subtle))
			}
			item.Children(content)
		}
	})
	return b
}

// optionContent builds icon, label/description column and the selected check of one option.
func optionContent[T comparable](c *ui.Context, o Option[T], selected bool) {
	if o.Icon != "" {
		Icon(c, o.Icon, IconProps{Small: true})
	}
	ui.Column(c.Key("copy")).Grow(1).MinWidth(0).Children(func() {
		Text(c, o.Label, TextProps{SingleLine: true, Key: "label"})
		if o.Description != "" {
			Text(c, o.Description, TextProps{Role: RoleCaption, Tone: ToneSecondary, SingleLine: true, Key: "description"})
		}
	})
	if selected {
		Icon(c, icon.Check, IconProps{Small: true, Tone: ToneAccent, Key: "check"})
	}
}

// selectOptionStatic draws one option in a given state without a popup, for the gallery and screenshots.
func selectOptionStatic(c *ui.Context, o Option[string], selected bool, state force.State) ui.Element {
	pl := pal(c)
	row := ui.Row(c).Width(menuMinWidth+theme.Space8).MinHeight(theme.ControlHeight).Padding(theme.Space1, theme.Space2).Gap(theme.Space2).
		Radius(theme.RadiusCtrl).AlignItems(ui.Center).Disabled(o.Disabled).TextColor(col(pl.Text))
	switch {
	case o.Disabled:
		row.TextColor(col(pl.Secondary))
	case state == force.Focus:
		row.Background(col(pl.AccentSoft))
	case state == force.Hover || state == force.Pressed:
		row.Background(col(pl.Hover))
	case selected:
		row.Background(col(pl.Subtle))
	}
	return row.Children(func() { optionContent(c, o, selected) })
}
