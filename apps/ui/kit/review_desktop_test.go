// INPUT: Kit components under ui.Tester after the K1 review and trial feedback.
// OUTPUT: Regression tests: forced state scope, inline action look and focus, busy password order, tooltip/busy description, nested Field, audit rules, select semantics.
// POS: [L7 Kit tests] Each test names the finding it pins; the severe ones are also mutation-verified.
//go:build desktop

package kit

import (
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/audit"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/force"
	"github.com/QuincySx/Alva/apps/ui/kit/kittest"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

func passwordTester(forced force.State, p InputProps, value string, reveal bool) *ui.Tester {
	return ui.NewTester(kittest.Centered(func(c *ui.Context) {
		force.With(forced, func() { Password(c, &value, &reveal, p) })
	}), 320, 120)
}

func TestForcedFocusStaysOnThePrimaryElement(t *testing.T) {
	tester := passwordTester(force.Focus, InputProps{Label: "密钥", Key: "k", Width: 260}, "", false)
	eye, ok := tester.Find("显示内容")
	if !ok {
		t.Fatal("reveal action missing")
	}
	// Where the eye's own inset focus stroke would run: it must not exist, only the box border focuses.
	stroke := kittest.PixelAt(tester, 320, eye.X+eye.W/2, eye.Y+theme.Space1+.5)
	if kittest.Near(stroke, theme.UI(theme.Light.Accent)) {
		t.Fatal("forced focus leaked into the nested reveal action: double focus ring")
	}
	in, _ := tester.Find("密钥")
	if border := kittest.PixelAt(tester, 320, in.X+in.W/2, in.Y+.5); !kittest.Near(border, theme.UI(theme.Light.Accent)) {
		t.Fatal("the input itself lost the forced focus border", border)
	}
	if force.Current() != force.None {
		t.Fatal("force leaked past its scope")
	}
}

func TestInlineActionIsLightweightAndFocusesInsideTheBox(t *testing.T) {
	tester := passwordTester(force.None, InputProps{Label: "密钥", Key: "k", Width: 260}, "", false)
	eye, _ := tester.Find("显示内容")
	if eye.H != theme.ControlHeight || eye.W != theme.IconSmall+theme.Space2 {
		t.Fatal("inline action is not input-high and icon-narrow", eye)
	}
	fill := kittest.PixelAt(tester, 320, eye.X+1, eye.Y+eye.H/2)
	rest := kittest.Ink(tester, 320, eye, fill)
	tester.Move(eye.X+eye.W/2, eye.Y+eye.H/2)
	hovered := kittest.PixelAt(tester, 320, eye.X+1, eye.Y+eye.H/2)
	if !kittest.Near(fill, hovered) || !kittest.Near(fill, theme.UI(theme.Light.Subtle)) {
		t.Fatal("hover painted a background on the inline action", fill, hovered)
	}
	if got := kittest.Ink(tester, 320, eye, fill); kittest.Near(got, rest) {
		t.Fatal("hover did not change the icon color")
	}
	tester.Move(0, 0)
	tester.Key(0, ui.KeyTab) // input
	tester.Key(0, ui.KeyTab) // reveal action
	if !tester.Focused("显示内容") {
		t.Fatal("the inline action is not reachable by keyboard")
	}
	stroke := kittest.PixelAt(tester, 320, eye.X+eye.W/2, eye.Y+theme.Space1+.5)
	if !kittest.Within(stroke, theme.UI(theme.Light.Accent), 10) {
		t.Fatal("focus stroke missing inside the box", stroke)
	}
	in, _ := tester.Find("密钥")
	if border := kittest.PixelAt(tester, 320, in.X+in.W/2, in.Y+.5); kittest.Near(border, theme.UI(theme.Light.Accent)) {
		t.Fatal("the input border also turned accent while only the action has focus: stacked indicators", border)
	}
}

func TestInlineActionIsReusableAndGated(t *testing.T) {
	calls := 0
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		InlineAction(c, icon.Close, "清除", InlineActionProps{OnClick: func() { calls++ }, Key: "a"})
		InlineAction(c, icon.Close, "停用清除", InlineActionProps{Disabled: true, OnClick: func() { calls += 10 }, Key: "b"})
	}), 200, 100)
	_ = tester.Click("清除")
	_ = tester.Click("停用清除")
	if calls != 1 {
		t.Fatal("click gating wrong", calls)
	}
}

func TestPasswordBusyKeepsTheToggleAfterTheLoader(t *testing.T) {
	tester := passwordTester(force.None, InputProps{Label: "密钥", Busy: true, Key: "k", Width: 260}, "abc", false)
	loader, okL := tester.Find("Loading indicator")
	eye, okE := tester.Find("显示内容")
	if !okL || !okE || loader.X >= eye.X {
		t.Fatal("busy password lost the toggle or put the loader after it", loader, eye)
	}
}

func TestTooltipKeepsTheBusyDescription(t *testing.T) {
	_, entries := kittest.Names(t)
	ui.NewTester(kittest.Centered(func(c *ui.Context) {
		IconButton(c, icon.Refresh, "刷新", IconButtonProps{Busy: true, Key: "r"})
		Button(c, "保存", ButtonProps{IconOnly: true, Icon: icon.Check, Busy: true, Key: "s"})
	}), 240, 120)
	found := 0
	for _, e := range entries() {
		if e.Kind == "description:Tooltip" && strings.Contains(e.Name, busyHint) && strings.Contains(e.Name, "。") {
			found++
		}
	}
	if found != 2 {
		t.Fatalf("%d tooltip descriptions carry the busy hint, want 2: %v", found, entries())
	}
}

func TestNestedFieldKeepsOuterStatesAndInnerError(t *testing.T) {
	v := ""
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		Field(c, FieldProps{Title: "外层", Disabled: true, Key: "o"}, func() {
			Field(c, FieldProps{Title: "内层", Error: "内层错误", Key: "i"}, func() { Input(c, &v, InputProps{Key: "in"}) })
		})
	}), 300, 200)
	tester.Key(0, ui.KeyTab)
	tester.Type("x")
	if v != "" || tester.Focused("内层") {
		t.Fatal("an outer disabled Field did not disable the inner control", v)
	}
	if !tester.HasText("内层错误") {
		t.Fatal("inner error missing")
	}
}

func TestAuditRejectsBlankNamesAndCoversRowsAndOptions(t *testing.T) {
	entries, restore := audit.Capture()
	audit.Named("X", "  \t")
	restore()
	if got := entries(); len(got) != 1 || got[0].Name != "" {
		t.Fatal("blank name was not normalised to empty so it fails the audit", got)
	}
	assertNamed, seen := kittest.Names(t)
	value := low
	open := true
	ui.NewTester(kittest.Centered(func(c *ui.Context) {
		a := Button(c, "更多", ButtonProps{Key: "t"})
		Menu(c, a, &open, menuItems(), MenuProps{Label: "操作"})
		Select(c, &value, effortOptions(), SelectProps{Label: "强度", Key: "s"})
		Tooltip(c, a, "   ")
	}), 420, 360)
	assertNamed()
	kinds := map[string]int{}
	for _, e := range seen() {
		kinds[e.Kind]++
		if e.Kind == "Tooltip" {
			t.Fatal("a blank tooltip was recorded")
		}
	}
	if kinds["MenuItem"] != 4 {
		t.Fatalf("menu rows audited %d times, want 4 (separator excluded)", kinds["MenuItem"])
	}
}

func TestDisabledSelectOptionIsDescribedAsUnavailable(t *testing.T) {
	_, entries := kittest.Names(t)
	value := low
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		Select(c, &value, effortOptions(), SelectProps{Label: "强度", Key: "s"})
	}), 360, 300)
	_ = tester.Click("强度")
	for _, e := range entries() {
		if e.Kind == "description:SelectOption" && e.Name == disabledOption {
			return
		}
	}
	t.Fatal("a disabled option exposes no unavailable semantics")
}
