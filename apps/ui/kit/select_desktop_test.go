// INPUT: Kit Select[T] under ui.Tester with real clicks and keys.
// OUTPUT: Interaction (open, arrows, Enter, Escape, disabled options), geometry and look proof, including the generic non-string value.
// POS: [L7 Kit tests] The popup is MyGo's; these tests pin what the kit adds: options, check, descriptions, plain look and placeholder.
//go:build desktop

package kit

import (
	"testing"

	"github.com/QuincySx/Alva/apps/ui/kit/kittest"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

type effort int

const (
	low effort = iota + 1
	medium
	high
)

func effortOptions() []Option[effort] {
	return []Option[effort]{
		{Value: low, Label: "低", Description: "更快更省"},
		{Value: medium, Label: "中"},
		{Value: high, Label: "高", Disabled: true},
	}
}

func TestSelectOpensChoosesByKeyboardAndSkipsDisabled(t *testing.T) {
	value, changed := low, 0
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		if Select(c, &value, effortOptions(), SelectProps{Label: "推理强度", Key: "effort"}).Changed() {
			changed++
		}
	}), 360, 320)
	r, ok := tester.Find("推理强度")
	if !ok || r.H != theme.ControlHeight {
		t.Fatal("select trigger missing or not 32px", r)
	}
	tester.Key(0, ui.KeyTab)
	tester.Key(0, ui.KeyDown)
	if !tester.HasText("更快更省") {
		t.Fatal("popup did not open with the option description")
	}
	tester.Key(0, ui.KeyDown)
	tester.Key(0, ui.KeyDown) // would be the disabled option
	tester.Key(0, ui.KeyEnter)
	if value != medium || changed != 1 {
		t.Fatal("keyboard choice failed or chose the disabled option", value, changed)
	}
	if tester.HasText("更快更省") {
		t.Fatal("popup stayed open after choosing")
	}
}

func TestSelectClickChoosesAndEscapeClosesKeepingValue(t *testing.T) {
	value := low
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		Select(c, &value, effortOptions(), SelectProps{Label: "推理强度", Key: "effort"})
	}), 360, 320)
	_ = tester.Click("推理强度")
	if err := tester.Click("中"); err != nil || value != medium {
		t.Fatal("clicking an option did not choose it", err, value)
	}
	_ = tester.Click("推理强度")
	tester.Key(0, ui.KeyEscape)
	if value != medium || tester.HasText("更快更省") {
		t.Fatal("Escape changed the value or left the popup open", value)
	}
	if !tester.Focused("推理强度") {
		t.Fatal("focus did not return to the trigger after Escape")
	}
}

func TestSelectPlaceholderPlainAndDisabled(t *testing.T) {
	var none effort
	value := low
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		ui.Column(c).Gap(theme.Space3).Children(func() {
			Select(c, &none, effortOptions(), SelectProps{Label: "未选", Placeholder: "请选择强度", Key: "a"})
			Select(c, &value, effortOptions(), SelectProps{Label: "值行", Plain: true, Key: "b"})
			Select(c, &value, effortOptions(), SelectProps{Label: "停用", Disabled: true, Key: "c"})
		})
	}), 360, 260)
	if !tester.HasText("请选择强度") {
		t.Fatal("placeholder missing while no option matches")
	}
	plain, _ := tester.Find("值行")
	if got := kittest.PixelAt(tester, 360, plain.X+plain.W/2, plain.Y+.5); !kittest.Near(got, theme.UI(theme.Light.Surface)) {
		t.Fatal("plain select drew a border", got)
	}
	bordered, _ := tester.Find("未选")
	if got := kittest.PixelAt(tester, 360, bordered.X+bordered.W/2, bordered.Y+.5); !kittest.Near(got, theme.UI(theme.Light.Border)) {
		t.Fatal("bordered select lost its border", got)
	}
	_ = tester.Click("停用")
	if tester.HasText("更快更省") {
		t.Fatal("disabled select opened")
	}
}

func TestSelectInvalidAndBusyAndFieldName(t *testing.T) {
	value := low
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		Field(c, FieldProps{Title: "模型", Error: "必须选择"}, func() {
			Select(c, &value, effortOptions(), SelectProps{Busy: true, Key: "m"})
		})
	}), 360, 200)
	if _, ok := tester.Find("模型"); !ok {
		t.Fatal("Field did not name the select")
	}
	if !tester.HasText("Loading indicator") || !tester.HasText("必须选择") {
		t.Fatal("busy select has no loader or the Field lost its error text")
	}
}
