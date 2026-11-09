// INPUT: Kit Input, TextArea and Field under ui.Tester with real typing, Tab focus and forced states.
// OUTPUT: Interaction, geometry and pixel proof for the Input and TextArea states of the design (Default/Focus/Error/Disabled/Loading/ErrorFocus/ReadOnly).
// POS: [L7 Kit tests] Pixel checks read the border row of the control box, so a regression in the 1px/2px role colors fails here.
//go:build desktop

package kit

import (
	"fmt"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/force"
	"github.com/QuincySx/Alva/apps/ui/kit/kittest"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

// edge samples the top border row of a control and the fill inside its padding.
func edge(tt *ui.Tester, width int, r ui.Rect, padLeft float32) (border, fill ui.Color) {
	border = kittest.PixelAt(tt, width, r.X+r.W/2, r.Y+.5)
	fill = kittest.PixelAt(tt, width, r.X-padLeft/2, r.Y+r.H/2)
	return
}

func TestInputGeometryTypingAndFillByAppearance(t *testing.T) {
	for _, dark := range []bool{false, true} {
		value := ""
		tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
			Input(c, &value, InputProps{Label: "名称", Placeholder: "输入名称", Key: "name"})
		}), 360, 120)
		tester.SetDark(dark)
		r, ok := tester.Find("名称")
		if !ok || r.H != theme.ControlHeight {
			t.Fatal("input missing or not 32px", r)
		}
		p := theme.For(dark)
		border, fill := edge(tester, 360, r, theme.Space2)
		if !kittest.Near(border, theme.UI(p.Border)) || !kittest.Near(fill, theme.UI(p.Subtle)) {
			t.Fatalf("default input border %v fill %v, want %v / %v", border, fill, theme.UI(p.Border), theme.UI(p.Subtle))
		}
		tester.Key(0, ui.KeyTab)
		tester.Type("Alva")
		if value != "Alva" || !tester.Focused("名称") {
			t.Fatal("typing did not reach the value", value)
		}
		border, fill = edge(tester, 360, r, theme.Space2)
		if !kittest.Near(border, theme.UI(p.Accent)) || !kittest.Near(fill, theme.UI(p.Surface)) {
			t.Fatalf("focused input border %v fill %v, want accent %v on surface", border, fill, theme.UI(p.Accent))
		}
	}
}

func TestInputInvalidReadOnlyDisabledAndBusy(t *testing.T) {
	var invalid, readOnly, disabled, busy = "x", "只读内容", "停用", "加载"
	view := func(c *ui.Context) {
		ui.Column(c).Gap(theme.Space3).Children(func() {
			Input(c, &invalid, InputProps{Label: "错误", Invalid: true, Problem: "名称已存在", Key: "a"})
			Input(c, &readOnly, InputProps{Label: "只读", ReadOnly: true, Key: "b"})
			Input(c, &disabled, InputProps{Label: "停用", Disabled: true, Key: "c"})
			Input(c, &busy, InputProps{Label: "加载", Busy: true, Leading: icon.Search, Key: "d"})
		})
	}
	tester := ui.NewTester(kittest.Centered(view), 360, 280)
	p := theme.Light
	r, _ := tester.Find("错误")
	if border, _ := edge(tester, 360, r, theme.Space2); !kittest.Near(border, theme.UI(p.Danger)) {
		t.Fatal("invalid input lost the danger border", border)
	}
	r, _ = tester.Find("只读")
	if _, fill := edge(tester, 360, r, theme.Space2); !kittest.Near(fill, theme.UI(p.Surface)) {
		t.Fatal("read-only input is not the surface-filled value look", fill)
	}
	tester.Key(0, ui.KeyTab)
	tester.Key(0, ui.KeyTab)
	if !tester.Focused("只读") {
		t.Fatal("read-only input must stay focusable")
	}
	tester.Type("改")
	if readOnly != "只读内容" {
		t.Fatal("read-only input accepted typing", readOnly)
	}
	tester.Key(0, ui.KeyTab)
	if tester.Focused("停用") || !tester.Focused("加载") {
		t.Fatal("disabled input took Tab focus")
	}
	if !tester.HasText("Loading indicator") {
		t.Fatal("busy input has no loader")
	}
	tester.Type("中")
	// Tab selects the whole value, so typing replaces it: what matters is that a busy input still edits.
	if busy != "中" {
		t.Fatal("busy input stopped accepting text", busy)
	}
}

func TestInputInvalidFocusUsesTheThickDangerBorder(t *testing.T) {
	value := ""
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		Input(c, &value, InputProps{Label: "邮箱", Invalid: true, Key: "mail"})
	}), 360, 120)
	tester.Key(0, ui.KeyTab)
	r, _ := tester.Find("邮箱")
	// A 2px stroke covers the second row too; the 1px default would leave it as the fill.
	row2 := kittest.PixelAt(tester, 360, r.X+r.W/2, r.Y+1.5)
	if !kittest.Near(row2, theme.UI(theme.Light.Danger)) {
		t.Fatal("ErrorFocus did not draw a 2px danger border", row2)
	}
}

func TestInputForcedFocusMatchesRealFocusPixels(t *testing.T) {
	value := ""
	mk := func(forced bool) *ui.Tester {
		return ui.NewTester(kittest.Centered(func(c *ui.Context) {
			build := func() { Input(c, &value, InputProps{Label: "强制", Key: "f"}) }
			if forced {
				force.With(force.Focus, build)
			} else {
				build()
			}
		}), 360, 120)
	}
	real, forced := mk(false), mk(true)
	real.Key(0, ui.KeyTab)
	r, _ := real.Find("强制")
	a, _ := edge(real, 360, r, theme.Space2)
	b, _ := edge(forced, 360, r, theme.Space2)
	if !kittest.Near(a, b) {
		t.Fatal("forced focus differs from real focus", a, b)
	}
}

func TestFieldInjectsNameStatesAndMessages(t *testing.T) {
	value := ""
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		Field(c, FieldProps{Title: "API 地址", Error: "地址无效", Disabled: false, Key: "field"}, func() {
			Input(c, &value, InputProps{Key: "url"})
		})
	}), 360, 160)
	if _, ok := tester.Find("API 地址"); !ok {
		t.Fatal("Field title missing")
	}
	if !tester.HasText("地址无效") {
		t.Fatal("Field error text missing")
	}
	tester.Key(0, ui.KeyTab)
	tester.Type("x")
	if value != "x" {
		t.Fatal("the field's control did not take input", value)
	}
	// The control's name came from the Field's Title.
	assertNamed, entries := kittest.Names(t)
	ui.NewTester(kittest.Centered(func(c *ui.Context) {
		Field(c, FieldProps{Title: "名称"}, func() { Input(c, &value, InputProps{Key: "n"}) })
	}), 200, 120)
	assertNamed()
	if len(entries()) == 0 {
		t.Fatal("no audit entries")
	}
}

func TestFieldDisabledDisablesTheControl(t *testing.T) {
	value := "固定"
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		Field(c, FieldProps{Title: "只读字段", Disabled: true}, func() { Input(c, &value, InputProps{Key: "x"}) })
	}), 360, 140)
	tester.Key(0, ui.KeyTab)
	if tester.Focused("只读字段") {
		t.Fatal("a disabled Field's control took focus")
	}
}

func TestTextAreaTypingStatesAndGrowth(t *testing.T) {
	value, invalid := "", ""
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		ui.Column(c).Gap(theme.Space3).Children(func() {
			TextArea(c, &value, TextAreaProps{Label: "说明", Placeholder: "写点什么", Key: "t"})
			TextArea(c, &invalid, TextAreaProps{Label: "错误说明", Invalid: true, Key: "e"})
		})
	}), 360, 360)
	tester.Key(0, ui.KeyTab)
	tester.Type("第一行")
	if value != "第一行" {
		t.Fatal("text area did not take typing", value)
	}
	r, _ := tester.Find("说明")
	if border := kittest.PixelAt(tester, 360, r.X+r.W/2, r.Y+.5); !kittest.Near(border, theme.UI(theme.Light.Accent)) {
		t.Fatal("focused text area lost the accent border", border)
	}
	e, _ := tester.Find("错误说明")
	if border := kittest.PixelAt(tester, 360, e.X+e.W/2, e.Y+.5); !kittest.Near(border, theme.UI(theme.Light.Danger)) {
		t.Fatal("invalid text area lost the danger border", border)
	}
}

// inputInk fingerprints the pixels at the start of an input's text area (the
// left 60%, away from the trailing toggle and its tooltip), so masked and
// revealed renderings can be told apart without reading the field's value.
func inputInk(tt *ui.Tester, width int, r ui.Rect) string {
	img := tt.Image()
	var b strings.Builder
	for y := int(r.Y + 4); y < int(r.Y+r.H-4); y += 2 {
		for x := int(r.X); x < int(r.X+r.W*0.6) && x < width; x += 2 {
			c := img.RGBAAt(x, y)
			fmt.Fprintf(&b, "%02x%02x%02x", c.R, c.G, c.B)
		}
	}
	return b.String()
}

func TestPasswordMasksUntilRevealedAndNamesItsToggle(t *testing.T) {
	value, reveal := "", false
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		Password(c, &value, &reveal, InputProps{Label: "API Key", Key: "key"})
	}), 360, 120)
	r, ok := tester.Find("API Key")
	if !ok || r.H != theme.ControlHeight {
		t.Fatal("password input missing or not 32px", r)
	}
	tester.Key(0, ui.KeyTab)
	tester.Type("sample-secret")
	if value != "sample-secret" {
		t.Fatal("typing did not reach the value", value)
	}
	// Move focus to the toggle so the caret does not differ between captures.
	tester.Key(0, ui.KeyTab)
	tester.Frame()
	masked := inputInk(tester, 360, r)
	if err := tester.Click("显示内容"); err != nil {
		t.Fatal("reveal toggle missing its accessible name", err)
	}
	tester.Frame()
	revealed := inputInk(tester, 360, r)
	if !reveal || revealed == masked {
		t.Fatal("reveal did not change the rendered text", reveal)
	}
	if err := tester.Click("隐藏内容"); err != nil || reveal {
		t.Fatal("hide toggle did not mask again", err, reveal)
	}
	tester.Frame()
	if inputInk(tester, 360, r) == revealed {
		t.Fatal("masking again still rendered the revealed text")
	}
}

func TestPasswordDisabledDisablesItsToggle(t *testing.T) {
	value, reveal := "sample-secret", false
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		Password(c, &value, &reveal, InputProps{Label: "API Key", Disabled: true, Key: "key"})
	}), 360, 120)
	_ = tester.Click("显示内容")
	tester.Frame()
	if reveal {
		t.Fatal("a disabled password revealed its text")
	}
}
