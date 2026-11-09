// INPUT: Kit Menu on PopoverBase under ui.Tester with real clicks, arrows, Home/End, typing and Escape.
// OUTPUT: Proof of roving focus, disabled-item skipping, type-ahead, choose-and-close, Escape with focus return, and popup shadow/surface paint.
// POS: [L7 Kit tests] Menu is the K1 risk item: MyGo 0.7.4 has no MenuBase, so this file is what keeps the hand-written keyboard model honest.
//go:build desktop

package kit

import (
	"testing"

	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/kittest"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

func menuItems() []MenuItem {
	return []MenuItem{
		{Key: "copy", Label: "复制", Icon: icon.Copy, Shortcut: "⌘C"},
		{Key: "rename", Label: "重命名", Disabled: true},
		{Key: "export", Label: "导出", Description: "保存为文件"},
		{Separator: true},
		{Key: "delete", Label: "删除", Icon: icon.Trash, Danger: true},
	}
}

type menuHarness struct {
	open   bool
	chosen []string
	items  []MenuItem
}

func (h *menuHarness) view(c *ui.Context) {
	theme.Apply(c, theme.Options{})
	ui.Column(c).Fill().Padding(theme.Space6).Background(theme.UI(pal(c).Surface)).Children(func() {
		trigger := Button(c, "更多", ButtonProps{Key: "trigger", OnClick: func() { h.open = !h.open }})
		if key := Menu(c, trigger, &h.open, h.items, MenuProps{Label: "更多操作"}); key != "" {
			h.chosen = append(h.chosen, key)
		}
	})
}

func newMenu(t *testing.T) (*menuHarness, *ui.Tester) {
	t.Helper()
	h := &menuHarness{items: menuItems()}
	tester := ui.NewTester(h.view, 420, 360)
	if err := tester.Click("更多"); err != nil || !h.open {
		t.Fatal("trigger did not open the menu", err)
	}
	return h, tester
}

func TestMenuOpensOnTheFirstEnabledItemAndWalksSkippingDisabled(t *testing.T) {
	_, tester := newMenu(t)
	if !tester.Focused("复制") {
		t.Fatal("focus did not enter the menu on its first enabled item")
	}
	tester.Key(0, ui.KeyDown)
	if !tester.Focused("导出") {
		t.Fatal("Down did not skip the disabled item")
	}
	tester.Key(0, ui.KeyDown)
	if !tester.Focused("删除") {
		t.Fatal("Down did not pass the separator")
	}
	tester.Key(0, ui.KeyDown)
	if !tester.Focused("复制") {
		t.Fatal("Down did not wrap to the first item")
	}
	tester.Key(0, ui.KeyUp)
	if !tester.Focused("删除") {
		t.Fatal("Up did not wrap to the last item")
	}
	tester.Key(0, ui.KeyHome)
	if !tester.Focused("复制") {
		t.Fatal("Home did not reach the first item")
	}
	tester.Key(0, ui.KeyEnd)
	if !tester.Focused("删除") {
		t.Fatal("End did not reach the last item")
	}
}

func TestMenuEnterChoosesAndCloses(t *testing.T) {
	h, tester := newMenu(t)
	tester.Key(0, ui.KeyDown)
	tester.Key(0, ui.KeyEnter)
	if len(h.chosen) != 1 || h.chosen[0] != "export" || h.open {
		t.Fatal("Enter did not choose the focused item and close", h.chosen, h.open)
	}
	if tester.HasText("保存为文件") {
		t.Fatal("menu stayed on screen after choosing")
	}
}

func TestMenuClickChoosesDisabledIsInert(t *testing.T) {
	h, tester := newMenu(t)
	_ = tester.Click("重命名")
	if len(h.chosen) != 0 || !h.open {
		t.Fatal("a disabled item was chosen or closed the menu", h.chosen)
	}
	if err := tester.Click("删除"); err != nil || len(h.chosen) != 1 || h.chosen[0] != "delete" {
		t.Fatal("click did not choose the item", err, h.chosen)
	}
}

func TestMenuEscapeClosesAndReturnsFocusToTheTrigger(t *testing.T) {
	h := &menuHarness{items: menuItems()}
	tester := ui.NewTester(h.view, 420, 360)
	tester.Key(0, ui.KeyTab)
	tester.Key(0, ui.KeyEnter)
	if !h.open || !tester.Focused("复制") {
		t.Fatal("Enter on the focused trigger did not open the menu onto its first item", h.open)
	}
	tester.Key(0, ui.KeyDown)
	tester.Key(0, ui.KeyEscape)
	if h.open || len(h.chosen) != 0 {
		t.Fatal("Escape did not close without choosing", h.open, h.chosen)
	}
	if !tester.Focused("更多") {
		t.Fatal("focus did not return to the trigger after Escape")
	}
}

func TestMenuTypeAheadFocusesByFirstLetters(t *testing.T) {
	_, tester := newMenu(t)
	tester.Type("导")
	if !tester.Focused("导出") {
		t.Fatal("typing the first letter did not focus the matching item")
	}
	tester.Type("删")
	if !tester.Focused("删除") {
		t.Fatal("type-ahead did not move on to another match")
	}
}

func TestTypeAheadMatchingRules(t *testing.T) {
	items := []MenuItem{{Label: "Alpha"}, {Label: "Beta", Disabled: true}, {Label: "Bravo"}, {Separator: true}, {Label: "Bronze"}}
	if got := typeAhead(items, "b", 0); got != 2 {
		t.Fatalf("b from 0 = %d, want the first enabled B (2)", got)
	}
	if got := typeAhead(items, "b", 2); got != 4 {
		t.Fatalf("repeated b = %d, want the next B (4)", got)
	}
	if got := typeAhead(items, "br", 0); got != 2 {
		t.Fatalf("br = %d, want Bravo", got)
	}
	if got := typeAhead(items, "z", 0); got != -1 {
		t.Fatalf("z = %d, want no match", got)
	}
}

func TestMenuPaintsThePopoverRecipeAndDangerTone(t *testing.T) {
	for _, dark := range []bool{false, true} {
		h := &menuHarness{items: menuItems()}
		tester := ui.NewTester(h.view, 420, 360)
		tester.SetDark(dark)
		_ = tester.Click("更多")
		p := theme.For(dark)
		copyRow, _ := tester.Find("复制")
		// Panel surface sits left of the row inside the panel padding; the selected/hovered row would differ.
		if got := kittest.PixelAt(tester, 420, copyRow.X-theme.Space1, copyRow.Y+copyRow.H*2); !kittest.Within(got, theme.UI(p.Surface), 3) {
			t.Fatalf("menu panel is not the surface color: %v", got)
		}
		del, _ := tester.Find("删除")
		ink := kittest.Ink(tester, 420, ui.Rect{X: del.X, Y: del.Y, W: del.W, H: del.H}, theme.UI(p.Surface))
		if kittest.Near(ink, theme.UI(p.Text)) {
			t.Fatal("danger item label did not leave the text color", ink)
		}
	}
}

func TestMenuRemountedAfterUnmountWhileOpenStartsFresh(t *testing.T) {
	h := &menuHarness{items: menuItems()}
	mounted := true
	tester := ui.NewTester(func(c *ui.Context) {
		if mounted {
			h.view(c)
			return
		}
		theme.Apply(c, theme.Options{})
	}, 420, 360)
	_ = tester.Click("更多")
	mounted, h.open = false, false
	tester.Frame()
	mounted = true
	tester.Frame()
	_ = tester.Click("更多")
	if !tester.Focused("复制") {
		t.Fatal("a menu unmounted while open kept stale state: focus did not enter the fresh menu")
	}
}

func TestMenuOutsideClickDoesNotStealFocus(t *testing.T) {
	var open bool
	other := 0
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, theme.Options{})
		ui.Row(c).Fill().Padding(theme.Space6).Gap(theme.Space4).Children(func() {
			trigger := Button(c, "更多", ButtonProps{Key: "trigger", OnClick: func() { open = !open }})
			Menu(c, trigger, &open, menuItems(), MenuProps{Label: "操作"})
			Button(c, "其它", ButtonProps{Key: "other", OnClick: func() { other++ }})
		})
	}, 600, 200)
	_ = tester.Click("更多")
	if err := tester.Click("其它"); err != nil {
		t.Fatal(err)
	}
	if open || other != 1 {
		t.Fatal("outside press did not close the menu and reach the button", open, other)
	}
	if tester.Focused("更多") {
		t.Fatal("closing by an outside press pulled the focus back to the trigger")
	}
}

func TestMenuTabClosesAndReturnsFocusToTheTrigger(t *testing.T) {
	h, tester := newMenu(t)
	tester.Key(0, ui.KeyTab)
	if h.open || !tester.Focused("更多") {
		t.Fatal("Tab left the menu open or did not return focus to the trigger", h.open)
	}
}

func TestTypeAheadIsRuneSafe(t *testing.T) {
	items := []MenuItem{{Label: "丁香"}, {Label: "中文"}, {Label: "中间"}}
	if got := typeAhead(items, "中", -1); got != 1 {
		t.Fatalf("中 matched %d, want 1 (a byte slice would match 丁香)", got)
	}
	if got := typeAhead(items, "中", 1); got != 2 {
		t.Fatalf("repeated 中 = %d, want the next match 2", got)
	}
	if got := typeAhead(items, "中中", 1); got != 2 {
		t.Fatalf("doubled 中 = %d, want cycling", got)
	}
}
