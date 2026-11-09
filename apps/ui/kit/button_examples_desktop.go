// INPUT: Alva Button (Kind x State x Content) and IconButton (State) axes.
// OUTPUT: Gallery pages with all 112 and 12 variants.
// POS: [L7 Kit] Loading/LoadingFocus are Busy (+ forced focus); Hover/Pressed/Focus are forced through internal/force, never Props.
//go:build desktop

package kit

import (
	"strings"

	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/force"
	"github.com/egoist/mygo/ui"
)

// stateOf maps a design State value to the forced runtime state and the Props bools.
func stateOf(state string) (forced force.State, busy, disabled bool) {
	state = strings.TrimPrefix(state, "Selected")
	switch state {
	case "Hover":
		forced = force.Hover
	case "Pressed":
		forced = force.Pressed
	case "Focus":
		forced = force.Focus
	case "Disabled":
		disabled = true
	case "Loading":
		busy = true
	case "LoadingFocus":
		busy, forced = true, force.Focus
	}
	return
}

func buttonPage() Page {
	return Page{
		Name: "Button", Design: "Button", Category: "动作",
		Axes: []Axis{
			axis("Kind", "Primary", "Secondary", "Ghost", "Danger"),
			axis("State", "Default", "Hover", "Pressed", "Focus", "Disabled", "Loading", "LoadingFocus"),
			axis("Content", "Text", "Leading", "Trailing", "Icon"),
		},
		Cell: func(c *ui.Context, sel []string) {
			p := ButtonProps{Kind: map[string]ButtonKind{"Primary": Primary, "Secondary": Secondary, "Ghost": Ghost, "Danger": Danger}[sel[0]], Key: "b"}
			forced, busy, disabled := stateOf(sel[1])
			p.Busy, p.Disabled = busy, disabled
			switch sel[2] {
			case "Leading":
				p.Icon = icon.Plus
			case "Trailing":
				p.TrailingIcon = icon.ArrowRight
			case "Icon":
				p.Icon, p.IconOnly = icon.Plus, true
			}
			force.With(forced, func() { Button(c, "创建任务", p) })
		},
	}
}

func iconButtonPage() Page {
	return Page{
		Name: "IconButton", Design: "IconButton", Category: "动作",
		Axes: []Axis{axis("State", "Default", "Hover", "Pressed", "Focus", "Disabled", "Loading", "LoadingFocus",
			"Selected", "SelectedHover", "SelectedPressed", "SelectedFocus", "SelectedDisabled")},
		Cell: func(c *ui.Context, sel []string) {
			forced, busy, disabled := stateOf(sel[0])
			p := IconButtonProps{Selected: strings.HasPrefix(sel[0], "Selected"), Busy: busy, Disabled: disabled, Key: "b"}
			force.With(forced, func() { IconButton(c, icon.Panel, "切换面板", p) })
		},
	}
}
