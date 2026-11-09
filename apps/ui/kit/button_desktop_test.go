// INPUT: Kit Button/IconButton under ui.Tester with real pointer/keyboard focus, forced states and reduced-motion preferences.
// OUTPUT: Interaction, geometry and pixel proof; the assertions of hubdesktop's workspace_controls_desktop_test.go for buttons, ported.
// POS: [L7 Kit tests] No OS window, service write or model execution.
//go:build desktop

package kit

import (
	"crypto/sha256"
	"math"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/force"
	"github.com/QuincySx/Alva/apps/ui/kit/kittest"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

func darkName(dark bool) string { return map[bool]string{false: "light", true: "dark"}[dark] }

func TestButtonBusyRetainsWidthNameFocusAndAdmission(t *testing.T) {
	for _, tc := range []struct {
		name, text string
		leading    icon.Name
		dark       bool
	}{
		{"text-short", "保存", "", false},
		{"text-long-dark", "保存新的工作区设置", "", true},
		{"leading-long", "重新获取模型目录", icon.Refresh, false},
		{"leading-short-dark", "重试", icon.Refresh, true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			busy, calls := false, 0
			tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
				Button(c, tc.text, ButtonProps{Kind: Primary, Icon: tc.leading, Busy: busy, Key: "action", OnClick: func() {
					calls++
					busy = true
				}}).Label("Reviewed action")
			}), 640, 180)
			tester.SetDark(tc.dark)
			before, ok := tester.Find("Reviewed action")
			if !ok || before.H != theme.ControlHeight {
				t.Fatal("button missing or not 32px", before)
			}
			tester.Key(0, ui.KeyTab)
			tester.Key(0, ui.KeyEnter)
			after, present := tester.Find("Reviewed action")
			if !present || before != after || calls != 1 || !tester.Focused("Reviewed action") {
				t.Fatal("busy changed width/name/focus or dispatched twice", before, after, calls)
			}
			spinner, found := tester.Find("Loading indicator")
			wantX := after.X + after.W/2
			if tc.leading != "" {
				wantX = after.X + theme.Space2 + theme.IconSmall/2
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
				t.Fatal("busy button dispatched a repeated pointer or keyboard activation", calls)
			}
			busy = false
			tester.Frame()
			tester.Key(0, ui.KeyEnter)
			if calls != 2 {
				t.Fatal("completed action could not be activated again", calls)
			}
		})
	}
}

func TestButtonTrailingIconSlotHoldsTheLoader(t *testing.T) {
	widths := map[bool]float32{}
	for _, busy := range []bool{false, true} {
		tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
			Button(c, "下一步", ButtonProps{TrailingIcon: icon.ArrowRight, Busy: busy, Key: "next"}).Label("Next")
		}), 320, 120)
		rect, _ := tester.Find("Next")
		widths[busy] = rect.W
		if busy {
			spinner, found := tester.Find("Loading indicator")
			if !found || spinner.X+spinner.W > rect.X+rect.W-theme.Space2+.02 || spinner.X < rect.X+rect.W/2 {
				t.Fatal("loader is not in the trailing slot", spinner, rect)
			}
		}
	}
	if widths[false] != widths[true] {
		t.Fatal("busy changed trailing button width", widths)
	}
}

func TestLoaderHonorsReducedMotionAndStopsRequestingFrames(t *testing.T) {
	builds := 0
	tester := ui.NewTester(func(c *ui.Context) {
		builds++
		ui.Box(c).Size(40, 40).Center().Children(func() { loader(c, theme.IconSize, theme.UI(pal(c).Secondary)) })
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

func TestLoaderFollowsTheApplicationMotionPreference(t *testing.T) {
	// theme.Apply with ReduceMotion disables animations for the frame; ReducedMotion reads that, so the
	// application preference reaches kit loaders without any Props or global.
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, theme.Options{ReduceMotion: true})
		ui.Box(c).Size(40, 40).Center().Children(func() { loader(c, theme.IconSize, theme.UI(pal(c).Secondary)) })
	}, 80, 80)
	before := sha256.Sum256(tester.Image().Pix)
	tester.Advance(210 * time.Millisecond)
	if before != sha256.Sum256(tester.Image().Pix) || tester.FramePending() {
		t.Fatal("application reduced-motion preference did not stop the loader")
	}
}

func TestButtonIntentColorsFollowFoundation(t *testing.T) {
	for _, dark := range []bool{false, true} {
		for _, kind := range []ButtonKind{Ghost, Danger} {
			t.Run(map[ButtonKind]string{Ghost: "ghost", Danger: "danger"}[kind]+"-"+darkName(dark), func(t *testing.T) {
				var hover, pressed ui.Color
				tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
					p := pal(c)
					hover, pressed = col(p.Subtle), col(p.Hover)
					if kind == Danger {
						hover, pressed = col(p.DangerBg), col(p.Danger)
					}
					Button(c, "确认操作", ButtonProps{Kind: kind, Key: "intent"}).Label("Intent action")
				}), 320, 140)
				tester.SetDark(dark)
				button, _ := tester.Find("Intent action")
				cx, cy := button.X+button.W/2, button.Y+button.H/2
				tester.Move(cx, cy)
				if got := kittest.PixelAt(tester, 320, button.X+3, cy); !kittest.Near(got, hover) {
					t.Fatal("hover lost its semantic surface", got, hover)
				}
				tester.Press(cx, cy)
				if got := kittest.PixelAt(tester, 320, button.X+3, cy); !kittest.Near(got, pressed) {
					t.Fatal("pressed lost its semantic surface", got, pressed)
				}
			})
		}
	}
}

func TestButtonDisabledColorsAndNativeAdmission(t *testing.T) {
	for _, dark := range []bool{false, true} {
		for _, kind := range []ButtonKind{Primary, Secondary, Ghost, Danger} {
			for _, leading := range []icon.Name{"", icon.Plus} {
				name := map[ButtonKind]string{Primary: "primary", Secondary: "secondary", Ghost: "ghost", Danger: "danger"}[kind] + "-" + darkName(dark) + map[bool]string{false: "-text", true: "-leading"}[leading != ""]
				t.Run(name, func(t *testing.T) {
					disabled, nativeDisabled, calls := false, false, 0
					var surface, subtle, muted ui.Color
					tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
						p := pal(c)
						surface, subtle, muted = col(p.Surface), col(p.Subtle), col(p.Secondary)
						ui.Column(c).Gap(theme.Space3).Children(func() {
							b := Button(c, "HHHH", ButtonProps{Kind: kind, Icon: leading, Disabled: disabled, Key: "disabled-action"}).Label("Disabled action").OnClick(func() { calls++ })
							nativeDisabled = b.IsDisabled()
							Button(c, "Next action", ButtonProps{Key: "next"})
						})
					}), 360, 180)
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
					want := ui.RGB(uint8((int(subtle.R)+int(surface.R))/2), uint8((int(subtle.G)+int(surface.G))/2), uint8((int(subtle.B)+int(surface.B))/2))
					if kind == Ghost {
						want = surface
					}
					if got := kittest.PixelAt(tester, 360, after.X+3, after.Y+after.H/2); !kittest.Within(got, want, 2) {
						t.Fatal("disabled surface did not use the Foundation subtle role with native 0.5 opacity", got, want)
					}
					text := ui.RGB(uint8((int(muted.R)+int(want.R))/2), uint8((int(muted.G)+int(want.G))/2), uint8((int(muted.B)+int(want.B))/2))
					closest := 256
					frame := tester.Image()
					for y := int(after.Y + 8); y < int(after.Y+after.H-8); y++ {
						for x := int(after.X + 12); x < int(after.X+after.W-12); x++ {
							px := frame.RGBAAt(x, y)
							closest = min(closest, max(diff(px.R, text.R), diff(px.G, text.G), diff(px.B, text.B)))
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
						t.Fatal("Tab did not skip the disabled control")
					}
					disabled = false
					tester.Frame()
					tester.Key(ui.Shift, ui.KeyTab)
					tester.Key(0, ui.KeyEnter)
					if calls != 1 || !tester.Focused("Disabled action") || nativeDisabled {
						t.Fatal("re-enabled control did not restore ordinary keyboard admission", calls)
					}
				})
			}
		}
	}
}

func diff(a, b uint8) int { return max(int(a)-int(b), int(b)-int(a)) }

func TestButtonBusyTakesPriorityOverDisabledIntent(t *testing.T) {
	for _, leading := range []icon.Name{"", icon.Plus} {
		busy, disabled, nativeDisabled, calls := false, false, false, 0
		tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
			ui.Column(c).Children(func() {
				b := Button(c, "Save changes", ButtonProps{Kind: Primary, Icon: leading, Busy: busy, Disabled: disabled, Key: "action", OnClick: func() { calls++ }}).Label("Loading action")
				nativeDisabled = b.IsDisabled()
				Button(c, "Next action", ButtonProps{Key: "next"})
			})
		}), 360, 140)
		tester.Key(0, ui.KeyTab)
		before, _ := tester.Find("Loading action")
		busy, disabled = true, true
		tester.Frame()
		after, ok := tester.Find("Loading action")
		if !ok || before != after || nativeDisabled || !tester.Focused("Loading action") || !tester.HasText("Loading indicator") {
			t.Fatal("busy lost its geometry, native name or focus to Disabled", before, after)
		}
		if err := tester.Click("Loading action"); err != nil {
			t.Fatal(err)
		}
		tester.Key(0, ui.KeyEnter)
		if calls != 0 {
			t.Fatal("busy dispatched a repeated action", calls)
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

func TestButtonForcedStatesPaintTheDesignRoles(t *testing.T) {
	for _, dark := range []bool{false, true} {
		for state, role := range map[force.State]func(theme.Palette) theme.Color{
			force.Hover:   func(p theme.Palette) theme.Color { return p.Hover },
			force.Pressed: func(p theme.Palette) theme.Color { return p.Hover },
		} {
			tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
				force.With(state, func() { Button(c, "按钮", ButtonProps{Key: "b"}).Label("Forced") })
			}), 240, 120)
			tester.SetDark(dark)
			r, _ := tester.Find("Forced")
			want := theme.UI(role(theme.For(dark)))
			if got := kittest.PixelAt(tester, 240, r.X+3, r.Y+r.H/2); !kittest.Near(got, want) {
				t.Fatalf("forced state %d paints %v, want %v", state, got, want)
			}
		}
		tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
			force.With(force.Focus, func() { Button(c, "按钮", ButtonProps{Key: "b"}).Label("Forced") })
		}), 240, 120)
		tester.SetDark(dark)
		r, _ := tester.Find("Forced")
		accent := theme.UI(theme.For(dark).Accent)
		if got := kittest.PixelAt(tester, 240, r.X+r.W/2, r.Y+1); !kittest.Near(got, accent) {
			t.Fatalf("forced focus did not draw the 2px accent ring at the edge: %v want %v", got, accent)
		}
	}
	// Forcing is scoped: a later button is unaffected.
	if force.Current() != force.None {
		t.Fatal("force leaked out of its scope")
	}
}

func TestButtonOnClickIsGatedAndIconOnlyKeepsItsName(t *testing.T) {
	calls := 0
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		Button(c, "添加文件", ButtonProps{Icon: icon.Plus, IconOnly: true, OnClick: func() { calls++ }, Key: "add"})
	}), 240, 120)
	r, ok := tester.Find("添加文件")
	if !ok || r.W != theme.ControlHeight || r.H != theme.ControlHeight {
		t.Fatal("icon-only button is not 32x32 with its label as name", r)
	}
	if err := tester.Click("添加文件"); err != nil || calls != 1 {
		t.Fatal("click did not reach OnClick", err, calls)
	}
}

func TestButtonKindsKeepOneGeometry(t *testing.T) {
	var widths [4]float32
	for i, kind := range []ButtonKind{Secondary, Primary, Ghost, Danger} {
		tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
			Button(c, "创建任务", ButtonProps{Kind: kind, Icon: icon.Plus, Key: "k"}).Label("K")
		}), 240, 120)
		r, _ := tester.Find("K")
		if r.H != theme.ControlHeight {
			t.Fatal("kind changed height", kind, r)
		}
		widths[i] = r.W
	}
	if widths[0] != widths[1] || widths[1] != widths[2] || widths[2] != widths[3] {
		t.Fatal("kind changed width", widths)
	}
}

func TestIconButtonSelectedAndNamed(t *testing.T) {
	calls := 0
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		IconButton(c, icon.Panel, "切换面板", IconButtonProps{Selected: true, OnClick: func() { calls++ }, Key: "panel"})
	}), 240, 120)
	r, ok := tester.Find("切换面板")
	if !ok || r.W != theme.ControlIcon || r.H != theme.ControlIcon {
		t.Fatal("icon button is not 28x28 and findable by its required name", r)
	}
	selected := theme.UI(theme.Light.AccentSoft)
	if got := kittest.PixelAt(tester, 240, r.X+2, r.Y+r.H/2); !kittest.Near(got, selected) {
		t.Fatal("Selected did not use the accentSoft surface", got, selected)
	}
	tester.Key(0, ui.KeyTab)
	if !tester.Focused("切换面板") {
		t.Fatal("icon button not reachable by keyboard")
	}
	tester.Key(0, ui.KeySpace)
	if calls != 1 {
		t.Fatal("Space did not activate", calls)
	}
}

func TestIconButtonBusyKeepsNameAndIgnoresRepeats(t *testing.T) {
	busy, calls := false, 0
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		IconButton(c, icon.Refresh, "刷新", IconButtonProps{Busy: busy, OnClick: func() { calls++; busy = true }, Key: "r"})
	}), 240, 120)
	before, _ := tester.Find("刷新")
	tester.Key(0, ui.KeyTab)
	tester.Key(0, ui.KeyEnter)
	after, ok := tester.Find("刷新")
	if !ok || before != after || !tester.Focused("刷新") || !tester.HasText("Loading indicator") {
		t.Fatal("busy icon button lost geometry, name, focus or loader")
	}
	_ = tester.Click("刷新")
	tester.Key(0, ui.KeyEnter)
	if calls != 1 {
		t.Fatal("busy icon button repeated", calls)
	}
}

func TestIconButtonDisabledSkipsTab(t *testing.T) {
	calls := 0
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		ui.Row(c).Gap(theme.Space2).Children(func() {
			IconButton(c, icon.Copy, "复制", IconButtonProps{Disabled: true, OnClick: func() { calls++ }, Key: "a"})
			IconButton(c, icon.Trash, "删除", IconButtonProps{Key: "b"})
		})
	}), 240, 120)
	_ = tester.Click("复制")
	tester.Key(0, ui.KeyTab)
	if calls != 0 || !tester.Focused("删除") {
		t.Fatal("disabled icon button took focus or fired", calls)
	}
}

func TestButtonsReportAccessibleNames(t *testing.T) {
	assertNamed, seen := kittest.Names(t)
	ui.NewTester(kittest.Centered(func(c *ui.Context) {
		Button(c, "保存", ButtonProps{Key: "a"})
		IconButton(c, icon.Close, "关闭", IconButtonProps{Key: "b"})
	}), 240, 160)
	assertNamed()
	if len(seen()) < 2 {
		t.Fatal("constructors did not report themselves")
	}
}
