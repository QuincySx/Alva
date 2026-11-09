// INPUT: The K3 review findings: unbounded tool output, nil pointers, Composer key rules, Question exclusivity and validation, busy forms, gating layers.
// OUTPUT: One regression test per finding.
// POS: [L7 Kit blocks tests] Frame-time budgets are generous (they run under -race); the point is that 200k characters cannot take seconds.
//go:build desktop

package blocks

import (
	"strings"
	"testing"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/audit"
	"github.com/QuincySx/Alva/apps/ui/kit/kittest"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

func TestClipFiltersControlsAndBoundsSize(t *testing.T) {
	dirty := "a\x00b\x1b[31mc‮d⁦e‏f\x85g\x7fh\ti\r\nj"
	got, dropped := clip(dirty, 10, 1<<10)
	for _, r := range got {
		if r != '\t' && r != '\n' && !printable(r) {
			t.Fatalf("control or bidi rune %U survived", r)
		}
	}
	if !strings.Contains(got, "\ti") || !strings.Contains(got, "\nj") || dropped != 0 {
		t.Fatalf("tab or newline lost, or something dropped: %q %d", got, dropped)
	}
	lines, dropped := clip(strings.Repeat("行\n", 500), 200, 1<<20)
	if strings.Count(lines, "\n") != 199 || dropped != 300 {
		t.Fatalf("line cap: %d newlines, %d dropped", strings.Count(lines, "\n"), dropped)
	}
	capped, droppedByConst := clip(strings.Repeat("a\n", 5000), outputMaxLines, outputMaxBytes)
	if strings.Count(capped, "\n") >= outputMaxLines || droppedByConst == 0 {
		t.Fatalf("the output line constant does not cap: %d newlines", strings.Count(capped, "\n"))
	}
	bytes, dropped := clip(strings.Repeat("你", 10000), 100, 1000)
	if len(bytes) > 1000 || !utf8.ValidString(bytes) || dropped < 1 {
		t.Fatalf("byte cap broke: %d bytes valid=%v dropped=%d", len(bytes), utf8.ValidString(bytes), dropped)
	}
	if one := oneLine("a\nb\tc"+strings.Repeat("x", 500), 64); strings.ContainsAny(one, "\n\t") || len(one) > 64 {
		t.Fatalf("oneLine: %q", one)
	}
}

func TestToolActivityHugeOutputRendersWithinBudgetAndIsBounded(t *testing.T) {
	cases := map[string]string{
		"200k characters":   strings.Repeat("x", 200_000),
		"40k lines":         strings.Repeat("line\x00\x1b[31m‮\n", 40_000),
		"200k wide runes":   strings.Repeat("字", 200_000),
		"huge detail+title": "",
	}
	for name, text := range cases {
		start := time.Now()
		tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
			ToolActivity(c, ToolActivityView{Title: oneBig(text, "标题"), Detail: oneBig(text, "详情"), Output: text}, true, nil, "t").Width(sampleRow)
		}), 480, 480)
		if took := time.Since(start); took > 3*time.Second {
			t.Fatalf("%s: first frame took %v", name, took)
		}
		if text != "" && !tester.HasText("已截断") {
			t.Fatalf("%s: no truncation notice", name)
		}
		for _, s := range tester.Texts() {
			if text == "" && len(s) > lineMaxBytes+titleMaxBytes {
				t.Fatalf("%s: an unclipped title or detail of %d bytes reached the screen", name, len(s))
			}
			if strings.ContainsAny(s, "\x00\x1b‮") {
				t.Fatalf("%s: control characters reached the screen: %q", name, s)
			}
		}
		if out, ok := tester.Find("已截断"); ok && out.Y > 480+outputMaxHeight*100 {
			t.Fatal("truncation notice is nowhere near the output area")
		}
	}
}

func oneBig(text, fallback string) string {
	if text == "" {
		return strings.Repeat("长", 50_000)
	}
	return fallback
}

func TestToolActivityOutputAreaHasAMaxHeightAndScrolls(t *testing.T) {
	long := strings.Repeat("一行内容\n", 150)
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		ToolActivity(c, ToolActivityView{Title: "输出", Output: long}, true, nil, "t").Width(sampleRow)
	}), 480, 640)
	head, ok := tester.Find("输出")
	if !ok {
		t.Fatal("activity missing")
	}
	// Just inside the area's last allowed pixel row the Subtle fill shows; one margin below the cap it is the page surface.
	inside := kittest.PixelAt(tester, 480, head.X+head.W-6, head.Y+head.H+outputMaxHeight-6)
	below := kittest.PixelAt(tester, 480, head.X+head.W-6, head.Y+head.H+outputMaxHeight+theme.Space6)
	if !kittest.Near(inside, theme.UI(theme.Light.Subtle)) || !kittest.Near(below, theme.UI(theme.Light.Surface)) {
		t.Fatalf("the output area is not capped at %d: inside %v below %v", outputMaxHeight, inside, below)
	}
}

func TestToolKindOutOfRangeFallsBackToOther(t *testing.T) {
	if toolGlyph(ToolKind(99)) != icon.Sparkles || toolGlyph(ToolOther) != icon.Sparkles {
		t.Fatal("an out-of-range kind did not fall back to Other")
	}
	ui.NewTester(kittest.Centered(func(c *ui.Context) { ToolActivity(c, ToolActivityView{Kind: 99, Title: "未知"}, false, nil, "t") }), 320, 100)
}

func auditHas(entries func() []audit.Entry, contains string) bool {
	for _, e := range entries() {
		if strings.Contains(e.Kind, contains) && e.Name == "" {
			return true
		}
	}
	return false
}

func TestNilRequiredPointersDoNotPanicAndAreAudited(t *testing.T) {
	_, entries := kittest.Names(t)
	ui.NewTester(kittest.Centered(func(c *ui.Context) { Composer(c, ComposerView{}, ComposerActions{}, "k") }), 640, 360)
	if !auditHas(entries, "Draft is required") {
		t.Fatal("a nil Draft was not reported")
	}
	ui.NewTester(kittest.Centered(func(c *ui.Context) { PermissionMenu(c, PermissionAsk, nil, nil, "p") }), 320, 200)
	if !auditHas(entries, "open is required") {
		t.Fatal("a nil open was not reported")
	}
	ui.NewTester(kittest.Centered(func(c *ui.Context) { ModelForm(c, ModelFormView{}, ModelFormActions{}, nil, "m") }), 560, 400)
	if !auditHas(entries, "ID and Name are required") {
		t.Fatal("nil model form pointers were not reported")
	}
	ui.NewTester(kittest.Centered(func(c *ui.Context) {
		Question(c, QuestionView{Prompt: "q", AllowOther: true, Selected: []bool{true}}, QuestionActions{}, "q")
	}), 480, 300)
	if !auditHas(entries, "Other is required") {
		t.Fatal("AllowOther without Other was not reported")
	}
}

func TestPermissionMenuIsAudited(t *testing.T) {
	_, entries := kittest.Names(t)
	open := false
	ui.NewTester(kittest.Centered(func(c *ui.Context) { PermissionMenu(c, PermissionFull, &open, nil, "p") }), 320, 200)
	for _, e := range entries() {
		if e.Kind == "PermissionMenu" && e.Name == "权限模式" {
			return
		}
	}
	t.Fatal("PermissionMenu did not report its name")
}

func TestComposerEnterTable(t *testing.T) {
	send, queue, interject := func() {}, func() {}, func() {}
	for name, tc := range map[string]struct {
		phase                               ComposerPhase
		blank, menu, composing, cmd, upload bool
		a                                   ComposerActions
		wantRun                             string
		wantConsume                         bool
	}{
		"idle enter sends":            {phase: PhaseReady, a: ComposerActions{OnSend: send}, wantRun: "send", wantConsume: true},
		"idle cmd+enter sends":        {phase: PhaseReady, cmd: true, a: ComposerActions{OnSend: send}, wantRun: "send", wantConsume: true},
		"blank enter is swallowed":    {phase: PhaseIdle, blank: true, a: ComposerActions{OnSend: send}, wantConsume: true},
		"blank cmd+enter is taken":    {phase: PhaseIdle, blank: true, cmd: true, a: ComposerActions{OnSend: send}, wantConsume: true},
		"composing never submits":     {phase: PhaseReady, composing: true, a: ComposerActions{OnSend: send}, wantConsume: true},
		"open menu never submits":     {phase: PhaseReady, menu: true, a: ComposerActions{OnSend: send}, wantConsume: true},
		"locked enter stays a break":  {phase: PhaseSubmitting, a: ComposerActions{OnSend: send}},
		"locked cmd+enter is taken":   {phase: PhaseDisabled, cmd: true, a: ComposerActions{OnSend: send}, wantConsume: true},
		"uploading enter is a break":  {phase: PhaseAttaching, upload: true, a: ComposerActions{OnSend: send}},
		"running enter queues":        {phase: PhaseRunning, a: ComposerActions{OnQueue: queue, OnInterject: interject}, wantRun: "queue", wantConsume: true},
		"running cmd+enter interject": {phase: PhaseRunning, cmd: true, a: ComposerActions{OnQueue: queue, OnInterject: interject}, wantRun: "interject", wantConsume: true},
		"interject falls back":        {phase: PhaseWaiting, cmd: true, a: ComposerActions{OnQueue: queue}, wantRun: "queue", wantConsume: true},
		"stopping does nothing":       {phase: PhaseStopping, a: ComposerActions{OnQueue: queue}},
		"no callback stays a break":   {phase: PhaseReady},
	} {
		tc := tc
		calls := map[string]*int{"send": new(int), "queue": new(int), "interject": new(int)}
		a := tc.a
		if a.OnSend != nil {
			a.OnSend = func() { *calls["send"]++ }
		}
		if a.OnQueue != nil {
			a.OnQueue = func() { *calls["queue"]++ }
		}
		if a.OnInterject != nil {
			a.OnInterject = func() { *calls["interject"]++ }
		}
		run, consume := composerEnter(tc.phase, tc.blank, tc.menu, tc.composing, tc.cmd, tc.upload, a)
		if run != nil {
			run()
		}
		for kind, n := range calls {
			if want := map[bool]int{true: 1}[kind == tc.wantRun]; *n != want {
				t.Errorf("%s: %s ran %d times, want %d", name, kind, *n, want)
			}
		}
		if consume != tc.wantConsume {
			t.Errorf("%s: consume = %v, want %v", name, consume, tc.wantConsume)
		}
	}
}

func TestComposerEnterAndShiftEnterThroughTheEditor(t *testing.T) {
	sent := 0
	draft := ""
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		Composer(c, ComposerView{Draft: &draft, Phase: PhaseReady}, ComposerActions{OnSend: func() { sent++ }}, "c").Width(sampleWide)
	}), 640, 360)
	tester.Key(0, ui.KeyTab)
	tester.Type("你好")
	tester.Key(ui.Shift, ui.KeyEnter)
	if sent != 0 || !strings.Contains(draft, "\n") {
		t.Fatalf("Shift+Enter did not break the line: sent=%d draft=%q", sent, draft)
	}
	tester.Key(0, ui.KeyEnter)
	if sent != 1 || !strings.HasPrefix(draft, "你好") {
		t.Fatalf("Enter did not send once with the typed draft: sent=%d draft=%q", sent, draft)
	}
}

func TestComposerEnterWhileComposingAndWithBlankDraftDoesNotSend(t *testing.T) {
	sent := 0
	draft := ""
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		Composer(c, ComposerView{Draft: &draft, Phase: PhaseReady}, ComposerActions{OnSend: func() { sent++ }}, "c").Width(sampleWide)
	}), 640, 360)
	tester.Key(0, ui.KeyTab)
	tester.Key(0, ui.KeyEnter)
	if sent != 0 || draft != "" {
		t.Fatalf("Enter on a blank draft sent or added a line: sent=%d draft=%q", sent, draft)
	}
	tester.Compose("ni", 2)
	tester.Key(0, ui.KeyEnter)
	if sent != 0 {
		t.Fatal("Enter during composition sent")
	}
}

func TestComposerRunningEnterQueuesAndCmdEnterInterjects(t *testing.T) {
	queued, interjected := 0, 0
	draft := "补充一句"
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		Composer(c, ComposerView{Draft: &draft, Phase: PhaseRunning}, ComposerActions{OnQueue: func() { queued++ }, OnInterject: func() { interjected++ }}, "c").Width(sampleWide)
	}), 640, 360)
	tester.Key(0, ui.KeyTab)
	tester.Key(0, ui.KeyEnter)
	tester.Key(ui.Cmd, ui.KeyEnter)
	if queued != 1 || interjected != 1 {
		t.Fatalf("running keys: queued=%d interjected=%d", queued, interjected)
	}
}

func TestQuestionSingleChoiceIsExclusiveAndMultiFlips(t *testing.T) {
	v := QuestionView{Options: []OptionView{{Label: "甲"}, {Label: "乙"}, {Label: "丙"}}, Selected: []bool{true, false, false}}
	if got := nextSelection(v, 2); got[0] || got[1] || !got[2] {
		t.Fatalf("single choice not exclusive: %v", got)
	}
	if got := nextSelection(v, 0); !got[0] {
		t.Fatalf("clicking the chosen single option must keep it chosen: %v", got)
	}
	v.Multi = true
	if got := nextSelection(v, 1); !got[0] || !got[1] || got[2] {
		t.Fatalf("multi did not flip: %v", got)
	}
	if got := nextSelection(v, 0); got[0] {
		t.Fatalf("multi did not unflip: %v", got)
	}
	if got := nextSelection(v, 9); len(got) != 3 {
		t.Fatal("an out-of-range click changed the shape")
	}
}

func TestQuestionOtherFieldValidationAndSubmit(t *testing.T) {
	selected := []bool{false, false, true}
	other := ""
	submits := 0
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		Question(c, QuestionView{Prompt: "选一项", Options: []OptionView{{Label: "甲"}, {Label: "乙"}}, AllowOther: true, Other: &other, Selected: selected},
			QuestionActions{OnChange: func(next []bool) { selected = next }, OnSubmit: func() { submits++ }}, "q")
	}), 520, 420)
	if !tester.HasText("其他") {
		t.Fatal("the free-text option is missing")
	}
	_ = tester.Click("提交")
	if submits != 0 {
		t.Fatal("submitted an empty free-text answer")
	}
	// The input is the first focusable after the options: Tab through the three option rows.
	for range 4 {
		tester.Key(0, ui.KeyTab)
		if tester.Focused("其他答案") {
			break
		}
	}
	tester.Type("自定义答案")
	if other != "自定义答案" {
		t.Fatalf("the free-text field did not edit *Other: %q", other)
	}
	tester.Frame()
	if err := tester.Click("提交"); err != nil || submits != 1 {
		t.Fatal("a filled free-text answer could not be submitted", err, submits)
	}
	if !questionValid(QuestionView{Options: []OptionView{{Label: "甲"}}, Selected: []bool{true}}) || questionValid(QuestionView{Options: []OptionView{{Label: "甲"}}}) {
		t.Fatal("questionValid wrong for plain options")
	}
}

func TestBusyFormsAreReadOnlyAsAWhole(t *testing.T) {
	name, url, id, display := "", "", "m-1", "显示名"
	source := SourcePreset
	switched := 0
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		ui.Column(c).Gap(theme.Space4).Children(func() {
			ProviderForm(c, ProviderFormView{Source: source, Busy: true}, ProviderFormActions{OnSource: func(ProviderSource) { switched++ }}, func() {
				kitInput(c, &name, "名称")
				kitInput(c, &url, "服务地址")
			}, "pf")
		})
	}), 640, 520)
	_ = tester.Click("自定义")
	if switched != 0 || tester.Focused("自定义") {
		t.Fatal("the source switch changed or took focus while busy")
	}
	tester.Key(0, ui.KeyTab)
	if tester.Focused("预设") || tester.Focused("自定义") {
		t.Fatal("the source segments are still keyboard stops while busy")
	}
	tester.Type("改")
	if name != "" || url != "" {
		t.Fatal("a caller field accepted input while the provider form was busy", name, url)
	}
	model := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		ModelForm(c, ModelFormView{Mode: ModelAdd, ID: &id, Name: &display, Busy: true}, ModelFormActions{}, nil, "mf")
	}), 560, 420)
	for range 3 {
		model.Key(0, ui.KeyTab)
		model.Type("x")
	}
	if id != "m-1" || display != "显示名" {
		t.Fatal("model form fields changed while busy", id, display)
	}
}

func TestBusinessGateIsSeparateFromKitsOwnGate(t *testing.T) {
	fn := func() {}
	for _, status := range []AskStatus{AskBusy, AskAllowed, AskDenied, AskAnswered, AskExpired} {
		if allow, deny := approvalHandlers(ApprovalView{Status: status}, ApprovalActions{OnAllow: fn, OnDeny: fn}); allow != nil || deny != nil {
			t.Fatalf("status %d hands live callbacks to the buttons", status)
		}
	}
	for _, status := range []AskStatus{AskPending, AskError} {
		if allow, deny := approvalHandlers(ApprovalView{Status: status}, ApprovalActions{OnAllow: fn, OnDeny: fn}); allow == nil || deny == nil {
			t.Fatalf("status %d must be decidable", status)
		}
	}
	live := QuestionView{Options: []OptionView{{Label: "甲"}}, Selected: []bool{true}, Status: AskPending}
	if questionSubmit(live, QuestionActions{OnSubmit: fn}) == nil {
		t.Fatal("a valid live question cannot be submitted")
	}
	for name, v := range map[string]QuestionView{
		"busy":      {Options: live.Options, Selected: live.Selected, Status: AskBusy},
		"expired":   {Options: live.Options, Selected: live.Selected, Status: AskExpired},
		"no choice": {Options: live.Options, Selected: []bool{false}, Status: AskPending},
	} {
		if questionSubmit(v, QuestionActions{OnSubmit: fn}) != nil {
			t.Fatalf("%s: submit is live", name)
		}
	}
}
