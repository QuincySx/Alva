// INPUT: kit.Pages() (one page per component with the design's axes) and the design tokens.
// OUTPUT: App: a native view with a component list, an appearance switch, a Narrow-viewport switch and the variant matrix of the selected page.
// POS: [L7 Kit] Visual review entry for Alva Kit; cmd/alva-kit-gallery runs it in a window and its tests render every page light and dark (ALVA_KIT_SHOTS saves PNGs, never committed).
//go:build desktop

package gallery

import (
	"strconv"
	"strings"

	"github.com/QuincySx/Alva/apps/ui/kit"
	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

// App is the gallery's state. The zero value is not usable; call New.
type App struct {
	Pages []kit.Page
	// Index selects the page shown.
	Index int
	// Dark and Narrow are the toolbar switches.
	Dark, Narrow bool
	// Animate plays the loader and MyGo transitions; the default is frozen so screenshots and review are stable.
	Animate bool
	// Flat renders only the matrix, without list, toolbar or scrolling, for screenshots.
	Flat bool
}

// New shows the first component.
func New() *App { return &App{Pages: kit.Pages()} }

// Select shows the page of the named component and reports whether it exists.
func (a *App) Select(name string) bool {
	for i, p := range a.Pages {
		if p.Name == name {
			a.Index = i
			return true
		}
	}
	return false
}

// View builds the window.
func (a *App) View(c *ui.Context) {
	appearance := theme.ForceLight
	if a.Dark {
		appearance = theme.ForceDark
	}
	theme.Apply(c, theme.Options{Appearance: appearance, ReduceMotion: !a.Animate})
	pl := theme.Of(c)
	page := a.Pages[a.Index]
	if a.Flat {
		ui.Column(c).Fill().Padding(theme.Space4).Background(theme.UI(pl.Canvas)).Children(func() { matrix(c, page, pl) })
		return
	}
	ui.Row(c).Fill().Background(theme.UI(pl.Canvas)).Children(func() {
		a.list(c, pl)
		ui.Column(c.Key("main")).Grow(1).MinWidth(0).Children(func() {
			a.toolbar(c, page, pl)
			ui.Scroll(c.Key("scroll")).Grow(1).Padding(theme.Space4).Children(func() {
				col := ui.Column(c.Key("matrix"))
				if a.Narrow {
					col.MaxWidth(theme.SizeViewportWidth(true) - theme.SizeNavigation(true))
				}
				col.Children(func() { matrix(c, page, pl) })
			})
		})
	})
}

func (a *App) list(c *ui.Context, pl theme.Palette) {
	ui.Scroll(c.Key("list")).Width(theme.SizeNavigation(a.Narrow)).Shrink(0).Background(theme.UI(pl.Sidebar)).Padding(theme.Space3).Children(func() {
		ui.Column(c.Key("pages")).Gap(theme.Space1).Children(func() {
			last := ""
			for i, p := range a.Pages {
				if p.Category != last {
					last = p.Category
					kit.Text(c, p.Category, kit.TextProps{Role: kit.RoleCaption, Tone: kit.ToneSecondary, Key: "cat-" + p.Category}).Margin(theme.Space2, 0, 0, theme.Space2)
				}
				kind := kit.Ghost
				if i == a.Index {
					kind = kit.Secondary
				}
				index := i
				kit.Button(c, p.Name, kit.ButtonProps{Kind: kind, OnClick: func() { a.Index = index }, Key: "page-" + p.Name}).FillWidth().Justify(ui.Start)
			}
		})
	})
}

func (a *App) toolbar(c *ui.Context, page kit.Page, pl theme.Palette) {
	ui.Row(c.Key("toolbar")).Height(theme.SizeHeader).Shrink(0).Padding(0, theme.Space4).Gap(theme.Space4).AlignItems(ui.Center).
		Background(theme.UI(pl.Surface)).Border(0, theme.UI(pl.Border)).Children(func() {
		kit.Icon(c, icon.Sparkles, kit.IconProps{Tone: kit.ToneAccent, Key: "logo"})
		kit.Text(c, "Alva Kit · "+page.Name, kit.TextProps{Role: kit.RoleSubtitle, Strong: true, SingleLine: true, Key: "title"})
		ui.Spacer(c).Grow(1)
		kit.Text(c, "暗色", kit.TextProps{Tone: kit.ToneSecondary, Key: "dark-label"})
		kit.Switch(c, &a.Dark, "暗色主题", kit.SwitchProps{Key: "dark"})
		kit.Text(c, "窄视口", kit.TextProps{Tone: kit.ToneSecondary, Key: "narrow-label"})
		kit.Switch(c, &a.Narrow, "窄视口预览", kit.SwitchProps{Key: "narrow"})
		kit.Text(c, "动画", kit.TextProps{Tone: kit.ToneSecondary, Key: "animate-label"})
		kit.Switch(c, &a.Animate, "播放动画", kit.SwitchProps{Key: "animate"})
	})
}

// caption names a cell by its axis values.
func caption(page kit.Page, sel []string) string {
	if len(sel) == 0 {
		return page.Name
	}
	return strings.Join(sel, " · ")
}

func matrix(c *ui.Context, page kit.Page, pl theme.Palette) {
	ui.Column(c.Key("page")).FillWidth().Gap(theme.Space3).Children(func() {
		heading := page.Name
		if page.Design != "" {
			heading += "  ·  设计组件 " + page.Design
		}
		kit.Text(c, heading, kit.TextProps{Role: kit.RoleHeading, Strong: true, SingleLine: true, Key: "heading"})
		axes := make([]string, len(page.Axes))
		for i, ax := range page.Axes {
			axes[i] = ax.Name + " " + itoa(len(ax.Values))
		}
		summary := itoa(page.Variants()) + " 个变体"
		if len(axes) > 0 {
			summary += "（" + strings.Join(axes, " × ") + "）"
		}
		kit.Text(c, summary, kit.TextProps{Role: kit.RoleCaption, Tone: kit.ToneSecondary, Key: "summary"})
		ui.Row(c.Key("cells")).FillWidth().Wrap().Gap(theme.Space3).Children(func() {
			for i, sel := range page.Combinations() {
				cell(c.Key(i), page, sel, pl)
			}
		})
		if page.Live != nil {
			kit.Text(c, "交互演示（真实输入、切换与菜单）", kit.TextProps{Role: kit.RoleSubtitle, Strong: true, Key: "live-title"})
			ui.Box(c.Key("live")).Padding(theme.Space3).Radius(theme.RadiusCtrl).Background(theme.UI(pl.Surface)).Border(1, theme.UI(pl.Border)).Children(func() {
				ui.Column(c.Key("live-col")).Gap(theme.Space2).Children(func() { page.Live(c.Key("live-body")) })
			})
		}
	})
}

func cell(c *ui.Context, page kit.Page, sel []string, pl theme.Palette) {
	ui.Column(c).Gap(theme.Space1).Shrink(0).Children(func() {
		kit.Text(c, caption(page, sel), kit.TextProps{Role: kit.RoleCaption, Tone: kit.ToneSecondary, SingleLine: true, Key: "caption"})
		ui.Box(c.Key("swatch")).Padding(theme.Space3).Radius(theme.RadiusCtrl).Background(theme.UI(pl.Surface)).Border(1, theme.UI(pl.Border)).Children(func() {
			if page.Skip != nil {
				if reason := page.Skip(sel); reason != "" {
					kit.Text(c, "未实现", kit.TextProps{Tone: kit.ToneWarning, Key: "skipped"})
					return
				}
			}
			page.Cell(c.Key("sample"), sel)
		})
	})
}

func itoa(n int) string { return strconv.Itoa(n) }
