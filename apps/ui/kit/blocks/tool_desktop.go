// INPUT: Alva ToolActivity (Collapsed/Running/Error/Expanded/Focus/ExpandedFocus/ErrorExpanded) definition.
// OUTPUT: ToolActivity: an expandable one-line record of something the assistant did, driven by a kind enum, never by a tool name.
// POS: [L7 Kit blocks] ADR 0013 section 3: the interface renders kinds. The product layer maps its tool or engine facts to ToolKind (K4).
//go:build desktop

package blocks

import (
	"github.com/QuincySx/Alva/apps/ui/kit"
	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/audit"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/force"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

// ToolKind is the kind of activity; Command is the zero value. The set is closed and presentation-only.
type ToolKind uint8

const (
	ToolCommand ToolKind = iota
	ToolReadFile
	ToolEditFile
	ToolSearch
	ToolSubAgent
	ToolTodo
	ToolOther
)

// ToolStatus is how the activity ended or whether it is still going; Done is the zero value.
type ToolStatus uint8

const (
	ToolDone ToolStatus = iota
	ToolRunning
	ToolError
)

// ToolActivityView is the data of ToolActivity.
type ToolActivityView struct {
	Kind   ToolKind
	Title  string
	Detail string
	Status ToolStatus
	// Output is the text shown when expanded (a log, a diff summary, an error message).
	Output string
}

func toolGlyph(k ToolKind) icon.Name {
	switch k {
	case ToolCommand:
		return icon.Terminal
	case ToolReadFile:
		return icon.File
	case ToolEditFile:
		return icon.Edit
	case ToolSearch:
		return icon.Search
	case ToolSubAgent:
		return icon.Agent
	case ToolTodo:
		return icon.Task
	}
	return icon.Sparkles // ToolOther and any value outside the closed set
}

const (
	// outputMaxHeight is the tallest the output area grows; longer text scrolls inside it.
	outputMaxHeight = 240
	titleMaxBytes   = 128
)

// ToolActivity draws the header row and, while *expanded, the output below it. onToggle is called with the new
// expanded value when the header is activated; the caller owns the state, so expansion survives rebuilds.
func ToolActivity(c *ui.Context, v ToolActivityView, expanded bool, onToggle func(bool), key string) ui.Element {
	v.Title, v.Detail = oneLine(v.Title, titleMaxBytes), oneLine(v.Detail, lineMaxBytes)
	audit.Named("ToolActivity", v.Title)
	pl, fs := pal(c), force.Take()
	return ui.Column(keyed(c, key)).FillWidth().Children(func() {
		head := ui.ButtonBase(c.Key("head")).Label(v.Title).Expanded(expanded).FillWidth().Height(theme.ControlHeight).Padding(0, theme.Space2).Gap(theme.Space2).
			Radius(theme.RadiusCtrl).AlignItems(ui.Center).FocusRing(false)
		switch {
		case fs.Pressed(head.Pressed()):
			head.Background(col(pl.Hover))
		case fs.Hovered(head.Hovered()):
			head.Background(col(pl.Subtle))
		}
		focused, accent := fs.Focused(head.FocusVisible()), col(pl.Accent)
		head.DrawOver(func(pt *ui.Painter, r ui.Rect) {
			if focused {
				kit.InlineFocus(pt, r, theme.RadiusCtrl, accent)
			}
		})
		if onToggle != nil {
			open := expanded
			head.OnClick(func() { onToggle(!open) })
		}
		head.Children(func() {
			tone := kit.ToneSecondary
			if v.Status == ToolError {
				tone = kit.ToneDanger
			}
			kit.Icon(c, toolGlyph(v.Kind), kit.IconProps{Small: true, Tone: tone, Key: "g"})
			kit.Text(c, v.Title, kit.TextProps{SingleLine: true, Key: "title"})
			if v.Detail != "" {
				kit.Text(c, v.Detail, kit.TextProps{Role: kit.RoleMono, Tone: kit.ToneSecondary, SingleLine: true, Key: "detail"}).Grow(1).MinWidth(0)
			} else {
				ui.Spacer(c).Grow(1)
			}
			switch v.Status {
			case ToolRunning:
				kit.Loader(c.Key("busy"), theme.IconSmall, kit.ToneAccent)
			case ToolError:
				kit.Badge(c, "失败", kit.BadgeProps{Tone: kit.BadgeDanger, Key: "badge"})
			}
			chevron := icon.ChevronRight
			if expanded {
				chevron = icon.ChevronDown
			}
			kit.Icon(c, chevron, kit.IconProps{Small: true, Tone: kit.ToneSecondary, Key: "chevron"})
		})
		if expanded && v.Output != "" {
			border := col(pl.Border)
			if v.Status == ToolError {
				border = col(pl.Danger)
			}
			output, dropped := clip(v.Output, outputMaxLines, outputMaxBytes)
			ui.Scroll(c.Key("out")).FillWidth().MaxHeight(outputMaxHeight).Margin(theme.Space1, 0, 0).Radius(theme.RadiusCtrl).Background(col(pl.Subtle)).Border(1, border).Children(func() {
				ui.Column(c.Key("body")).FillWidth().Padding(theme.Space2, theme.Space3).Children(func() {
					kit.Text(c, output, kit.TextProps{Role: kit.RoleMono, Key: "text"}).Selectable()
					if dropped > 0 {
						kit.Text(c, "已截断 "+itoa(dropped)+" 行", kit.TextProps{Role: kit.RoleCaption, Tone: kit.ToneSecondary, Key: "truncated"})
					}
				})
			})
		}
	})
}
