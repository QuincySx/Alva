// INPUT: The K2 review findings: tooltip Escape inside overlays, radio grouping, real toasts, tab/row/nav semantics, naming and dialog sizing.
// OUTPUT: One regression test per finding.
// POS: [L7 Kit tests] Severe findings (tooltip Escape, RadioGroup, toast parity) are also mutation-verified.
//go:build desktop

package kit

import (
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/audit"
	"github.com/QuincySx/Alva/apps/ui/kit/kittest"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

func hasDescription(entries func() []audit.Entry, kind, contains string) bool {
	for _, e := range entries() {
		if e.Kind == "description:"+kind && strings.Contains(e.Name, contains) {
			return true
		}
	}
	return false
}

func TestEscapeClosesADialogFromAFocusedIconButtonWithATooltip(t *testing.T) {
	open := false
	_, entries := kittest.Names(t)
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, theme.Options{})
		ui.Column(c).Fill().Children(func() {
			Button(c, "打开", ButtonProps{Key: "open", OnClick: func() { open = true }})
			Dialog(c, &open, DialogProps{Label: "d"}, func() {
				DialogHeader(c, "标题", "说明", func() { open = false })
				IconButton(c, icon.More, "更多操作", IconButtonProps{Key: "more"})
				InlineAction(c, icon.Copy, "复制内容", InlineActionProps{Key: "copy"})
			})
		})
	}, 800, 600)
	tester.Key(0, ui.KeyTab)
	tester.Key(0, ui.KeyEnter)
	for _, target := range []string{"更多操作", "复制内容", "关闭"} {
		for i := 0; i < 6 && !tester.Focused(target); i++ {
			tester.Key(0, ui.KeyTab)
		}
		if !tester.Focused(target) {
			t.Fatal("could not focus", target)
		}
		if !tester.HasText(target) {
			t.Fatal("no tip text for", target)
		}
		tester.Key(0, ui.KeyEscape)
		if open {
			t.Fatalf("the first Escape with %s focused only hid its tooltip", target)
		}
		if !tester.Focused("打开") {
			t.Fatal("focus did not return to the opener after Escape from", target)
		}
		tester.Key(0, ui.KeyEnter)
		if !open {
			t.Fatal("could not reopen")
		}
	}
	if !hasDescription(entries, "Tooltip", "关闭") {
		t.Fatal("the dialog's close action lost its tooltip")
	}
}

func TestEscapeClosesAPopoverFromAHoveredAndFocusedIconButton(t *testing.T) {
	open := false
	view := func(c *ui.Context) {
		theme.Apply(c, theme.Options{})
		ui.Row(c).Fill().Padding(theme.Space6).Children(func() {
			trigger := Button(c, "筛选", ButtonProps{Key: "t", OnClick: func() { open = !open }})
			Popover(c, trigger, &open, PopoverProps{Label: "p", Width: 200}, func() {
				IconButton(c, icon.Filter, "清除筛选", IconButtonProps{Key: "clear"})
			})
		})
	}
	tester := ui.NewTester(view, 600, 300)
	tester.Key(0, ui.KeyTab)
	tester.Key(0, ui.KeyEnter)
	for i := 0; i < 4 && !tester.Focused("清除筛选"); i++ {
		tester.Key(0, ui.KeyTab)
	}
	tester.Key(0, ui.KeyEscape)
	if open || !tester.Focused("筛选") {
		t.Fatal("focused icon button: Escape did not close the popover and return focus", open)
	}
	// Hover case: the pointer rests on the icon button past the tip delay.
	tester.Key(0, ui.KeyEnter)
	r, ok := tester.Find("清除筛选")
	if !ok {
		t.Fatal("popover content missing")
	}
	tester.Move(r.X+r.W/2, r.Y+r.H/2)
	tester.Advance(tipDelay + 100*time.Millisecond)
	tester.Key(0, ui.KeyEscape)
	if open {
		t.Fatal("hovered icon button: Escape only hid the tip")
	}
}

func TestRadioGroupIsOneTabStopWithArrowsAndAName(t *testing.T) {
	selected := 1
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		ui.Column(c).Children(func() {
			RadioGroup(c, "尺寸", false, func() {
				Radio(c, &selected, 1, "小", RadioProps{Key: "a"})
				Radio(c, &selected, 2, "中", RadioProps{Key: "b"})
				Radio(c, &selected, 3, "大", RadioProps{Key: "c"})
			})
			Button(c, "之后", ButtonProps{Key: "after"})
		})
	}), 320, 240)
	tester.Key(0, ui.KeyTab)
	if !tester.Focused("小") {
		t.Fatal("Tab did not enter the group on the chosen radio")
	}
	tester.Key(0, ui.KeyDown)
	if selected != 2 {
		t.Fatal("an arrow key did not switch the selection", selected)
	}
	tester.Key(0, ui.KeyTab)
	if !tester.Focused("之后") {
		t.Fatal("the group is more than one Tab stop")
	}
}

func TestRadioOutsideAGroupFailsTheAudit(t *testing.T) {
	_, entries := kittest.Names(t)
	selected := 0
	ui.NewTester(kittest.Centered(func(c *ui.Context) { Radio(c, &selected, 1, "孤立", RadioProps{Key: "r"}) }), 200, 100)
	bad := false
	for _, e := range entries() {
		bad = bad || (strings.Contains(e.Kind, "outside RadioGroup") && e.Name == "")
	}
	if !bad {
		t.Fatal("a lone Radio was not reported")
	}
}

func TestRealToastMatchesToastViewSizeTitleAndShadow(t *testing.T) {
	const message = "已保存设置"
	view := ui.NewTester(kittest.Centered(func(c *ui.Context) { ToastView(c, ToastSuccess, message) }), 480, 240)
	shown := false
	real := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, theme.Options{})
		ui.Column(c).Fill().Background(theme.UI(pal(c).Surface)).Children(func() {
			ToastHost(c)
			if Button(c, "触发", ButtonProps{Key: "go"}).Clicked() && !shown {
				shown = true
				ShowToast(c, ToastSuccess, message)
			}
		})
	}, 480, 240)
	_ = real.Click("触发")
	real.Advance(1500 * time.Millisecond)
	a, okA := view.Find(message)
	b, okB := real.Find(message)
	if !okA || !okB {
		t.Fatal("toast missing", okA, okB)
	}
	if abs32(a.W-b.W) > .05 || abs32(a.H-b.H) > .05 {
		t.Fatalf("real toast is %vx%v, ToastView is %vx%v", b.W, b.H, a.W, a.H)
	}
	// Same surface, border and the shadow below the card, sampled relative to each card.
	for _, dy := range []float32{a.H / 2, a.H + 3, a.H + 12} {
		pa := kittest.PixelAt(view, 480, a.X+a.W-6, a.Y+dy)
		pb := kittest.PixelAt(real, 480, b.X+b.W-6, b.Y+dy)
		if !kittest.Within(pa, pb, 6) {
			t.Fatalf("dy %v: view %v real %v: surface or shadow differs", dy, pa, pb)
		}
	}
}

func TestTabsSelectedDisabledAndDuplicateDisabledLabels(t *testing.T) {
	value := 1
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		Tabs(c, &value, []TabItem{{Label: "甲"}, {Label: "归档", Disabled: true}, {Label: "归档", Disabled: true}, {Label: "乙"}}, TabsProps{Label: "t", Key: "tabs"})
	}), 420, 120)
	tester.Key(0, ui.KeyTab)
	tester.Key(0, ui.KeyRight)
	if value == 1 || value == 2 {
		t.Fatal("a disabled tab became the choice", value)
	}
	if !tester.HasText("归档") {
		t.Fatal("disabled tabs vanished")
	}
}

func TestListRowWithoutOnClickIsNotAControl(t *testing.T) {
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		ui.Column(c).Width(300).Children(func() {
			ListRow(c, "只读行", ListRowProps{Key: "info"})
			ListRow(c, "可点行", ListRowProps{OnClick: func() {}, Key: "act"})
		})
	}), 360, 160)
	tester.Key(0, ui.KeyTab)
	if tester.Focused("只读行") || !tester.Focused("可点行") {
		t.Fatal("an informational row is a tab stop")
	}
	info, _ := tester.Find("只读行")
	tester.Move(info.X+info.W/2, info.Y+info.H/2)
	if px := kittest.PixelAt(tester, 360, info.X+2, info.Y+info.H/2); !kittest.Near(px, theme.UI(theme.Light.Surface)) {
		t.Fatal("an informational row reacts to hover", px)
	}
}

func TestNavigationsAreOneFocusGroupWithArrows(t *testing.T) {
	area, index := AreaWork, 0
	side := ui.NewTester(kittest.Row(func(c *ui.Context) {
		Sidebar(c, SidebarProps{Area: area, OnSelect: func(a SidebarArea) { area = a }, OnCollapse: func() {}, Key: "sb"})
	}), 400, 400)
	side.Key(0, ui.KeyTab)
	first := side.Focused("收起侧栏") || side.Focused("新建") || side.Focused("工作区")
	side.Key(0, ui.KeyDown)
	if !first {
		t.Fatal("Tab did not enter the sidebar group")
	}
	nav := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		SettingsNav(c, []SettingsCategory{{"常规", icon.Settings}, {"模型", icon.Chip}, {"插件", icon.Plugin}}, SettingsNavProps{Selected: index, OnSelect: func(i int) { index = i }, Key: "sn"})
		Button(c, "之后", ButtonProps{Key: "after"})
	}), 320, 300)
	nav.Key(0, ui.KeyTab)
	if !nav.Focused("常规") {
		t.Fatal("Tab did not enter the settings navigation")
	}
	nav.Key(0, ui.KeyDown)
	if !nav.Focused("模型") {
		t.Fatal("Down did not move within the navigation")
	}
	nav.Key(0, ui.KeyTab)
	if !nav.Focused("之后") {
		t.Fatal("the navigation is more than one Tab stop")
	}
}

func TestProgressErrorNamesTheFailureAndSkeletonAnnouncesOnce(t *testing.T) {
	_, entries := kittest.Names(t)
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		ui.Column(c).Width(300).Gap(theme.Space3).Children(func() {
			Progress(c, ProgressProps{Title: "同步", Percent: 40, State: ProgressError, Key: "p"})
			SkeletonGroup(c, "sk", func() {
				Skeleton(c.Key(0), 100, theme.Space4)
				Skeleton(c.Key(1), 60, theme.Space4)
			})
		})
	}), 360, 240)
	if !tester.HasText("失败") || !hasDescription(entries, "Progress", "失败") {
		t.Fatal("a failed progress relies on color alone")
	}
	n := 0
	for _, s := range tester.Texts() {
		if s == "加载中" {
			n++
		}
	}
	if n != 1 {
		t.Fatalf("the skeleton group announces %d times, want 1", n)
	}
}

func TestAvatarRequiresANameAndKindsStringify(t *testing.T) {
	entries, restore := audit.Capture()
	ui.NewTester(kittest.Centered(func(c *ui.Context) { Avatar(c, AvatarProps{Kind: AvatarAgent, Key: "a"}) }), 120, 100)
	restore()
	named := false
	for _, e := range entries() {
		named = named || (e.Kind == "Avatar" && e.Name == "")
	}
	if !named {
		t.Fatal("a nameless avatar passed the audit")
	}
	if AvatarPerson.String() != "Person" || AvatarProvider.String() != "Provider" || AvatarAgent.String() == "" {
		t.Fatal("kind strings are not the design axis values")
	}
}

func TestDialogStaysInsideANarrowWindowAndScrollsLongContent(t *testing.T) {
	open := true
	const w, h = 360, 300
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, theme.Options{})
		Dialog(c, &open, DialogProps{Label: "长内容"}, func() {
			DialogHeader(c, "长内容标题", "", func() { open = false })
			for i := range 30 {
				Text(c, "第 "+string(rune('A'+i%26))+" 行内容", TextProps{Key: "line-" + string(rune('a'+i%26)) + string(rune('0'+i/26))})
			}
			DialogFooter(c, func() { Button(c, "确定", ButtonProps{Key: "ok"}) })
		})
	}, w, h)
	head, ok := tester.Find("长内容标题")
	if !ok || head.X < 0 || head.X+head.W > w {
		t.Fatalf("dialog content outside the %d px window: %v", w, head)
	}
	before, _ := tester.Find("确定")
	if before.Y < h {
		t.Fatalf("the footer is inside the window (%v) before scrolling: the content did not overflow", before)
	}
	tester.Scroll(w/2, h/2, 0, 4000)
	after, _ := tester.Find("确定")
	if after.Y+after.H > h || after.Y < 0 {
		t.Fatalf("scrolling did not bring the footer into the window: %v", after)
	}
}

func abs32(v float32) float32 {
	if v < 0 {
		return -v
	}
	return v
}
