// INPUT: Kit Popover, Dialog, Sheet, DialogHeader/Footer and the shell components under ui.Tester.
// OUTPUT: Proof of Esc with focus return, dialog focus lock, scrim dismissal, popover flip, sheet docking and shell geometry and callbacks.
// POS: [L7 Kit tests] The floating-surface hard requirements of K2; mutation-verified.
//go:build desktop

package kit

import (
	"testing"

	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/kittest"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

type dialogHarness struct {
	open, sheet bool
	confirmed   int
}

func (h *dialogHarness) view(c *ui.Context) {
	theme.Apply(c, theme.Options{})
	ui.Column(c).Fill().Padding(theme.Space6).Gap(theme.Space3).Background(theme.UI(pal(c).Surface)).Children(func() {
		Button(c, "打开", ButtonProps{Key: "open", OnClick: func() { h.open = true }})
		Button(c, "页面其它按钮", ButtonProps{Key: "other"})
		Dialog(c, &h.open, DialogProps{Label: "删除会话", Sheet: h.sheet}, func() {
			DialogHeader(c, "删除会话", "此操作无法撤销。", func() { h.open = false })
			DialogFooter(c, func() {
				Button(c, "取消", ButtonProps{Key: "cancel", OnClick: func() { h.open = false }})
				Button(c, "确认删除", ButtonProps{Kind: Danger, Key: "ok", OnClick: func() { h.confirmed++; h.open = false }})
			})
		})
	})
}

func openDialog(t *testing.T, sheet bool) (*dialogHarness, *ui.Tester) {
	t.Helper()
	h := &dialogHarness{sheet: sheet}
	tester := ui.NewTester(h.view, 800, 600)
	tester.Key(0, ui.KeyTab)
	tester.Key(0, ui.KeyEnter)
	if !h.open || !tester.HasText("此操作无法撤销。") {
		t.Fatal("dialog did not open")
	}
	return h, tester
}

func TestDialogEscapeClosesAndReturnsFocusToTheOpener(t *testing.T) {
	h, tester := openDialog(t, false)
	tester.Key(0, ui.KeyEscape)
	if h.open || tester.HasText("此操作无法撤销。") {
		t.Fatal("Escape did not close the dialog")
	}
	if !tester.Focused("打开") {
		t.Fatal("focus did not return to the element that opened the dialog")
	}
}

func TestDialogLocksFocusInside(t *testing.T) {
	h, tester := openDialog(t, false)
	for range 12 {
		tester.Key(0, ui.KeyTab)
		if tester.Focused("页面其它按钮") || tester.Focused("打开") {
			t.Fatal("Tab escaped the dialog to the page behind it")
		}
	}
	for range 12 {
		tester.Key(ui.Shift, ui.KeyTab)
		if tester.Focused("页面其它按钮") || tester.Focused("打开") {
			t.Fatal("Shift+Tab escaped the dialog")
		}
	}
	if !h.open {
		t.Fatal("dialog closed by itself")
	}
}

func TestDialogScrimPaintsTokenAndClickDismisses(t *testing.T) {
	h, tester := openDialog(t, false)
	want := kittest.Over(theme.UI(theme.Light.Surface), theme.UI(theme.Light.Scrim).Alpha(scrimAlpha))
	if px := kittest.PixelAt(tester, 800, 5, 5); !kittest.Within(px, want, 4) {
		t.Fatalf("scrim %v, want the scrim token over the page %v", px, want)
	}
	tester.ClickAt(5, 5)
	if h.open {
		t.Fatal("clicking the scrim did not close the dialog")
	}
}

func TestDialogActionsAndSheetDocking(t *testing.T) {
	h, tester := openDialog(t, false)
	panel, _ := tester.Find("删除会话")
	_ = panel
	if err := tester.Click("确认删除"); err != nil || h.confirmed != 1 || h.open {
		t.Fatal("the dialog action did not run and close", err, h.confirmed)
	}
	_, sheet := openDialog(t, true)
	header, _ := sheet.Find("此操作无法撤销。")
	if header.X < 800-theme.SizePanel(false) {
		t.Fatalf("sheet content is not docked in the right %v px: x=%v", theme.SizePanel(false), header.X)
	}
	ok, _ := sheet.Find("确认删除")
	if ok.X+ok.W > 800 {
		t.Fatal("sheet footer is off screen", ok)
	}
}

func TestPopoverEscapeReturnsFocusAndOutsidePressCloses(t *testing.T) {
	open := false
	checked := true
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, theme.Options{})
		ui.Row(c).Fill().Padding(theme.Space6).Gap(theme.Space4).Children(func() {
			trigger := Button(c, "筛选", ButtonProps{Key: "trigger", OnClick: func() { open = !open }})
			Popover(c, trigger, &open, PopoverProps{Label: "筛选面板", Width: 200}, func() {
				Checkbox(c, &checked, "只看进行中", CheckboxProps{Key: "c"})
			})
			Button(c, "其它", ButtonProps{Key: "other"})
		})
	}, 600, 300)
	tester.Key(0, ui.KeyTab)
	tester.Key(0, ui.KeyEnter)
	if !open || !tester.HasText("只看进行中") {
		t.Fatal("popover did not open")
	}
	tester.Key(0, ui.KeyEscape)
	if open || tester.HasText("只看进行中") {
		t.Fatal("Escape did not close")
	}
	if !tester.Focused("筛选") {
		t.Fatal("focus did not return to the trigger")
	}
	_ = tester.Click("筛选")
	_ = tester.Click("其它")
	if open {
		t.Fatal("a press outside did not close the popover")
	}
}

func TestPopoverFlipsAboveWhenThereIsNoRoomBelow(t *testing.T) {
	open := true
	var trigger, panel ui.Rect
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, theme.Options{})
		ui.Column(c).Fill().Justify(ui.End).Padding(theme.Space4).Children(func() {
			a := Button(c, "底部触发器", ButtonProps{Key: "t", OnClick: func() { open = true }})
			Popover(c, a, &open, PopoverProps{Label: "面板", Width: 200}, func() { Text(c, "弹层内容", TextProps{Key: "x"}) })
		})
	}, 400, 240)
	trigger, _ = tester.Find("底部触发器")
	panel, ok := tester.Find("弹层内容")
	if !ok || panel.Y >= trigger.Y {
		t.Fatalf("popover did not flip above its trigger: panel %v trigger %v", panel, trigger)
	}
}

func TestPopupShadowTokenIsTheDesignOne(t *testing.T) {
	// The kit calls Shadow only inside popupShadow (tests/architecture TestKitShadowComesOnlyFromThePopupToken).
	if s := theme.Light.Popup; s.Y != 8 || s.Blur != 30 || s.Spread != -6 || theme.Dark.Popup.Color.A != 0x66 {
		t.Fatal("shadow.popup token changed", s)
	}
}

func TestShellHeadersSidebarAndSettingsNav(t *testing.T) {
	var area SidebarArea
	collapsed, toggled, picked := true, 0, -1
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		ui.Column(c).Width(420).Gap(theme.Space2).Children(func() {
			PageHeader(c, PageHeaderProps{Title: "设置", Description: "常规", Collapsed: collapsed, OnToggleSidebar: func() { toggled++ }, Key: "ph"}).Label("页头")
			PanelHeader(c, PanelHeaderProps{Title: "文件", OnClose: func() {}, Key: "pn"}).Label("面板头")
		})
	}), 480, 320)
	for name, want := range map[string]float32{"页头": theme.SizeHeader, "面板头": theme.SizePanelHeader} {
		if r, _ := tester.Find(name); r.H != want || (name == "页头" && r.H != 36) || (name == "面板头" && r.H != 52) {
			t.Fatalf("%s is %v high, want %v (page header 36, panel header 52)", name, r.H, want)
		}
	}
	if err := tester.Click("展开侧栏"); err != nil || toggled != 1 {
		t.Fatal("collapsed header toggle did not call back", err, toggled)
	}
	collapsed = false
	tester.Frame()
	if tester.HasText("展开侧栏") {
		t.Fatal("an expanded header must not show the toggle")
	}
	// Narrow keeps the 220 sidebar with labels: the collapse action sits space.3 from the right edge of the centered sidebar.
	side := ui.NewTester(kittest.Row(func(c *ui.Context) {
		Sidebar(c, SidebarProps{Area: area, OnSelect: func(a SidebarArea) { area = a }, OnCollapse: func() {}, Key: "sb"})
	}), 400, 400)
	collapse, _ := side.Find("收起侧栏")
	right := collapse.X + collapse.W + theme.Space3
	if width := 2*right - 400; width != theme.SizeSidebar {
		t.Fatalf("sidebar width %v, want %v", width, theme.SizeSidebar)
	}
	if err := side.Click("伙伴"); err != nil || area != AreaPartners {
		t.Fatal("sidebar did not report the chosen area", area, err)
	}
	if !side.HasText("工作区") || !side.HasText("设置") {
		t.Fatal("the sidebar must show its labels")
	}
	nav := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		SettingsNav(c, []SettingsCategory{{"常规", icon.Settings}, {"模型", icon.Chip}, {"插件", icon.Plugin}}, SettingsNavProps{Selected: 0, OnSelect: func(i int) { picked = i }, Key: "sn"})
	}), 300, 240)
	row, _ := nav.Find("常规")
	if row.W != theme.SizeNavigation-2*theme.Space3 {
		t.Fatal("settings nav rows are not the 208 column minus padding", row.W)
	}
	if err := nav.Click("插件"); err != nil || picked != 2 {
		t.Fatal("settings nav did not report the index", picked)
	}
}

func TestPageHeaderTruncatesLongTitleAndSourceInsteadOfPushingActionsOut(t *testing.T) {
	for _, width := range []float32{360, 240} {
		pageHeaderKeepsActionsInView(t, width)
	}
}

func pageHeaderKeepsActionsInView(t *testing.T, width float32) {
	t.Helper()
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		ui.Column(c).Width(width).Children(func() {
			PageHeader(c, PageHeaderProps{
				Title:       "这是一个非常非常长的会话标题用来检查页头在窄宽度下的截断行为",
				Description: "alva-agent · 一个同样很长的来源说明文字 · 当前会话与工作目录",
				Collapsed:   true,
				Actions: func() {
					IconButton(c, icon.More, "更多操作", IconButtonProps{Key: "more"})
				},
				Key: "ph",
			}).Label("页头")
		})
	}), int(width+2*theme.Space6), 160)
	header, ok := tester.Find("页头")
	if !ok || header.W != width {
		t.Fatalf("header rect %v (found %v), want %v wide", header, ok, width)
	}
	more, ok := tester.Find("更多操作")
	if !ok {
		t.Fatal("the actions are not rendered")
	}
	// The action keeps its place inside the header's right padding; the long text gave way.
	if more.X+more.W > header.X+header.W-theme.Space4+0.5 || more.X < header.X {
		t.Fatalf("the action at %v sticks out of the header %v", more, header)
	}
	toggle, _ := tester.Find("展开侧栏")
	if toggle.X < header.X || toggle.X+toggle.W > more.X {
		t.Fatalf("the toggle %v is not between the header start and the action %v", toggle, more)
	}
}

// The source text owns the room between the title and the actions (no spacer element), so it shrinks first.
func TestPageHeaderSourceTakesTheRemainingWidth(t *testing.T) {
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		ui.Column(c).Width(480).Children(func() {
			PageHeader(c, PageHeaderProps{Title: "设置", Description: "常规", Actions: func() {
				IconButton(c, icon.More, "更多操作", IconButtonProps{Key: "more"})
			}, Key: "ph"})
		})
	}), 480+2*int(theme.Space6), 160)
	desc, ok := tester.Find("常规")
	more, ok2 := tester.Find("更多操作")
	if !ok || !ok2 {
		t.Fatal("header text or action missing")
	}
	if gap := more.X - (desc.X + desc.W); gap > theme.Space2+0.5 {
		t.Fatalf("the source ends %v before the action: it should fill the remaining width", gap)
	}
}
