// INPUT: Kit Tooltip on icon buttons under ui.Tester with a controlled clock, keyboard focus and Escape.
// OUTPUT: Proof the tip appears after hover rest and at once on keyboard focus, hides on Escape, paints the design tip and names its anchor.
// POS: [L7 Kit tests] Timing belongs to MyGo's TooltipBase; these pin that the kit dresses it without losing the triggers.
//go:build desktop

package kit

import (
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/kittest"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

func TestIconButtonTooltipShowsOnHoverRestAndKeyboardFocus(t *testing.T) {
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		IconButton(c, icon.Copy, "复制全文", IconButtonProps{Tooltip: "复制整条回复", Key: "copy"})
	}), 320, 160)
	r, _ := tester.Find("复制全文")
	tester.Move(r.X+r.W/2, r.Y+r.H/2)
	if tester.HasText("复制整条回复") {
		t.Fatal("tip appeared before the pointer rested")
	}
	tester.Advance(700 * time.Millisecond)
	if !tester.HasText("复制整条回复") {
		t.Fatal("tip did not appear after the pointer rested")
	}
	tester.Key(0, ui.KeyEscape)
	if tester.HasText("复制整条回复") {
		t.Fatal("Escape did not hide the tip")
	}
	tester.Move(0, 0)
	tester.Key(0, ui.KeyTab)
	if !tester.HasText("复制整条回复") {
		t.Fatal("tip did not appear at once on keyboard focus")
	}
}

func TestTooltipDefaultsToTheLabelAndPaintsTheDesignTip(t *testing.T) {
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		IconButton(c, icon.Trash, "删除会话", IconButtonProps{Key: "trash"})
	}), 320, 160)
	tester.Key(0, ui.KeyTab)
	if !tester.HasText("删除会话") {
		t.Fatal("tip missing")
	}
	// The tip's body is the surface role with a border; sample the page canvas vs the tip surface through its text box.
	var tip ui.Rect
	for _, s := range tester.Texts() {
		if s == "删除会话" {
			if rect, ok := tester.Find(s); ok {
				tip = rect
			}
		}
	}
	if tip.W == 0 {
		t.Fatal("could not locate the tip text")
	}
	if got := kittest.PixelAt(tester, 320, tip.X-theme.Space1, tip.Y+tip.H/2); !kittest.Within(got, theme.UI(theme.Light.Surface), 3) {
		t.Fatalf("tip is not the surface color: %v", got)
	}
}

func TestTooltipWithEmptyTextAttachesNothing(t *testing.T) {
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		a := ui.ButtonBase(c.Key("a")).Label("锚点")
		if Tooltip(c, a, "").Valid() {
			t.Error("an empty tooltip produced a tip")
		}
	}), 200, 100)
	_ = tester
}
