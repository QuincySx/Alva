// INPUT: Kit Radio, Segmented, Tabs, ToggleButton and the row components under ui.Tester with real pointer and keyboard input.
// OUTPUT: Interaction, geometry and pixel proof for the K2 choice and list controls.
// POS: [L7 Kit tests] Keyboard models (arrows, Home/End, Space, disabled skipping) are MyGo's; these pin that the kit keeps them and paints the design roles.
//go:build desktop

package kit

import (
	"testing"

	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/force"
	"github.com/QuincySx/Alva/apps/ui/kit/kittest"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

func TestRadioSelectsPaintsAndSkipsDisabled(t *testing.T) {
	selected := 0
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		RadioGroup(c, "选项", false, func() {
			Radio(c, &selected, 1, "甲", RadioProps{Key: "a"})
			Radio(c, &selected, 2, "乙", RadioProps{Disabled: true, Key: "b"})
			Radio(c, &selected, 3, "丙", RadioProps{Key: "c"})
		})
	}), 320, 200)
	row, _ := tester.Find("甲")
	if row.H != theme.ControlHeight {
		t.Fatal("radio row is not 32px", row)
	}
	if err := tester.Click("甲"); err != nil || selected != 1 {
		t.Fatal("click did not select", selected)
	}
	cy := row.Y + row.H/2
	if edge := kittest.PixelAt(tester, 320, row.X+radioSize/2, cy-radioSize/2+.5); !kittest.Within(edge, theme.UI(theme.Light.Accent), 12) {
		t.Fatal("selected radio lost its accent border", edge)
	}
	if dot := kittest.PixelAt(tester, 320, row.X+radioSize/2, cy); !kittest.Near(dot, theme.UI(theme.Light.Accent)) {
		t.Fatal("selected radio has no dot", dot)
	}
	_ = tester.Click("乙")
	if selected != 1 {
		t.Fatal("disabled radio was selected")
	}
	tester.Key(0, ui.KeyTab)
	tester.Key(0, ui.KeyTab)
	if tester.Focused("乙") {
		t.Fatal("disabled radio took focus")
	}
}

func TestRadioForcedFocusDrawsTheRing(t *testing.T) {
	selected := 0
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		force.With(force.Focus, func() {
			RadioGroup(c, "g", false, func() { Radio(c, &selected, 1, "焦点", RadioProps{Key: "r"}) })
		})
	}), 240, 120)
	r, _ := tester.Find("焦点")
	if top := kittest.PixelAt(tester, 240, r.X+radioSize/2, r.Y+r.H/2-radioSize/2+.5); !kittest.Within(top, theme.UI(theme.Light.Accent), 12) {
		t.Fatal("no focus ring", top)
	}
}

func TestSegmentedKeyboardChoosesAndSkipsDisabled(t *testing.T) {
	value := 0
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		Segmented(c, &value, []string{"日", "周", "月", "年"}, SegmentedProps{Label: "周期", Disabled: []bool{1: true}, Key: "seg"})
	}), 360, 120)
	if r, _ := tester.Find("日"); r.H != theme.ControlRow {
		t.Fatal("segment is not 28px", r)
	}
	tester.Key(0, ui.KeyTab)
	tester.Key(0, ui.KeyRight)
	if value != 2 {
		t.Fatalf("Right chose %d, want 2 (the disabled segment is skipped)", value)
	}
	_ = tester.Click("周")
	if value != 2 {
		t.Fatal("a disabled segment was chosen by click")
	}
	if err := tester.Click("年"); err != nil || value != 3 {
		t.Fatal("click did not choose", value)
	}
	sel, _ := tester.Find("年")
	if px := kittest.PixelAt(tester, 360, sel.X+2, sel.Y+sel.H/2); !kittest.Near(px, theme.UI(theme.Light.Surface)) {
		t.Fatal("selected segment is not the surface chip", px)
	}
}

func TestTabsArrowsHomeEndAndBusy(t *testing.T) {
	value := 0
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		Tabs(c, &value, []TabItem{{Label: "概览"}, {Label: "活动", Busy: true}, {Label: "归档", Disabled: true}, {Label: "设置", Icon: icon.Settings}}, TabsProps{Label: "示例", Key: "tabs"})
	}), 420, 120)
	if r, _ := tester.Find("概览"); r.H != theme.ControlHeight {
		t.Fatal("tab is not 32px", r)
	}
	if !tester.HasText("Loading indicator") {
		t.Fatal("busy tab has no loader")
	}
	tester.Key(0, ui.KeyTab)
	tester.Key(0, ui.KeyRight)
	if value != 1 {
		t.Fatal("Right did not choose the next tab", value)
	}
	tester.Key(0, ui.KeyRight)
	if value != 3 {
		t.Fatalf("Right chose %d, want 3 (disabled skipped)", value)
	}
	tester.Key(0, ui.KeyHome)
	if value != 0 {
		t.Fatal("Home", value)
	}
	tester.Key(0, ui.KeyEnd)
	if value != 3 {
		t.Fatal("End", value)
	}
	r, _ := tester.Find("设置")
	if px := kittest.PixelAt(tester, 420, r.X+2, r.Y+r.H/2); !kittest.Within(px, theme.UI(theme.Light.AccentSoft), 3) {
		t.Fatal("selected tab is not accentSoft", px)
	}
}

func TestToggleButtonTogglesPaintsAndGates(t *testing.T) {
	on, off := false, false
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		ui.Row(c).Gap(theme.Space2).Children(func() {
			ToggleButton(c, &on, "收藏", ToggleButtonProps{Icon: icon.Star, Key: "a"})
			ToggleButton(c, &off, "停用", ToggleButtonProps{Disabled: true, Key: "b"})
		})
	}), 320, 120)
	r, _ := tester.Find("收藏")
	if r.H != theme.ControlHeight {
		t.Fatal("not 32px", r)
	}
	tester.Key(0, ui.KeyTab)
	tester.Key(0, ui.KeySpace)
	if !on {
		t.Fatal("Space did not toggle")
	}
	if px := kittest.PixelAt(tester, 320, r.X+focusGap+1, r.Y+r.H/2); !kittest.Within(px, theme.UI(theme.Light.AccentSoft), 4) {
		t.Fatal("pressed toggle lost its accent look", px)
	}
	_ = tester.Click("停用")
	if off {
		t.Fatal("disabled toggle changed")
	}
}

func TestRowsSelectedHoverGatingAndIndent(t *testing.T) {
	clicks := 0
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		ui.Column(c).Width(240).Children(func() {
			NavItem(c, "工作区", NavItemProps{Icon: icon.Chat, Selected: true, OnClick: func() { clicks++ }, Key: "a"})
			NavItem(c, "伙伴", NavItemProps{Icon: icon.Users, OnClick: func() {}, Key: "b"})
			NavItem(c, "停用项", NavItemProps{Icon: icon.Lock, Disabled: true, OnClick: func() { clicks += 10 }, Key: "c"})
			TreeRow(c, "根", TreeRowProps{Kind: TreeFolder, Key: "t0"})
			TreeRow(c, "深层", TreeRowProps{Kind: TreeFile, Level: 2, Key: "t2"})
		})
	}), 320, 320)
	a, _ := tester.Find("工作区")
	if a.H != theme.ControlRow {
		t.Fatal("nav row is not 28px", a)
	}
	if px := kittest.PixelAt(tester, 320, a.X+2, a.Y+a.H/2); !kittest.Near(px, theme.UI(theme.Light.Hover)) {
		t.Fatal("selected row is not the hover role", px)
	}
	b, _ := tester.Find("伙伴")
	tester.Move(b.X+b.W/2, b.Y+b.H/2)
	if px := kittest.PixelAt(tester, 320, b.X+2, b.Y+b.H/2); !kittest.Near(px, theme.UI(theme.Light.Subtle)) {
		t.Fatal("hovered row is not the subtle role", px)
	}
	_ = tester.Click("工作区")
	_ = tester.Click("停用项")
	if clicks != 1 {
		t.Fatal("click gating wrong", clicks)
	}
	root, _ := tester.Find("根")
	deep, _ := tester.Find("深层")
	firstInk := func(r ui.Rect) int {
		img := tester.Image()
		for x := int(r.X) + 1; x < int(r.X+r.W); x++ {
			for y := int(r.Y) + 4; y < int(r.Y+r.H)-4; y++ {
				if px := img.RGBAAt(x, y); !kittest.Near(ui.RGB(px.R, px.G, px.B), theme.UI(theme.Light.Surface)) {
					return x
				}
			}
		}
		return -1
	}
	if firstInk(deep)-firstInk(root) < 2*theme.Space4-2 {
		t.Fatalf("tree indent too small: %d vs %d", firstInk(deep), firstInk(root))
	}
}

func TestSkillResourceCompanionAndSettingRows(t *testing.T) {
	on, picked := true, "weekly"
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		ui.Column(c).Width(400).Children(func() {
			SkillRow(c, "审查", SkillRowProps{State: SkillOwned, Description: "d", Key: "s"})
			SkillRow(c, "下线", SkillRowProps{State: SkillUnavailable, Key: "u"})
			ResourceRow(c, "文档", ResourceRowProps{Kind: ResourceLink, Detail: "https://example.invalid", Key: "r"})
			CompanionRow(c, "伙伴甲", CompanionRowProps{State: CompanionPaused, Key: "p"})
			CompanionRow(c, "伙伴乙", CompanionRowProps{State: CompanionError, Key: "e"})
			SettingRow(c, SettingRowProps{Title: "自动同步", Description: "启动时刷新", Key: "sr"}, func() { Switch(c, &on, "自动同步开关", SwitchProps{Key: "sw"}) })
			SettingRow(c, SettingRowProps{Title: "频率", Key: "sr2"}, func() {
				Select(c, &picked, []Option[string]{{Value: "weekly", Label: "每周"}}, SelectProps{Label: "频率选择", Plain: true, Key: "se"})
			})
		})
	}), 480, 480)
	for _, text := range []string{"自有", "不可用", "https://example.invalid", "已暂停", "出错", "启动时刷新"} {
		if !tester.HasText(text) {
			t.Fatalf("%q missing", text)
		}
	}
	tester.Key(0, ui.KeyTab)
	tester.Key(0, ui.KeyTab)
	if tester.Focused("下线") {
		t.Fatal("unavailable skill took focus")
	}
	if err := tester.Click("自动同步开关"); err != nil || on {
		t.Fatal("the setting row's switch did not toggle", on)
	}
}

func TestPaginationStatesAndGating(t *testing.T) {
	more := 0
	for _, state := range []PaginationState{PaginationReady, PaginationLoading, PaginationEnd} {
		more = 0
		tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
			Pagination(c, PaginationProps{State: state, OnMore: func() { more++ }, Key: "p"})
		}), 320, 120)
		_ = tester.Click("加载更多")
		switch state {
		case PaginationReady:
			if more != 1 {
				t.Fatal("ready did not load more", more)
			}
		case PaginationLoading:
			if more != 0 || !tester.HasText("Loading indicator") {
				t.Fatal("loading dispatched or has no loader", more)
			}
		case PaginationEnd:
			if !tester.HasText("没有更多了") || tester.HasText("加载更多") {
				t.Fatal("end marker wrong")
			}
		}
	}
}
