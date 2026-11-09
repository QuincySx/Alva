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
	for _, name := range []string{"页头", "面板头"} {
		if r, _ := tester.Find(name); r.H != theme.SizeHeader {
			t.Fatalf("%s is %v high, want 52", name, r.H)
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
	for _, narrow := range []bool{false, true} {
		side := ui.NewTester(kittest.Row(func(c *ui.Context) {
			Sidebar(c, SidebarProps{Area: area, Narrow: narrow, OnSelect: func(a SidebarArea) { area = a }, OnCollapse: func() {}, Key: "sb"})
		}), 400, 400)
		collapse, _ := side.Find("收起侧栏")
		// The sidebar is centered in the 400px window and its collapse action sits space.3 from its right edge.
		right := collapse.X + collapse.W + theme.Space3
		if width := 2*right - 400; width != theme.SizeSidebar(narrow) {
			t.Fatalf("sidebar width %v, want %v (narrow=%v)", width, theme.SizeSidebar(narrow), narrow)
		}
		if err := side.Click("伙伴"); err != nil || area != AreaPartners {
			t.Fatal("sidebar did not report the chosen area", narrow, area, err)
		}
		area = AreaWork
	}
	nav := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		SettingsNav(c, []SettingsCategory{{"常规", icon.Settings}, {"模型", icon.Chip}, {"插件", icon.Plugin}}, SettingsNavProps{Selected: 0, OnSelect: func(i int) { picked = i }, Key: "sn"})
	}), 300, 240)
	row, _ := nav.Find("常规")
	if row.W != theme.SizeNavigation(false)-2*theme.Space3 {
		t.Fatal("settings nav rows are not the 208 column minus padding", row.W)
	}
	if err := nav.Click("插件"); err != nil || picked != 2 {
		t.Fatal("settings nav did not report the index", picked)
	}
}
