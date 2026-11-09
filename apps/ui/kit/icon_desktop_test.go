// INPUT: Kit Icon and Text under ui.Tester.
// OUTPUT: Proof every design icon renders at 16px (14px small) in the inherited or toned color, PanelLeft mirrors Panel, and Text follows the type scale.
// POS: [L7 Kit tests] The full 69-icon sweep also feeds the gallery coverage; pixel checks use Ink so a missing glyph fails.
//go:build desktop

package kit

import (
	"testing"

	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/kittest"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

func TestEveryDesignIconRendersInkAtTheDesignSize(t *testing.T) {
	for _, small := range []bool{false, true} {
		want := float32(theme.IconSize)
		if small {
			want = theme.IconSmall
		}
		for _, name := range icon.Names() {
			tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
				Icon(c, name, IconProps{Small: small, Label: string(name), Tone: ToneText})
			}), 80, 80)
			r, ok := tester.Find(string(name))
			if !ok || r.W != want || r.H != want {
				t.Fatalf("%s is %v, want %vpx square", name, r, want)
			}
			ink := kittest.Ink(tester, 80, r, theme.UI(theme.Light.Surface))
			if kittest.Near(ink, theme.UI(theme.Light.Surface)) {
				t.Fatalf("%s drew nothing", name)
			}
		}
	}
}

func TestIconToneAndInheritance(t *testing.T) {
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		ui.Row(c).Gap(theme.Space4).Children(func() {
			Icon(c, icon.Check, IconProps{Tone: ToneDanger, Label: "danger", Key: "a"})
			ui.Row(c.Key("b")).TextColor(theme.UI(theme.Light.Accent)).Children(func() {
				Icon(c, icon.Check, IconProps{Label: "inherit", Key: "b1"})
			})
		})
	}), 120, 80)
	danger, _ := tester.Find("danger")
	if ink := kittest.Ink(tester, 120, danger, theme.UI(theme.Light.Surface)); !kittest.Within(ink, theme.UI(theme.Light.Danger), 40) {
		t.Fatal("tone did not color the icon", ink)
	}
	inherit, _ := tester.Find("inherit")
	if ink := kittest.Ink(tester, 120, inherit, theme.UI(theme.Light.Surface)); !kittest.Within(ink, theme.UI(theme.Light.Accent), 40) {
		t.Fatal("icon did not inherit the parent's text color", ink)
	}
}

func TestPanelLeftMirrorsPanelAndUnknownNamesKeepTheirSlot(t *testing.T) {
	var panel, left ui.Rect
	var tester *ui.Tester
	tester = ui.NewTester(kittest.Centered(func(c *ui.Context) {
		ui.Row(c).Gap(theme.Space6).Children(func() {
			Icon(c, icon.Panel, IconProps{Label: "panel", Key: "p"})
			Icon(c, icon.PanelLeft, IconProps{Label: "left", Key: "l"})
			Icon(c, "", IconProps{Label: "empty", Key: "e"})
			Icon(c, "NotAnIcon", IconProps{Label: "unknown", Key: "u"})
		})
	}), 200, 80)
	panel, _ = tester.Find("panel")
	left, _ = tester.Find("left")
	bg := theme.UI(theme.Light.Surface)
	// Panel (right panel icon) has its filled side on the right; the mirror has it on the left.
	probe := func(r ui.Rect, right bool) ui.Color {
		x := r.X + r.W*0.2
		if right {
			x = r.X + r.W*0.8
		}
		return kittest.Ink(tester, 200, ui.Rect{X: x - 1, Y: r.Y + 2, W: 2, H: r.H - 4}, bg)
	}
	if kittest.Near(probe(panel, true), bg) || kittest.Near(probe(left, false), bg) {
		t.Fatal("expected ink on the divider sides of Panel (right) and PanelLeft (left)")
	}
	for _, label := range []string{"empty", "unknown"} {
		if r, ok := tester.Find(label); !ok || r.W != theme.IconSize {
			t.Fatalf("%s did not keep a %vpx slot", label, theme.IconSize)
		}
	}
}

func TestTextFollowsTheTypeScale(t *testing.T) {
	sizes := map[TextRole]float32{RoleCaption: theme.FontSizeCaption, RoleBody: theme.FontSizeBody, RoleSubtitle: theme.FontSizeSubtitle, RoleHeading: theme.FontSizeHeading, RoleDisplay: theme.FontSizeDisplay}
	var prev float32
	for _, role := range []TextRole{RoleCaption, RoleBody, RoleSubtitle, RoleHeading, RoleDisplay} {
		tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
			Text(c, "字号阶梯", TextProps{Role: role, SingleLine: true})
		}), 400, 120)
		r, ok := tester.Find("字号阶梯")
		if !ok {
			t.Fatal("text missing")
		}
		if r.H < sizes[role] || r.H <= prev {
			t.Fatalf("role %d height %v does not grow with its scale step %v", role, r.H, sizes[role])
		}
		prev = r.H
	}
	if roleSize(RoleMono) != theme.FontSizeBody {
		t.Fatal("mono shares the body size")
	}
}
