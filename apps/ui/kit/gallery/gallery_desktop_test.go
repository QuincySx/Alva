// INPUT: The gallery App, Pages(), testdata/design-components.json and ui.Tester.
// OUTPUT: Coverage gate against the design catalog, render-everything sweeps in light/dark/Narrow, accessible-name audit, toolbar and navigation interaction, and optional screenshots.
// POS: [L7 Kit tests] ALVA_KIT_SHOTS=<dir> saves one PNG per page and appearance for human review; no PNG baseline is committed.
//go:build desktop

package gallery

import (
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"image"
	"image/color"
	"image/png"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/ui/kit"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/cells"
	"github.com/QuincySx/Alva/apps/ui/kit/kittest"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

// deferred lists design components the kit does not build yet. The list only shrinks: an entry for a
// component that now has a page fails as stale, and a design component in neither place fails.
var deferred = map[string]string{}

// skippedAllowance caps the cells a page may decline; it only decreases.
var skippedAllowance = map[string]int{}

type designComponent struct {
	Name     string
	Variants int
	Axes     []struct {
		Name   string
		Values []string
	}
}

func catalog(t *testing.T) []designComponent {
	t.Helper()
	data, err := os.ReadFile("../testdata/design-components.json")
	if err != nil {
		t.Fatal(err)
	}
	var doc struct{ Components []designComponent }
	if err = json.Unmarshal(data, &doc); err != nil {
		t.Fatal(err)
	}
	return doc.Components
}

func TestGalleryCoversDesignCatalog(t *testing.T) {
	pages := map[string]kit.Page{}
	for _, p := range Pages() {
		if p.Design != "" {
			pages[p.Design] = p
		}
	}
	total, covered := 0, 0
	for _, d := range catalog(t) {
		total++
		page, built := pages[d.Name]
		reason, isDeferred := deferred[d.Name]
		switch {
		case built && isDeferred:
			t.Errorf("%s has a page but is still listed as deferred (%s): delete the deferred entry", d.Name, reason)
		case !built && !isDeferred:
			t.Errorf("design component %s is neither in the gallery nor in the deferred list", d.Name)
		case built:
			covered++
			if page.Variants() != d.Variants {
				t.Errorf("%s page has %d cells, design has %d variants", d.Name, page.Variants(), d.Variants)
			}
			if len(page.Axes) != len(d.Axes) {
				t.Errorf("%s page has %d axes, design has %d", d.Name, len(page.Axes), len(d.Axes))
				continue
			}
			for i, ax := range d.Axes {
				if page.Axes[i].Name != ax.Name || !slices.Equal(page.Axes[i].Values, ax.Values) {
					t.Errorf("%s axis %d is %v, design says %s %v", d.Name, i, page.Axes[i], ax.Name, ax.Values)
				}
			}
			if allowed := skippedAllowance[d.Name]; page.Skipped() > allowed {
				t.Errorf("%s declines %d cells, allowance %d (the allowance only shrinks)", d.Name, page.Skipped(), allowed)
			} else if page.Skipped() < allowed {
				t.Errorf("%s declines %d cells, below its allowance %d: lower the allowance", d.Name, page.Skipped(), allowed)
			}
		}
	}
	for name := range deferred {
		found := false
		for _, d := range catalog(t) {
			found = found || d.Name == name
		}
		if !found {
			t.Errorf("deferred entry %s is not a design component", name)
		}
	}
	if total != 56 {
		t.Fatalf("the design catalog has %d Alva components, the reviewed count is 56", total)
	}
	if covered+len(deferred) != total {
		t.Fatalf("%d built + %d deferred != %d design components", covered, len(deferred), total)
	}
}

func TestSkippedCellsAreOnlyDeclaredForTheMaskedKind(t *testing.T) {
	for _, p := range Pages() {
		if p.Skip == nil {
			continue
		}
		for _, sel := range p.Combinations() {
			if reason := p.Skip(sel); reason != "" && !strings.Contains(strings.Join(sel, " "), "Password") {
				t.Errorf("%s skips %v: %s", p.Name, sel, reason)
			}
		}
	}
}

func render(t *testing.T, page kit.Page, dark, narrow bool, width, height int) *ui.Tester {
	t.Helper()
	if err := theme.RegisterFonts(); err != nil {
		t.Fatal(err)
	}
	a := &App{Pages: []kit.Page{page}, Dark: dark, Narrow: narrow, Flat: true}
	tester := ui.NewTester(a.View, width, height)
	tester.SetDark(dark)
	return tester
}

func TestEveryExampleRenders(t *testing.T) {
	assertNamed, seen := kittest.Names(t)
	cells := 0
	for _, page := range Pages() {
		for _, dark := range []bool{false, true} {
			tester := render(t, page, dark, false, 1440, 2200)
			if !tester.HasText(fmt.Sprint(page.Variants()) + " 个变体") {
				t.Errorf("%s (%s) did not render its header", page.Name, darkLabel(dark))
			}
			kittest.Screenshot(t, tester, page.Name+"-"+darkLabel(dark))
			cells += page.Variants() - page.Skipped()
		}
	}
	assertNamed()
	if len(seen()) < 100 || cells < 500 {
		t.Fatalf("sweep looked too small: %d audited controls, %d cells", len(seen()), cells)
	}
}

func darkLabel(dark bool) string {
	if dark {
		return "dark"
	}
	return "light"
}

func TestMatrixCellsAreAllReachableInTheLayout(t *testing.T) {
	// Each Button variant caption appears once: the Cartesian product has no duplicates or holes.
	var button kit.Page
	for _, p := range Pages() {
		if p.Name == "Button" {
			button = p
		}
	}
	tester := render(t, button, false, false, 1440, 2200)
	if got := len(button.Combinations()); got != 112 {
		t.Fatal("Button matrix has", got, "cells")
	}
	for _, sel := range button.Combinations() {
		if !tester.HasText(caption(button, sel)) {
			t.Fatalf("caption %q not on screen", caption(button, sel))
		}
	}
}

func TestToolbarSwitchesAppearanceAndNavigationChangesPage(t *testing.T) {
	if err := theme.RegisterFonts(); err != nil {
		t.Fatal(err)
	}
	a := New()
	tester := ui.NewTester(a.View, 1440, 2400)
	if a.Dark || !tester.HasText("Alva Kit · "+a.Pages[0].Name) {
		t.Fatal("gallery did not open on the first page in light")
	}
	before := kittest.PixelAt(tester, 1440, 700, 20)
	if err := tester.Click("暗色主题"); err != nil || !a.Dark {
		t.Fatal("appearance switch did not turn dark on", err)
	}
	if after := kittest.PixelAt(tester, 1440, 700, 20); after == before {
		t.Fatal("the window did not repaint in the dark palette")
	}
	if err := tester.Click("窄视口预览"); err != nil || !a.Narrow {
		t.Fatal("Narrow switch did not toggle", err)
	}
	if err := tester.Click("Button"); err != nil || a.Pages[a.Index].Name != "Button" {
		t.Fatal("navigation did not select Button", err)
	}
	if !tester.HasText("Alva Kit · Button") || !tester.HasText("112 个变体") {
		t.Fatal("Button page did not render")
	}
	if !a.Select("Menu") || a.Select("Nope") {
		t.Fatal("Select lookup misbehaved")
	}
}

func pageApp(t *testing.T, name string) (*App, *ui.Tester) {
	t.Helper()
	if err := theme.RegisterFonts(); err != nil {
		t.Fatal(err)
	}
	cells.Reset()
	a := New()
	if !a.Select(name) {
		t.Fatal("no page", name)
	}
	a.Flat = true
	return a, ui.NewTester(a.View, 1440, 2200)
}

func TestGalleryCellsKeepTheirOwnState(t *testing.T) {
	_, tester := pageApp(t, "Input")
	tester.Key(0, ui.KeyTab) // first focusable: Text / Default
	tester.Type("你好")
	if got, _ := cells.Text("Input", []string{"Text", "Default"}, "text"); got != "你好" {
		t.Fatal("typing in a gallery cell did not stick: the sample resets every frame")
	}
	if tester.HasText("隐藏内容") {
		t.Fatal("password cells start masked")
	}
	if err := tester.Click("显示内容"); err != nil {
		t.Fatal(err)
	}
	if !tester.HasText("隐藏内容") {
		t.Fatal("the reveal toggle did not keep its state")
	}
}

func TestGalleryMenuAndSelectDemosWork(t *testing.T) {
	_, tester := pageApp(t, "Menu")
	if err := tester.Click("打开菜单"); err != nil {
		t.Fatal(err)
	}
	if err := tester.Click("导出"); err != nil {
		t.Fatal(err)
	}
	if !tester.HasText("已选择：export") {
		t.Fatal("the live menu demo did not choose an item")
	}
	_, tester = pageApp(t, "Select")
	if err := tester.Click("范围"); err != nil {
		t.Fatal(err)
	}
	if err := tester.Click("全部项目"); err != nil {
		t.Fatal(err)
	}
	if !tester.HasText("全部项目") {
		t.Fatal("the live select demo kept its old value")
	}
}

func TestGalleryFreezesAnimationUnlessAsked(t *testing.T) {
	a, tester := pageApp(t, "Button")
	if tester.FramePending() {
		t.Fatal("loaders animate by default: screenshots would not be stable")
	}
	before := sha256.Sum256(tester.Image().Pix)
	tester.Advance(300 * time.Millisecond)
	if before != sha256.Sum256(tester.Image().Pix) {
		t.Fatal("a frozen gallery still repainted its loaders")
	}
	a.Animate = true
	tester.Frame()
	tester.Advance(300 * time.Millisecond)
	if before == sha256.Sum256(tester.Image().Pix) {
		t.Fatal("the animation switch did not start the loaders")
	}
}

func TestGalleryOverlayAndChoiceDemos(t *testing.T) {
	_, tester := pageApp(t, "Dialog")
	if err := tester.Click("打开对话框"); err != nil || !tester.HasText("此操作无法撤销。") {
		t.Fatal("the dialog demo did not open", err)
	}
	tester.Key(0, ui.KeyEscape)
	if tester.HasText("此操作无法撤销。") {
		t.Fatal("Escape did not close the dialog demo")
	}
	if err := tester.Click("打开侧边栏"); err != nil || !tester.HasText("状态") {
		t.Fatal("the sheet demo did not open", err)
	}
	_, tester = pageApp(t, "Popover")
	if err := tester.Click("打开弹层"); err != nil || !tester.HasText("只看进行中") {
		t.Fatal("the popover demo did not open", err)
	}
	_, tester = pageApp(t, "Segmented")
	if err := tester.Click("按月"); err != nil {
		t.Fatal(err)
	}
	_, tester = pageApp(t, "Tabs")
	if err := tester.Click("活动"); err != nil {
		t.Fatal(err)
	}
	_, tester = pageApp(t, "Toast")
	if err := tester.Click("显示提示"); err != nil || !tester.HasText("已保存设置") {
		t.Fatal("the toast demo did not show", err)
	}
}

func TestGalleryShowsTheDesignSheetBesideTheImplementation(t *testing.T) {
	dir := t.TempDir()
	img := image.NewRGBA(image.Rect(0, 0, 60, 30))
	for x := 0; x < 60; x++ {
		for y := 0; y < 30; y++ {
			img.Set(x, y, color.RGBA{R: 0x65, G: 0x55, B: 0xCD, A: 0xFF})
		}
	}
	file, err := os.Create(filepath.Join(dir, "Button.png"))
	if err != nil {
		t.Fatal(err)
	}
	if err := png.Encode(file, img); err != nil {
		t.Fatal(err)
	}
	file.Close()

	a, tester := pageApp(t, "Button")
	if tester.HasText("设计稿 · Button") {
		t.Fatal("the design column shows without a design directory")
	}
	a, tester = pageApp(t, "Button")
	a.DesignDir = dir
	tester = ui.NewTester(a.View, 1440, 2200)
	if !tester.HasText("设计稿 · Button") || !tester.HasText("实现") || tester.HasText("没有 Button.png") {
		t.Fatal("Button page lacks the design sheet beside the implementation")
	}
	a.Select("Input")
	tester = ui.NewTester(a.View, 1440, 2200)
	if !tester.HasText("设计稿 · Input") || !tester.HasText("没有 Input.png") {
		t.Fatal("a component without an exported sheet must say how to export it")
	}
}
