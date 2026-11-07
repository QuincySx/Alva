// INPUT: Shared native actions, real MyGo keyboard/pointer focus and system motion preferences.
// OUTPUT: Native disabled colors/admission, stable Loading geometry and retained action identity/focus without duplicate activation.
// POS: [L7 Hub UI tests] Exercises control behavior without an OS window, service write or model execution.
//go:build desktop

package hubdesktop

import (
	"crypto/sha256"
	"math"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
)

func TestWorkspaceControlLoadingRetainsWidthNameFocusAndAdmission(t *testing.T) {
	for _, tc := range []struct {
		name, text, icon string
		dark             bool
	}{
		{"text-short", "保存", "", false},
		{"text-long-dark", "保存新的工作区设置", "", true},
		{"leading-long", "重新获取模型目录", "refresh-cw", false},
		{"leading-short-dark", "重试", "refresh-cw", true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			service, _ := fixture(t)
			page := NewNativePage(service)
			busy, calls := false, 0
			tester := ui.NewTester(func(c *ui.Context) {
				page.applyPreferences(c)
				ui.Column(c).Padding(24).Children(func() {
					workspaceControlAction(c, "action", tc.text, tc.icon, "primary", busy, func() {
						calls++
						busy = true
					}).Label("Reviewed action")
				})
			}, 640, 180)
			tester.SetDark(tc.dark)
			before, ok := tester.Find("Reviewed action")
			if !ok || before.H != 32 {
				t.Fatal("shared action missing", before)
			}
			tester.Key(0, ui.KeyTab)
			tester.Key(0, ui.KeyEnter)
			after, present := tester.Find("Reviewed action")
			if !present || before != after || calls != 1 || !tester.Focused("Reviewed action") {
				t.Fatal("loading changed width/name/focus or dispatched twice", before, after, calls)
			}
			spinner, found := tester.Find("Loading indicator")
			wantX := after.X + after.W/2
			if tc.icon != "" {
				wantX = after.X + 8 + 7
			}
			if !found || math.Abs(float64(spinner.X+spinner.W/2-wantX)) > .02 || math.Abs(float64(spinner.Y+spinner.H/2-after.Y-after.H/2)) > .02 {
				t.Fatal("loader did not retain its centered Text or leading icon slot", spinner, after)
			}
			for range 2 {
				if err := tester.Click("Reviewed action"); err != nil {
					t.Fatal(err)
				}
				tester.Key(0, ui.KeyEnter)
			}
			if calls != 1 {
				t.Fatal("busy action dispatched a repeated pointer or keyboard activation", calls)
			}
			busy = false
			tester.Frame()
			tester.Key(0, ui.KeyEnter)
			if calls != 2 {
				t.Fatal("completed action could not be activated explicitly again", calls)
			}
		})
	}
}

func TestWorkspaceLoaderHonorsReducedMotionAndStopsRequestingFrames(t *testing.T) {
	builds := 0
	tester := ui.NewTester(func(c *ui.Context) {
		builds++
		ui.Box(c).Size(40, 40).Center().Children(func() { workspaceLoadingIcon(c, 16, c.Theme().TextMuted) })
	}, 80, 80)
	initialBuilds := builds
	before := sha256.Sum256(tester.Image().Pix)
	tester.Advance(210 * time.Millisecond)
	if before == sha256.Sum256(tester.Image().Pix) {
		t.Fatal("active loader did not turn")
	}
	if builds != initialBuilds {
		t.Fatal("loader repaints rebuilt the view", initialBuilds, builds)
	}
	tester.SetPreferences(ui.Preferences{ReduceMotion: true})
	before = sha256.Sum256(tester.Image().Pix)
	tester.Advance(210 * time.Millisecond)
	if before != sha256.Sum256(tester.Image().Pix) || tester.FramePending() {
		t.Fatal("reduced-motion loader kept rotating or requesting frames")
	}
}

func TestWorkspaceControlIntentColorsFollowFoundation(t *testing.T) {
	for _, dark := range []bool{false, true} {
		for _, kind := range []string{"ghost", "danger"} {
			t.Run(kind+map[bool]string{false: "-light", true: "-dark"}[dark], func(t *testing.T) {
				service, _ := fixture(t)
				page := NewNativePage(service)
				var hover, pressed ui.Color
				tester := ui.NewTester(func(c *ui.Context) {
					page.applyPreferences(c)
					palette := workspacePalette(c)
					hover, pressed = palette.subtle, palette.hover
					if kind == "danger" {
						hover, pressed = palette.dangerBg, c.Theme().Danger
					}
					ui.Column(c).Padding(24).Background(c.Theme().Surface).Children(func() {
						workspaceControlButton(c, "intent", "确认操作", kind).Label("Intent action")
					})
				}, 320, 140)
				tester.SetDark(dark)
				button, _ := tester.Find("Intent action")
				cx, cy := button.X+button.W/2, button.Y+button.H/2
				tester.Move(cx, cy)
				pixel := tester.Image().RGBAAt(int(button.X+3), int(cy))
				if pixel.R != hover.R || pixel.G != hover.G || pixel.B != hover.B {
					t.Fatal("hover lost its semantic surface", kind, pixel, hover)
				}
				tester.Press(cx, cy)
				pixel = tester.Image().RGBAAt(int(button.X+3), int(cy))
				if pixel.R != pressed.R || pixel.G != pressed.G || pixel.B != pressed.B {
					t.Fatal("pressed lost its semantic surface", kind, pixel, pressed)
				}
			})
		}
	}
}

func TestWorkspaceControlDisabledColorsAndNativeAdmission(t *testing.T) {
	difference := func(a, b uint8) int { return max(int(a)-int(b), int(b)-int(a)) }
	for _, dark := range []bool{false, true} {
		for _, kind := range []string{"primary", "secondary", "ghost", "danger"} {
			for _, icon := range []string{"", "plus"} {
				name := kind + map[bool]string{false: "-light", true: "-dark"}[dark] + map[bool]string{false: "-text", true: "-leading"}[icon != ""]
				t.Run(name, func(t *testing.T) {
					service, _ := fixture(t)
					page := NewNativePage(service)
					disabled, nativeDisabled, calls := false, false, 0
					var surface, subtle, muted ui.Color
					tester := ui.NewTester(func(c *ui.Context) {
						page.applyPreferences(c)
						surface, subtle, muted = c.Theme().Surface, workspacePalette(c).subtle, c.Theme().TextMuted
						ui.Column(c).Padding(24).Gap(12).Background(surface).Children(func() {
							b := workspaceControlIconButton(c, "disabled-action", "HHHH", icon, kind, disabled).Label("Disabled action").OnClick(func() { calls++ })
							nativeDisabled = b.IsDisabled()
							workspaceControlButton(c, "next", "Next action", "secondary")
						})
					}, 360, 180)
					tester.SetDark(dark)
					before, ok := tester.Find("Disabled action")
					if !ok || nativeDisabled {
						t.Fatal("enabled control missing or disabled")
					}
					disabled = true
					tester.Frame()
					after, ok := tester.Find("Disabled action")
					if !ok || after != before || !nativeDisabled {
						t.Fatal("disabled variant changed geometry or lost native disabled semantics", before, after)
					}
					blend := func(a, b ui.Color) ui.Color {
						return ui.RGB(uint8((int(a.R)+int(b.R))/2), uint8((int(a.G)+int(b.G))/2), uint8((int(a.B)+int(b.B))/2))
					}
					want := blend(subtle, surface)
					if kind == "ghost" {
						want = surface
					}
					pixel := tester.Image().RGBAAt(int(after.X+3), int(after.Y+after.H/2))
					if difference(pixel.R, want.R) > 2 || difference(pixel.G, want.G) > 2 || difference(pixel.B, want.B) > 2 {
						t.Fatal("disabled surface did not use the Foundation subtle role with native 0.5 opacity", pixel, want)
					}
					// A fully covered glyph/icon pixel approaches secondary text at
					// the same native opacity, rather than Primary's on-ink text.
					text := blend(muted, want)
					closest := 256
					for y := int(after.Y + 8); y < int(after.Y+after.H-8); y++ {
						for x := int(after.X + 12); x < int(after.X+after.W-12); x++ {
							p := tester.Image().RGBAAt(x, y)
							distance := max(difference(p.R, text.R), difference(p.G, text.G), difference(p.B, text.B))
							closest = min(closest, distance)
						}
					}
					if closest > 12 {
						t.Fatal("disabled label/icon lost its secondary text role", closest, text)
					}
					image := sha256.Sum256(tester.Image().Pix)
					if err := tester.Click("Disabled action"); err != nil {
						t.Fatal(err)
					}
					if calls != 0 || tester.Focused("Disabled action") || image != sha256.Sum256(tester.Image().Pix) {
						t.Fatal("disabled pointer activation changed appearance, focus or admission", calls)
					}
					tester.Key(0, ui.KeyTab)
					if !tester.Focused("Next action") {
						t.Fatal("Tab did not skip the native disabled control")
					}
					disabled = false
					tester.Frame()
					tester.Key(ui.Shift, ui.KeyTab)
					tester.Key(0, ui.KeyEnter)
					if calls != 1 || !tester.Focused("Disabled action") || nativeDisabled {
						t.Fatal("reenabled control did not restore ordinary keyboard admission", calls)
					}
				})
			}
		}
	}
}

func TestWorkspaceControlLoadingTakesPriorityOverDisabledIntent(t *testing.T) {
	for _, icon := range []string{"", "plus"} {
		busy, disabled, nativeDisabled, calls := false, false, false, 0
		tester := ui.NewTester(func(c *ui.Context) {
			ui.Column(c).Padding(24).Children(func() {
				b := workspaceControlAction(c, "action", "Save changes", icon, "primary", busy, func() { calls++ }, disabled).Label("Loading action")
				nativeDisabled = b.IsDisabled()
				workspaceControlButton(c, "next", "Next action", "secondary")
			})
		}, 360, 140)
		tester.Key(0, ui.KeyTab)
		before, _ := tester.Find("Loading action")
		busy, disabled = true, true
		tester.Frame()
		after, ok := tester.Find("Loading action")
		if !ok || before != after || nativeDisabled || !tester.Focused("Loading action") || !tester.HasText("Loading indicator") {
			t.Fatal("Loading lost its geometry, native name or focus to Disabled", before, after)
		}
		if err := tester.Click("Loading action"); err != nil {
			t.Fatal(err)
		}
		tester.Key(0, ui.KeyEnter)
		if calls != 0 {
			t.Fatal("Loading dispatched a repeated action", calls)
		}
		busy = false
		tester.Frame()
		if !nativeDisabled || tester.HasText("Loading indicator") {
			t.Fatal("settled action did not apply the retained Disabled intent")
		}
		tester.Key(0, ui.KeyEnter)
		tester.Key(0, ui.KeyTab)
		if calls != 0 || !tester.Focused("Next action") {
			t.Fatal("settled Disabled action accepted a key or remained in Tab order", calls)
		}
	}
}
