// INPUT: kit.Pages() (one page per component with the design's axes), the design tokens and, when ALVA_KIT_DESIGN_DIR is set, the design PNG sheets exported by docs/design/alva/source/next/export_design.py.
// OUTPUT: App: a native view with a component list, an appearance switch, a Narrow-viewport switch and the variant matrix of the selected page, beside the design sheet ("设计稿" next to "实现") when a design directory is set.
// POS: [L7 Kit] Visual review entry for Alva Kit; cmd/alva-kit-gallery runs it in a window and its tests render every page light and dark (ALVA_KIT_SHOTS saves PNGs, never committed).
//go:build desktop

package gallery

import (
	"os"
	"path/filepath"
	"slices"
	"strconv"
	"strings"

	"github.com/QuincySx/Alva/apps/ui/kit"
	"github.com/QuincySx/Alva/apps/ui/kit/blocks"
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
	// DesignDir holds <Component>.png sheets exported from the design package (smallpen export --component X --format png).
	// When set, every page shows the design sheet beside the implementation. They are local review files, never committed.
	DesignDir string
	sheets    map[string]sheet
}

// Pages is every kit and blocks page in gallery order.
func Pages() []kit.Page { return append(kit.Pages(), blocks.Pages()...) }

// designColumnWidth is the width of the design-sheet column in the comparison view.
const designColumnWidth = 520

type sheet struct {
	img ui.ImageSource
	err string
}

// New shows the first component. ALVA_KIT_DESIGN_DIR turns on the design comparison.
func New() *App { return &App{Pages: Pages(), DesignDir: os.Getenv("ALVA_KIT_DESIGN_DIR")} }

// designSheet loads and caches the exported PNG of a component.
func (a *App) designSheet(name string) sheet {
	if s, ok := a.sheets[name]; ok {
		return s
	}
	var s sheet
	if data, err := os.ReadFile(filepath.Join(a.DesignDir, name+".png")); err != nil {
		s.err = "没有 " + name + ".png：运行 docs/design/alva/source/next/export_design.py " + a.DesignDir
	} else if img, err := ui.DecodeImage(data); err != nil {
		s.err = name + ".png 无法解码：" + err.Error()
	} else {
		s.img = img
	}
	if a.sheets == nil {
		a.sheets = map[string]sheet{}
	}
	a.sheets[name] = s
	return s
}

// withDesign shows the matrix, with the design sheet in a left column when a design directory is set.
func (a *App) withDesign(c *ui.Context, page kit.Page, pl theme.Palette) {
	if a.DesignDir == "" {
		matrix(c, page, pl)
		return
	}
	ui.Row(c.Key("compare")).FillWidth().Gap(theme.Space6).AlignItems(ui.Start).Children(func() {
		ui.Column(c.Key("design")).Width(designColumnWidth).Shrink(0).Gap(theme.Space2).Children(func() {
			kit.Text(c, "设计稿 · "+page.Name, kit.TextProps{Role: kit.RoleSubtitle, Strong: true, Key: "design-title"})
			s := a.designSheet(page.Name)
			if s.img == nil {
				kit.Text(c, s.err, kit.TextProps{Tone: kit.ToneWarning, Key: "design-missing"})
				return
			}
			ui.Image(c.Key("design-sheet"), s.img).FillWidth().Fit(ui.Contain)
		})
		ui.Column(c.Key("impl")).Grow(1).MinWidth(0).Gap(theme.Space2).Children(func() {
			kit.Text(c, "实现", kit.TextProps{Role: kit.RoleSubtitle, Strong: true, Key: "impl-title"})
			matrix(c, page, pl)
		})
	})
}

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
	kit.ToastHost(c)
	if a.Flat {
		ui.Column(c).Fill().Padding(theme.Space4).Background(theme.UI(pl.Canvas)).Children(func() { a.withDesign(c, page, pl) })
		return
	}
	ui.Row(c).Fill().Background(theme.UI(pl.Canvas)).Children(func() {
		a.list(c, pl)
		ui.Column(c.Key("main")).Grow(1).MinWidth(0).Children(func() {
			a.toolbar(c, page, pl)
			ui.Scroll(c.Key("scroll")).Grow(1).Padding(theme.Space4).Children(func() {
				col := ui.Column(c.Key("matrix"))
				if a.Narrow {
					col.MaxWidth(theme.SizeViewportWidth(true) - theme.SizeNavigation)
				}
				col.Children(func() { a.withDesign(c, page, pl) })
			})
		})
	})
}

func (a *App) list(c *ui.Context, pl theme.Palette) {
	ui.Scroll(c.Key("list")).Width(theme.SizeNavigation).Shrink(0).Background(theme.UI(pl.Sidebar)).Padding(theme.Space3).Children(func() {
		ui.Column(c.Key("pages")).Gap(theme.Space1).Children(func() {
			var categories []string
			for _, p := range a.Pages {
				if !slices.Contains(categories, p.Category) {
					categories = append(categories, p.Category)
				}
			}
			for _, category := range categories {
				kit.Text(c, category, kit.TextProps{Role: kit.RoleCaption, Tone: kit.ToneSecondary, Key: "cat-" + category}).Margin(theme.Space2, 0, 0, theme.Space2)
				for i, p := range a.Pages {
					if p.Category != category {
						continue
					}
					kind := kit.Ghost
					if i == a.Index {
						kind = kit.Secondary
					}
					index := i
					kit.Button(c, p.Name, kit.ButtonProps{Kind: kind, OnClick: func() { a.Index = index }, Key: "page-" + p.Name}).FillWidth().Justify(ui.Start)
				}
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
