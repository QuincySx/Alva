// INPUT: Alva Checkbox (State 9) and Switch (State 6) axes.
// OUTPUT: Gallery pages with every design variant.
// POS: [L7 Kit] Mixed is the caller-owned Props bool; Focus variants are forced focus.
//go:build desktop

package kit

import (
	"strings"

	"github.com/QuincySx/Alva/apps/ui/kit/internal/force"
	"github.com/egoist/mygo/ui"
)

func toggleState(s string) (on, mixed, focus, disabled bool) {
	return strings.HasPrefix(s, "On"), strings.HasPrefix(s, "Mixed"), strings.HasSuffix(s, "Focus"), strings.HasSuffix(s, "Disabled")
}

func checkboxPage() Page {
	return Page{
		Name: "Checkbox", Design: "Checkbox", Category: "选择",
		Axes: []Axis{axis("State", "Off", "On", "Mixed", "Focus", "Disabled", "OnFocus", "OnDisabled", "MixedFocus", "MixedDisabled")},
		Cell: func(c *ui.Context, sel []string) {
			on0, mixed0, focus, disabled := toggleState(sel[0])
			on, mixed := cellSlot("Checkbox", sel, "on", on0), cellSlot("Checkbox", sel, "mixed", mixed0)
			forced := force.None
			if focus || sel[0] == "Focus" {
				forced = force.Focus
			}
			if sel[0] == "Disabled" {
				disabled = true
			}
			force.With(forced, func() {
				if Checkbox(c, on, "选择此项", CheckboxProps{Mixed: *mixed, Disabled: disabled, Key: "c"}).Changed() {
					*mixed = false
				}
			})
		},
	}
}

func switchPage() Page {
	return Page{
		Name: "Switch", Design: "Switch", Category: "选择",
		Axes: []Axis{axis("State", "Off", "On", "Focus", "Disabled", "OnFocus", "OnDisabled")},
		Cell: func(c *ui.Context, sel []string) {
			on0, _, focus, disabled := toggleState(sel[0])
			on := cellSlot("Switch", sel, "on", on0)
			forced := force.None
			if focus || sel[0] == "Focus" {
				forced = force.Focus
			}
			if sel[0] == "Disabled" {
				disabled = true
			}
			force.With(forced, func() { Switch(c, on, "启用", SwitchProps{Disabled: disabled, Key: "s"}) })
		},
	}
}
