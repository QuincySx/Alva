// INPUT: Composer, VoiceInput, ComposerMenu, PermissionMenu and the form shells under ui.Tester with real typing, clicks and keys.
// OUTPUT: Proof of send/stop gating, locked editing per phase, busy without repeat, slots, menus and form callbacks.
// POS: [L7 Kit blocks tests] Queue and interjection behavior is out of scope (M2): only the Queued count and the Queue slot are checked.
//go:build desktop

package blocks

import (
	"testing"

	"github.com/QuincySx/Alva/apps/ui/kit/icon"
	"github.com/QuincySx/Alva/apps/ui/kit/internal/force"
	"github.com/QuincySx/Alva/apps/ui/kit/kittest"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

type composerCalls struct{ send, stop, attach, voice, remove int }

func composerTester(draft *string, phase ComposerPhase, n *composerCalls, tweak func(*ComposerView, *ComposerActions)) *ui.Tester {
	return ui.NewTester(kittest.Centered(func(c *ui.Context) {
		v := ComposerView{Draft: draft, Phase: phase}
		a := ComposerActions{OnSend: func() { n.send++ }, OnStop: func() { n.stop++ }, OnAttach: func() { n.attach++ }, OnVoice: func() { n.voice++ }, OnRemoveAttachment: func(int) { n.remove++ }}
		if tweak != nil {
			tweak(&v, &a)
		}
		Composer(c, v, a, "composer").Width(sampleWide)
	}), 640, 360)
}

func TestComposerSendNeedsADraftAndStopReplacesItWhileRunning(t *testing.T) {
	n := &composerCalls{}
	empty := ""
	idle := composerTester(&empty, PhaseIdle, n, nil)
	_ = idle.Click("发送")
	if n.send != 0 {
		t.Fatal("send fired with an empty draft")
	}
	draft := "你好"
	ready := composerTester(&draft, PhaseReady, n, nil)
	if err := ready.Click("发送"); err != nil || n.send != 1 {
		t.Fatal("ready send did not fire", err, n.send)
	}
	if r, _ := ready.Find("发送"); r.W != roundAction || r.H != roundAction {
		t.Fatal("send is not the 32px round action", r)
	}
	running := composerTester(&draft, PhaseRunning, n, nil)
	if running.HasText("发送") && !running.HasText("停止") {
		t.Fatal("a running composer offers Send instead of Stop")
	}
	if err := running.Click("停止"); err != nil || n.stop != 1 {
		t.Fatal("stop did not fire", err, n.stop)
	}
}

func TestComposerBusyPhasesNeverRepeatAndKeepNameAndFocus(t *testing.T) {
	for _, phase := range []ComposerPhase{PhaseSubmitting, PhaseStopping} {
		n := &composerCalls{}
		draft := "你好"
		tester := composerTester(&draft, phase, n, nil)
		name := map[ComposerPhase]string{PhaseSubmitting: "发送", PhaseStopping: "停止"}[phase]
		if !tester.HasText("Loading indicator") {
			t.Fatal("busy phase has no loader")
		}
		_ = tester.Click(name)
		for range 4 {
			tester.Key(0, ui.KeyTab)
		}
		tester.Key(0, ui.KeyEnter)
		if n.send+n.stop != 0 {
			t.Fatalf("phase %d fired again", phase)
		}
	}
}

func TestComposerLockedPhasesRejectTyping(t *testing.T) {
	for _, phase := range []ComposerPhase{PhaseSubmitting, PhaseRecording, PhaseTranscribing, PhaseDisabled} {
		draft := "原文"
		tester := composerTester(&draft, phase, &composerCalls{}, nil)
		tester.Key(0, ui.KeyTab)
		tester.Type("改")
		if draft != "原文" {
			t.Fatalf("phase %d accepted typing", phase)
		}
	}
	draft := ""
	free := composerTester(&draft, PhaseIdle, &composerCalls{}, nil)
	_ = free.Click("消息输入")
	free.Type("你好")
	if draft != "你好" {
		t.Fatal("an idle composer rejects typing", draft)
	}
}

func TestComposerSlotsQueueAttachmentsAndNotice(t *testing.T) {
	n := &composerCalls{}
	draft := "草稿"
	tester := composerTester(&draft, PhaseRunning, n, func(v *ComposerView, a *ComposerActions) {
		v.Queued = 3
		v.Attachments = []AttachmentView{{Name: "资料.pdf", Detail: "128 KB"}}
		v.Notice = "提示一行"
		a.Queue = func() {}
		a.Toolbar = func() {}
	})
	for _, s := range []string{"已排队 3 条", "资料.pdf", "提示一行"} {
		if !tester.HasText(s) {
			t.Fatalf("%q missing", s)
		}
	}
	if err := tester.Click("移除 资料.pdf"); err != nil || n.remove != 1 {
		t.Fatal("attachment removal not wired", err, n.remove)
	}
	if err := tester.Click("添加"); err != nil || n.attach != 1 {
		t.Fatal("add not wired", err, n.attach)
	}
}

func TestComposerAttachingBlocksSendUntilReady(t *testing.T) {
	n := &composerCalls{}
	draft := "草稿"
	tester := composerTester(&draft, PhaseAttaching, n, func(v *ComposerView, _ *ComposerActions) {
		v.Attachments = []AttachmentView{{Name: "资料.pdf", State: AttachmentUploading}}
	})
	_ = tester.Click("发送")
	if n.send != 0 {
		t.Fatal("send fired while an attachment is uploading")
	}
}

func TestComposerBordersFollowFocusAndFailure(t *testing.T) {
	draft := ""
	focus := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		force.With(force.Focus, func() { Composer(c, ComposerView{Draft: &draft}, ComposerActions{}, "c").Width(sampleWide) })
	}), 640, 360)
	r, _ := focus.Find("输入框")
	if px := kittest.PixelAt(focus, 640, r.X+r.W/2, r.Y+.5); !kittest.Within(px, theme.UI(theme.Light.Accent), 14) {
		t.Fatal("focused composer lost its accent border", px)
	}
	failed := composerTester(&draft, PhaseFailed, &composerCalls{}, func(v *ComposerView, _ *ComposerActions) { v.Notice = "发送失败" })
	f, _ := failed.Find("输入框")
	if px := kittest.PixelAt(failed, 640, f.X+f.W/2, f.Y+.5); !kittest.Within(px, theme.UI(theme.Light.Danger), 14) {
		t.Fatal("failed composer lost its danger border", px)
	}
}

func TestVoiceInputStatesGateAndName(t *testing.T) {
	for state, label := range map[VoiceState]string{VoiceIdle: "开始语音输入", VoiceRecording: "停止录音", VoiceTranscribing: "正在转写", VoiceError: "语音输入出错，重试"} {
		calls := 0
		tester := ui.NewTester(kittest.Centered(func(c *ui.Context) { VoiceInput(c, state, "00:05", false, func() { calls++ }, "v") }), 240, 120)
		if r, ok := tester.Find(label); !ok || r.H != roundAction {
			t.Fatalf("%s missing or not 32px", label)
		}
		_ = tester.Click(label)
		want := 1
		if state == VoiceTranscribing {
			want = 0
			if !tester.HasText("Loading indicator") {
				t.Fatal("transcribing has no loader")
			}
		}
		if calls != want {
			t.Fatalf("%s fired %d times, want %d", label, calls, want)
		}
	}
	off := 0
	disabled := ui.NewTester(kittest.Centered(func(c *ui.Context) { VoiceInput(c, VoiceIdle, "", true, func() { off++ }, "v") }), 240, 120)
	_ = disabled.Click("开始语音输入")
	if off != 0 {
		t.Fatal("disabled voice input fired")
	}
}

func TestPermissionMenuChoosesAMode(t *testing.T) {
	open, mode := false, PermissionAsk
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		PermissionMenu(c, mode, &open, func(m PermissionMode) { mode = m }, "perm")
	}), 400, 320)
	if !tester.HasText("询问") {
		t.Fatal("trigger does not name the mode")
	}
	_ = tester.Click("询问")
	if !open || !tester.HasText("完全访问") {
		t.Fatal("menu did not open with the three modes")
	}
	if err := tester.Click("自动"); err != nil || mode != PermissionAuto || open {
		t.Fatal("choosing a mode failed", err, mode)
	}
}

func TestComposerMenuViews(t *testing.T) {
	picked, retries := -1, 0
	for kind, want := range map[ComposerMenuKind]string{MenuLoading: "正在加载", MenuEmpty: "没有匹配", MenuError: "加载失败"} {
		tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
			ComposerMenu(c, ComposerMenuView{Kind: kind, Message: map[ComposerMenuKind]string{MenuEmpty: "没有匹配", MenuError: "加载失败"}[kind]}, func(int) {}, func() { retries++ }, "m")
		}), 420, 360)
		if !tester.HasText(want) {
			t.Fatalf("kind %d missing %q", kind, want)
		}
		if kind == MenuError {
			_ = tester.Click("重试")
		}
	}
	if retries != 1 {
		t.Fatal("retry not wired", retries)
	}
	list := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		ComposerMenu(c, ComposerMenuView{Kind: MenuCommands, Items: []ComposerMenuItem{{Label: "总结", Icon: icon.Sparkles}, {Label: "压缩", Icon: icon.Database, Shortcut: "/compact"}}}, func(i int) { picked = i }, nil, "m")
	}), 420, 360)
	if err := list.Click("压缩"); err != nil || picked != 1 || !list.HasText("/compact") {
		t.Fatal("entry pick or shortcut missing", err, picked)
	}
	if r, _ := list.Find("撰写菜单"); r.W != composerMenuWidth {
		t.Fatal("menu width", r.W)
	}
}

func TestProviderFormSaveGatingSourceSwitchAndFooter(t *testing.T) {
	saves, cancels := 0, 0
	source := SourcePreset
	view := func(busy bool) *ui.Tester {
		return ui.NewTester(kittest.Centered(func(c *ui.Context) {
			ProviderForm(c, ProviderFormView{Source: source, Busy: busy}, ProviderFormActions{OnSource: func(s ProviderSource) { source = s }, OnSave: func() { saves++ }, OnCancel: func() { cancels++ }}, func() {}, "f")
		}), 560, 360)
	}
	tester := view(false)
	_ = tester.Click("自定义")
	if source != SourceCustom {
		t.Fatal("source switch not reported", source)
	}
	_ = tester.Click("保存")
	_ = tester.Click("取消")
	if saves != 1 || cancels != 1 {
		t.Fatal("footer actions", saves, cancels)
	}
	busy := view(true)
	_ = busy.Click("保存")
	_ = busy.Click("取消")
	if saves != 1 || cancels != 1 || !busy.HasText("Loading indicator") {
		t.Fatal("a busy form fired or lost its loader", saves, cancels)
	}
}

func TestModelFormEditKeepsTheIDReadOnly(t *testing.T) {
	id, name := "sample-model", "示例"
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		ModelForm(c, ModelFormView{Mode: ModelEdit, ID: &id, Name: &name}, ModelFormActions{}, nil, "f")
	}), 560, 420)
	if !tester.HasText("编辑模型") || !tester.HasText("已添加的模型不能改 ID") {
		t.Fatal("edit title or ID hint missing")
	}
	tester.Key(0, ui.KeyTab)
	if !tester.Focused("模型 ID") {
		t.Fatal("the read-only ID is not focusable (it must stay selectable)")
	}
	tester.Type("x")
	if id != "sample-model" {
		t.Fatal("the model ID was edited in Edit mode", id)
	}
	add := ""
	adding := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		ModelForm(c, ModelFormView{Mode: ModelAdd, ID: &add, Name: &name}, ModelFormActions{}, nil, "f")
	}), 560, 420)
	adding.Key(0, ui.KeyTab)
	adding.Type("new-model")
	if add != "new-model" {
		t.Fatal("the model ID is not editable in Add mode", add)
	}
}

func TestProviderModelsStatesAndFetchGating(t *testing.T) {
	fetches, favs := 0, 0
	texts := map[CatalogState]string{CatalogIdle: "还没有获取模型", CatalogEmpty: "没有可用的模型", CatalogUnavailable: "暂时无法获取", CatalogError: "获取失败", CatalogReady: "示例模型"}
	for state, want := range texts {
		tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
			ProviderModels(c, ProviderModelsView{State: state, Models: []ModelRowView{{Name: "示例模型"}}}, ProviderModelsActions{OnFetch: func() { fetches++ },
				Row: func(int) ModelRowActions { return ModelRowActions{OnToggleFavorite: func() { favs++ }} }}, "p").Width(sampleRow)
		}), 480, 360)
		if !tester.HasText(want) {
			t.Fatalf("state %d missing %q", state, want)
		}
		_ = tester.Click("获取模型")
		if state == CatalogReady {
			_ = tester.Click("收藏 示例模型")
		}
	}
	// Idle, Empty, Error and Ready fetch; Unavailable is disabled.
	if fetches != 4 || favs != 1 {
		t.Fatal("fetch gating wrong", fetches, favs)
	}
	loading := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		ProviderModels(c, ProviderModelsView{State: CatalogLoading}, ProviderModelsActions{OnFetch: func() { fetches += 100 }}, "p").Width(sampleRow)
	}), 480, 360)
	_ = loading.Click("获取模型")
	if fetches != 4 || !loading.HasText("加载中") {
		t.Fatal("a loading catalog fetched again or lost its skeleton", fetches)
	}
}
