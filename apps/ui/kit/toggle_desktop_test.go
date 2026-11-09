// INPUT: Kit Checkbox and Switch under ui.Tester with real pointer and keyboard input.
// OUTPUT: Interaction (click, Space, Tab, disabled admission), geometry and pixel proof for Off/On/Mixed/Focus/Disabled.
// POS: [L7 Kit tests] Checkbox mark pixels are read at the 18px box; Switch thumb and track at the 38x24 box.
//go:build desktop

package kit

import (
	"testing"

	"github.com/QuincySx/Alva/apps/ui/kit/internal/force"
	"github.com/QuincySx/Alva/apps/ui/kit/kittest"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

func TestCheckboxInteractionAndPaint(t *testing.T) {
	for _, dark := range []bool{false, true} {
		on := false
		tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
			Checkbox(c, &on, "选择此项", CheckboxProps{Key: "cb"})
		}), 320, 120)
		tester.SetDark(dark)
		r, ok := tester.Find("选择此项")
		if !ok {
			t.Fatal("checkbox missing")
		}
		if r.H != theme.ControlHeight {
			t.Fatal("checkbox row is not 32px", r)
		}
		if err := tester.Click("选择此项"); err != nil || !on {
			t.Fatal("click did not check", err, on)
		}
		tester.Key(0, ui.KeyTab)
		tester.Key(0, ui.KeySpace)
		if on {
			t.Fatal("Space did not toggle the focused checkbox")
		}
	}
}

func TestCheckboxMarkPixels(t *testing.T) {
	for _, dark := range []bool{false, true} {
		for _, tc := range []struct {
			name         string
			checked, mix bool
		}{{"off", false, false}, {"on", true, false}, {"mixed", false, true}} {
			on := tc.checked
			tester := ui.NewTester(func(c *ui.Context) {
				theme.Apply(c, theme.Options{})
				ui.Box(c).Fill().Padding(theme.Space6).Background(theme.UI(theme.Of(c).Surface)).Children(func() {
					Checkbox(c, &on, "标记", CheckboxProps{Mixed: tc.mix, Key: "cb"}).Label("Mark row")
				})
			}, 320, 120)
			tester.SetDark(dark)
			p := theme.For(dark)
			row, _ := tester.Find("Mark row")
			if row.H != theme.ControlHeight {
				t.Fatal("row is not 32px", row)
			}
			cy := row.Y + row.H/2
			edgeColor := kittest.PixelAt(tester, 320, row.X+checkBoxSize/2, cy-checkBoxSize/2+.5)
			inner := kittest.PixelAt(tester, 320, row.X+2, cy)
			wantEdge, wantInner := theme.UI(p.ControlBorder), theme.UI(p.Surface)
			if tc.checked || tc.mix {
				wantEdge, wantInner = theme.UI(p.Accent), theme.UI(p.Accent)
			}
			if !kittest.Near(edgeColor, wantEdge) || !kittest.Near(inner, wantInner) {
				t.Fatalf("%s %s: edge %v inner %v, want %v / %v", tc.name, darkName(dark), edgeColor, inner, wantEdge, wantInner)
			}
			if tc.mix {
				if bar := kittest.PixelAt(tester, 320, row.X+checkBoxSize/2, cy); !kittest.Near(bar, theme.UI(p.OnAccent)) {
					t.Fatal("mixed bar missing", bar)
				}
			}
		}
	}
}

func TestCheckboxMixedClickChecksAndFocusRingDraws(t *testing.T) {
	on := false
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		Checkbox(c, &on, "全选", CheckboxProps{Mixed: !on, Key: "all"})
	}), 320, 120)
	if err := tester.Click("全选"); err != nil || !on {
		t.Fatal("clicking a mixed checkbox did not set the value", on)
	}
	on = false
	forced := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		force.With(force.Focus, func() { Checkbox(c, &on, "焦点", CheckboxProps{Key: "f"}).Label("Focus row") })
	}), 320, 120)
	r, _ := forced.Find("Focus row")
	top := kittest.PixelAt(forced, 320, r.X+checkBoxSize/2, r.Y+r.H/2-checkBoxSize/2+.5)
	if !kittest.Near(top, theme.UI(theme.Light.Accent)) {
		t.Fatal("focus ring missing on the checkbox mark", top)
	}
}

func TestCheckboxDisabledIsInertAndSkipped(t *testing.T) {
	a, b := false, false
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		ui.Column(c).Children(func() {
			Checkbox(c, &a, "停用项", CheckboxProps{Disabled: true, Key: "a"})
			Checkbox(c, &b, "可用项", CheckboxProps{Key: "b"})
		})
	}), 320, 160)
	_ = tester.Click("停用项")
	tester.Key(0, ui.KeyTab)
	if a || !tester.Focused("可用项") {
		t.Fatal("disabled checkbox toggled or took focus", a)
	}
}

func TestSwitchInteractionAndPaint(t *testing.T) {
	for _, dark := range []bool{false, true} {
		on := false
		tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
			Switch(c, &on, "启用", SwitchProps{Key: "sw"})
		}), 240, 120)
		tester.SetDark(dark)
		p := theme.For(dark)
		r, ok := tester.Find("启用")
		if !ok || r.W != switchWidth || r.H != switchHeight {
			t.Fatal("switch is not 38x24", r)
		}
		cy := r.Y + r.H/2
		thumbOff := kittest.PixelAt(tester, 240, r.X+switchInset+switchThumb/2, cy)
		if !kittest.Near(thumbOff, theme.UI(p.Secondary)) {
			t.Fatalf("off thumb %v want secondary %v", thumbOff, theme.UI(p.Secondary))
		}
		tester.Key(0, ui.KeyTab)
		tester.Key(0, ui.KeySpace)
		if !on {
			t.Fatal("Space did not turn the switch on")
		}
		track := kittest.PixelAt(tester, 240, r.X+switchInset+switchThumb/2, cy)
		thumbOn := kittest.PixelAt(tester, 240, r.X+switchWidth-switchInset-switchThumb/2, cy)
		if !kittest.Near(track, theme.UI(p.Accent)) || !kittest.Near(thumbOn, theme.UI(p.OnAccent)) {
			t.Fatalf("on track %v thumb %v, want accent / onAccent", track, thumbOn)
		}
		// Focus ring: accent line just outside the track top edge row.
		if ring := kittest.PixelAt(tester, 240, r.X+r.W/2, r.Y+.5); !kittest.Near(ring, theme.UI(p.Accent)) {
			t.Fatal("focus ring missing", ring)
		}
	}
}

func TestSwitchDisabledStaysQuiet(t *testing.T) {
	on := true
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		Switch(c, &on, "停用开关", SwitchProps{Disabled: true, Key: "sw"})
	}), 240, 120)
	r, _ := tester.Find("停用开关")
	_ = tester.Click("停用开关")
	if !on {
		t.Fatal("disabled switch changed")
	}
	// A disabled "on" switch does not use the accent track.
	track := kittest.PixelAt(tester, 240, r.X+switchInset+switchThumb/2, r.Y+r.H/2)
	if kittest.Within(track, theme.UI(theme.Light.Accent), 8) {
		t.Fatal("disabled switch painted the active accent track", track)
	}
}
