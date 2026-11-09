// INPUT: Kit Badge, Avatar, Notice, EmptyState, Skeleton, Progress, Step, DetailRow, Divider, Card and Toast under ui.Tester.
// OUTPUT: Pixel and geometry proof of the tone surfaces, sizes and progress fill.
// POS: [L7 Kit tests] These components have no interaction, so the design roles in pixels are the contract.
//go:build desktop

package kit

import (
	"testing"

	"github.com/QuincySx/Alva/apps/ui/kit/kittest"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

func TestBadgeAndNoticeTonesUsePaletteSurfaces(t *testing.T) {
	for _, dark := range []bool{false, true} {
		p := theme.For(dark)
		want := map[BadgeTone]theme.Color{BadgeNeutral: p.Subtle, BadgeAccent: p.AccentSoft, BadgeSuccess: p.SuccessBg, BadgeWarning: p.WarningBg, BadgeDanger: p.DangerBg, BadgeInfo: p.Hover}
		for tone, color := range want {
			tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
				Badge(c, "标签", BadgeProps{Tone: tone, Key: "b"}).Label("徽标")
				Notice(c, NoticeProps{Tone: tone, Title: "提示", Key: "n"}).Width(260)
			}), 360, 200)
			tester.SetDark(dark)
			b, _ := tester.Find("徽标")
			if b.H != theme.Space6 {
				t.Fatal("badge height", b.H)
			}
			if px := kittest.PixelAt(tester, 360, b.X+2, b.Y+b.H/2); !kittest.Near(px, theme.UI(color)) {
				t.Fatalf("badge tone %d %s: %v want %v", tone, darkName(dark), px, theme.UI(color))
			}
			n, _ := tester.Find("提示")
			if px := kittest.PixelAt(tester, 360, n.X+2, n.Y+n.H/2); !kittest.Near(px, theme.UI(color)) {
				t.Fatalf("notice tone %d %s: %v want %v", tone, darkName(dark), px, theme.UI(color))
			}
		}
	}
}

func TestAvatarKindsAreThirtyTwoAndShaped(t *testing.T) {
	tester := ui.NewTester(kittest.Row(func(c *ui.Context) {
		Avatar(c, AvatarProps{Kind: AvatarPerson, Name: "林", Key: "a"})
		Avatar(c, AvatarProps{Kind: AvatarAgent, Name: "助手", Key: "b"})
		Avatar(c, AvatarProps{Kind: AvatarProvider, Name: "供应商", Key: "c"})
	}), 240, 120)
	for _, name := range []string{"林", "助手", "供应商"} {
		r, ok := tester.Find(name)
		if !ok || r.W != avatarSize || r.H != avatarSize {
			t.Fatalf("%s avatar %v", name, r)
		}
	}
	person, _ := tester.Find("林")
	provider, _ := tester.Find("供应商")
	// A circle leaves its corner on the page color; a rounded square covers it.
	corner := func(r ui.Rect) ui.Color { return kittest.PixelAt(tester, 240, r.X+2.5, r.Y+2.5) }
	if !kittest.Near(corner(person), theme.UI(theme.Light.Surface)) || kittest.Near(corner(provider), theme.UI(theme.Light.Surface)) {
		t.Fatal("person must be round and provider a rounded square")
	}
}

func TestProgressFillTracksPercentAndClamps(t *testing.T) {
	for _, tc := range []struct {
		pct   float32
		state ProgressState
		color theme.Color
		frac  float32
	}{{50, ProgressRunning, theme.Light.Accent, .5}, {100, ProgressDone, theme.Light.Success, 1}, {30, ProgressError, theme.Light.Danger, .3}, {250, ProgressRunning, theme.Light.Accent, 1}, {-5, ProgressRunning, theme.Light.Accent, 0}} {
		tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
			Progress(c, ProgressProps{Title: "进度", Percent: tc.pct, State: tc.state, Key: "p"}).Width(300)
		}), 400, 140)
		r, _ := tester.Find("进度")
		want := map[float32]string{250: "100%", -5: "0%", 50: "50%"}
		if text, ok := want[tc.pct]; ok && !tester.HasText(text) {
			t.Fatalf("pct %v shows no %q label", tc.pct, text)
		}
		barY := r.Y + r.H - progressBar/2
		left := kittest.PixelAt(tester, 400, r.X+3, barY)
		right := kittest.PixelAt(tester, 400, r.X+r.W-3, barY)
		wantRight := theme.Light.Subtle
		if tc.frac >= 1 {
			wantRight = tc.color
		}
		if tc.frac > 0 && !kittest.Near(left, theme.UI(tc.color)) || !kittest.Near(right, theme.UI(wantRight)) {
			t.Fatalf("pct %v: left %v right %v", tc.pct, left, right)
		}
	}
}

func TestStepEmptyStateSkeletonDetailDividerCardToast(t *testing.T) {
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		ui.Column(c).Width(360).Gap(theme.Space2).Children(func() {
			Step(c, "完成", StepProps{Index: 1, State: StepDone, Key: "s1"})
			Step(c, "进行", StepProps{Index: 2, State: StepCurrent, Key: "s2"})
			EmptyState(c, EmptyStateProps{Kind: EmptyError, Title: "加载失败", Description: "稍后再试", Key: "e"})
			SkeletonGroup(c, "sk", func() { Skeleton(c, 120, theme.Space4).Label("条") })
			DetailRow(c, "创建时间", "2026-11-09").Label("详情行")
			Divider(c.Key("d"), false).Label("分隔线")
			Card(c, "card", func() { Text(c, "卡片内容", TextProps{Key: "t"}) })
			ToastView(c, ToastError, "保存失败")
		})
	}), 420, 640)
	for _, s := range []string{"加载失败", "稍后再试", "卡片内容", "保存失败", "创建时间", "2026-11-09"} {
		if !tester.HasText(s) {
			t.Fatalf("%q missing", s)
		}
	}
	if sk, _ := tester.Find("条"); sk.W != 120 || sk.H != theme.Space4 {
		t.Fatal("skeleton size", sk)
	}
	if d, _ := tester.Find("分隔线"); d.H != 1 {
		t.Fatal("divider is not 1px", d)
	}
	label, _ := tester.Find("创建时间")
	value, _ := tester.Find("2026-11-09")
	if value.X-label.X < detailLabelWidth {
		t.Fatal("detail value does not start after the label column", label, value)
	}
}

func TestShowToastShowsTheMessage(t *testing.T) {
	shown := false
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		ToastHost(c)
		if Button(c, "提示", ButtonProps{Key: "b"}).Clicked() && !shown {
			shown = true
			ShowToast(c, ToastSuccess, "已保存设置")
		}
	}), 400, 300)
	_ = tester.Click("提示")
	if !tester.HasText("已保存设置") {
		t.Fatal("toast did not show")
	}
}
