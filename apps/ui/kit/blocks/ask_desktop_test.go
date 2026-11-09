// INPUT: ToolActivity, Approval, Question and QuestionOption under ui.Tester with real clicks and keys.
// OUTPUT: Proof of expansion callbacks, decision gating, busy exposure without repeat, status rendering and keyboard reachability.
// POS: [L7 Kit blocks tests] The cards never decide safety; these tests pin that they cannot fire twice and always stay named and focusable.
//go:build desktop

package blocks

import (
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/ui/kit/internal/cells"
	"github.com/QuincySx/Alva/apps/ui/kit/kittest"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

func TestToolActivityTogglesByClickEnterAndSpace(t *testing.T) {
	expanded, calls := false, 0
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		ToolActivity(c, ToolActivityView{Kind: ToolSearch, Title: "搜索资料", Detail: "关键词", Output: "找到 3 条结果"}, expanded, func(on bool) { expanded = on; calls++ }, "t").Width(sampleRow)
	}), 420, 200)
	if tester.HasText("找到 3 条结果") {
		t.Fatal("collapsed activity shows its output")
	}
	if err := tester.Click("搜索资料"); err != nil || !expanded {
		t.Fatal("click did not expand", err)
	}
	tester.Frame()
	if !tester.HasText("找到 3 条结果") {
		t.Fatal("expanded output missing")
	}
	tester.Key(0, ui.KeyTab)
	tester.Key(0, ui.KeyEnter)
	if expanded || calls != 2 {
		t.Fatal("Enter did not collapse", expanded, calls)
	}
	tester.Key(0, ui.KeySpace)
	if !expanded {
		t.Fatal("Space did not expand")
	}
}

func TestToolActivityStatusesAndKinds(t *testing.T) {
	for kind := ToolCommand; kind <= ToolOther; kind++ {
		if toolGlyph(kind) == "" {
			t.Fatalf("kind %d has no icon", kind)
		}
	}
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		ui.Column(c).Width(sampleRow).Gap(theme.Space2).Children(func() {
			ToolActivity(c, ToolActivityView{Title: "运行中", Status: ToolRunning}, false, nil, "a")
			ToolActivity(c, ToolActivityView{Title: "已失败", Status: ToolError, Output: "错误详情"}, true, nil, "b")
		})
	}), 420, 240)
	if !tester.HasText("Loading indicator") || !tester.HasText("失败") || !tester.HasText("错误详情") {
		t.Fatal("running loader, error badge or error output missing")
	}
	tester.Key(0, ui.KeyTab)
	if !tester.Focused("运行中") {
		t.Fatal("the header is not keyboard reachable")
	}
}

type approvalCalls struct{ allow, deny int }

func approvalView(c *ui.Context, v ApprovalView, n *approvalCalls) {
	Approval(c, v, ApprovalActions{OnAllow: func() { n.allow++ }, OnDeny: func() { n.deny++ }}, "a")
}

func TestApprovalPendingDecidesAndIsKeyboardOperable(t *testing.T) {
	n := &approvalCalls{}
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		approvalView(c, ApprovalView{Title: "需要运行命令", Detail: "ls -la"}, n)
	}), 520, 300)
	if err := tester.Click("允许"); err != nil || n.allow != 1 {
		t.Fatal("allow", err, n.allow)
	}
	tester.Key(0, ui.KeyTab)
	if !tester.Focused("拒绝") {
		t.Fatal("deny is not the first tab stop")
	}
	tester.Key(0, ui.KeyEnter)
	if n.deny != 1 {
		t.Fatal("Enter did not deny", n.deny)
	}
}

func TestApprovalBusyShowsLoaderKeepsNamesAndNeverRepeats(t *testing.T) {
	for _, choice := range []AskChoice{ChoiceAllow, ChoiceDeny} {
		n := &approvalCalls{}
		tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
			approvalView(c, ApprovalView{Title: "需要运行命令", Status: AskBusy, Choice: choice}, n)
		}), 520, 300)
		if !tester.HasText("Loading indicator") || !tester.HasText("处理中") {
			t.Fatal("busy not exposed")
		}
		for _, name := range []string{"允许", "拒绝"} {
			_ = tester.Click(name)
		}
		tester.Key(0, ui.KeyTab)
		tester.Key(0, ui.KeyEnter)
		if n.allow != 0 || n.deny != 0 {
			t.Fatalf("a busy approval fired again: %+v", n)
		}
	}
}

func TestApprovalSettledStatusesHideTheButtonsAndErrorReopensThem(t *testing.T) {
	for status, label := range map[AskStatus]string{AskAllowed: "已允许", AskDenied: "已拒绝", AskExpired: "已过期"} {
		n := &approvalCalls{}
		tester := ui.NewTester(kittest.Centered(func(c *ui.Context) { approvalView(c, ApprovalView{Title: "请求", Status: status}, n) }), 520, 240)
		if !tester.HasText(label) || tester.HasText("允许") && status != AskAllowed || n.allow+n.deny != 0 {
			t.Fatalf("status %d: label %q missing or buttons present", status, label)
		}
		if _, ok := tester.Find("拒绝"); ok && status != AskDenied {
			t.Fatalf("status %d still offers a decision", status)
		}
	}
	n := &approvalCalls{}
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		approvalView(c, ApprovalView{Title: "请求", Status: AskError, Message: "网络错误"}, n)
	}), 520, 260)
	if !tester.HasText("网络错误") || !tester.HasText("出错") {
		t.Fatal("error message missing")
	}
	_ = tester.Click("允许")
	if n.allow != 1 {
		t.Fatal("after an error the decision must be possible again")
	}
}

func TestQuestionSelectionSubmitGatingAndStatuses(t *testing.T) {
	selected := []bool{false, false}
	toggles, submits := 0, 0
	view := func(status AskStatus) func(c *ui.Context) {
		return kittest.Centered(func(c *ui.Context) {
			Question(c, QuestionView{Prompt: "选一项", Options: []OptionView{{Label: "甲"}, {Label: "乙", Description: "说明"}}, Selected: selected, Status: status},
				QuestionActions{OnChange: func(next []bool) { toggles++; selected = next }, OnSubmit: func() { submits++ }}, "q")
		})
	}
	tester := ui.NewTester(view(AskPending), 520, 360)
	_ = tester.Click("提交")
	if submits != 0 {
		t.Fatal("submit fired without a selection")
	}
	_ = tester.Click("乙")
	tester.Frame()
	if err := tester.Click("提交"); err != nil || submits != 1 || toggles != 1 {
		t.Fatal("selected submit did not fire", err, submits, toggles)
	}
	busy := ui.NewTester(view(AskBusy), 520, 360)
	_ = busy.Click("提交")
	busy.Key(0, ui.KeyTab)
	busy.Key(0, ui.KeyEnter)
	_ = busy.Click("甲")
	if submits != 1 || toggles != 1 || !busy.HasText("Loading indicator") {
		t.Fatal("a busy question fired or lost its loader", submits, toggles)
	}
	expired := ui.NewTester(view(AskExpired), 520, 360)
	_ = expired.Click("甲")
	if toggles != 1 || !expired.HasText("已过期") {
		t.Fatal("an expired question accepts input", toggles)
	}
}

func TestQuestionOptionStatesPaintAndGate(t *testing.T) {
	toggles := 0
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		ui.Column(c).Width(sampleRow).Gap(theme.Space2).Children(func() {
			QuestionOption(c, OptionView{Label: "选中项", Selected: true, Multi: true}, func() { toggles++ }, "a")
			QuestionOption(c, OptionView{Label: "停用项", Disabled: true}, func() { toggles += 10 }, "b")
		})
	}), 420, 200)
	sel, _ := tester.Find("选中项")
	if px := kittest.PixelAt(tester, 420, sel.X+sel.W-8, sel.Y+sel.H/2); !kittest.Near(px, theme.UI(theme.Light.AccentSoft)) {
		t.Fatal("selected option is not accentSoft", px)
	}
	_ = tester.Click("选中项")
	_ = tester.Click("停用项")
	if toggles != 1 {
		t.Fatal("option gating wrong", toggles)
	}
}

func TestApprovalLiveDemoRunsPendingBusyAllowed(t *testing.T) {
	cells.Reset()
	live := approvalPage().Live
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) { ui.Column(c).Gap(theme.Space2).Children(func() { live(c) }) }), 640, 360)
	if err := tester.Click("允许"); err != nil || !tester.HasText("处理中") || !tester.HasText("Loading indicator") {
		t.Fatal("the demo did not go busy after Allow", err)
	}
	tester.Advance(liveBusy + 100*time.Millisecond)
	if !tester.HasText("已允许") {
		t.Fatal("the demo did not settle to Allowed")
	}
	if err := tester.Click("重置演示"); err != nil || tester.HasText("已允许") {
		t.Fatal("reset did not return to pending", err)
	}
}
