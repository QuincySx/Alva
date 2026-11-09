// INPUT: The small blocks (RunStatus, TaskRow, ConnectionRow, ModelRow, MessageHeader, Citation, Attachment, CodeBlock) under ui.Tester.
// OUTPUT: Interaction, naming, gating and pixel proof for each.
// POS: [L7 Kit blocks tests] Callbacks are asserted through counters; accessible names through the kit audit and Tester.Find by label.
//go:build desktop

package blocks

import (
	"testing"

	"github.com/QuincySx/Alva/apps/ui/kit/internal/force"
	"github.com/QuincySx/Alva/apps/ui/kit/kittest"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

func TestRunStatusShowsEveryStateWithItsLabel(t *testing.T) {
	for state := RunWorking; state <= RunError; state++ {
		label := runLabel(state)
		tester := ui.NewTester(kittest.Centered(func(c *ui.Context) { RunStatus(c, RunStatusView{State: state, Detail: "细节"}, "r") }), 320, 100)
		if !tester.HasText(label) || !tester.HasText("细节") {
			t.Fatalf("state %d lost its label or detail", state)
		}
		busy := state == RunWorking || state == RunStopping
		if tester.HasText("Loading indicator") != busy {
			t.Fatalf("state %d loader presence wrong", state)
		}
	}
}

func TestTaskAndConnectionRowsGateAndBadge(t *testing.T) {
	clicks := 0
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		ui.Column(c).Width(sampleRow).Children(func() {
			TaskRow(c, TaskRowView{Title: "任务甲", State: TaskFailed}, func() { clicks++ }, "a")
			TaskRow(c, TaskRowView{Title: "任务乙", State: TaskQueued}, nil, "b")
			ConnectionRow(c, ConnectionRowView{Name: "供应商甲", State: ConnectionError}, func() { clicks += 10 }, "c")
			ConnectionRow(c, ConnectionRowView{Name: "供应商乙", State: ConnectionDisconnected}, nil, "d")
		})
	}), 420, 320)
	for _, s := range []string{"失败", "排队中", "出错", "未连接"} {
		if !tester.HasText(s) {
			t.Fatalf("badge %q missing", s)
		}
	}
	_ = tester.Click("任务甲")
	_ = tester.Click("供应商甲")
	tester.Key(0, ui.KeyTab)
	tester.Key(0, ui.KeyTab)
	tester.Key(0, ui.KeyTab)
	if clicks != 11 || tester.Focused("任务乙") || tester.Focused("供应商乙") {
		t.Fatal("rows without a callback are controls, or clicks lost", clicks)
	}
}

func TestModelRowPickerAndManage(t *testing.T) {
	picks, favs, edits := 0, 0, 0
	a := ModelRowActions{OnSelect: func() { picks++ }, OnToggleFavorite: func() { favs++ }, OnEdit: func() { edits++ }}
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		ui.Column(c).Width(sampleRow).Children(func() {
			ModelRow(c, ModelRowView{Name: "可选模型", Selected: true}, ModelPicker, a, "p")
			ModelRow(c, ModelRowView{Name: "停用模型", Unavailable: true}, ModelPicker, a, "u")
			ModelRow(c, ModelRowView{Name: "管理模型", Favorite: true}, ModelManage, a, "m")
		})
	}), 420, 240)
	_ = tester.Click("可选模型")
	_ = tester.Click("停用模型")
	if picks != 1 {
		t.Fatal("picker click gating wrong", picks)
	}
	if !tester.HasText("不可用") {
		t.Fatal("unavailable badge missing")
	}
	_ = tester.Click("管理模型")
	if picks != 1 {
		t.Fatal("a manage row must not select")
	}
	if err := tester.Click("收藏 管理模型"); err != nil || favs != 1 {
		t.Fatal("favorite action missing or not named", err, favs)
	}
	if err := tester.Click("编辑 管理模型"); err != nil || edits != 1 {
		t.Fatal("edit action missing or not named", err, edits)
	}
}

func TestMessageHeaderAuthors(t *testing.T) {
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		ui.Column(c).Gap(theme.Space2).Children(func() {
			MessageHeader(c, MessageHeaderView{Author: AuthorUser, Name: "我", Meta: "10:30"}, "u")
			MessageHeader(c, MessageHeaderView{Author: AuthorPartner, Name: "研究伙伴"}, "p")
		})
	}), 320, 200)
	if !tester.HasText("10:30") || !tester.HasText("伙伴") {
		t.Fatal("meta or partner badge missing")
	}
}

func TestCitationKeyboardFocusAndUnavailable(t *testing.T) {
	opened := 0
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		ui.Row(c).Gap(theme.Space2).Children(func() {
			Citation(c, CitationView{Index: 1, Title: "来源甲", Source: "docs"}, func() { opened++ }, "a")
			Citation(c, CitationView{Index: 2, Title: "来源乙", Unavailable: true}, func() { opened += 10 }, "b")
		})
	}), 400, 120)
	tester.Key(0, ui.KeyTab)
	tester.Key(0, ui.KeyEnter)
	_ = tester.Click("来源乙")
	if opened != 1 || tester.Focused("来源乙") {
		t.Fatal("citation activation or unavailable gating wrong", opened)
	}
	r, _ := tester.Find("来源甲")
	if r.H != theme.Space6 {
		t.Fatal("citation chip height", r.H)
	}
}

func TestAttachmentStatesAndRemove(t *testing.T) {
	removed := 0
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		ui.Column(c).Gap(theme.Space2).Children(func() {
			Attachment(c, AttachmentView{Name: "甲.pdf", Detail: "上传中", State: AttachmentUploading}, func() { removed++ }, "a")
			Attachment(c, AttachmentView{Name: "乙.pdf", Detail: "上传失败", State: AttachmentError}, nil, "b")
		})
	}), 360, 200)
	if !tester.HasText("Loading indicator") || !tester.HasText("上传失败") {
		t.Fatal("uploading or error state missing")
	}
	if err := tester.Click("移除 甲.pdf"); err != nil || removed != 1 {
		t.Fatal("remove action not named or not wired", err, removed)
	}
	if tester.HasText("移除 乙.pdf") {
		t.Fatal("a nil onRemove must hide the control")
	}
	errChip, _ := tester.Find("乙.pdf")
	if px := kittest.PixelAt(tester, 360, errChip.X+errChip.W/2, errChip.Y+.5); !kittest.Within(px, theme.UI(theme.Light.Danger), 12) {
		t.Fatal("error attachment lost its danger border", px)
	}
}

func TestCodeBlockCopyStatesAndCallback(t *testing.T) {
	copies := 0
	for state, label := range map[CopyState]string{CopyDefault: "复制", CopyCopied: "已复制", CopyFailed: "复制失败"} {
		tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
			CodeBlock(c, CodeBlockView{Language: "go", Code: "fmt.Println(1)", Copy: state}, func() { copies++ }, "c").Width(sampleRow)
		}), 420, 200)
		if !tester.HasText(label) || !tester.HasText("fmt.Println(1)") {
			t.Fatalf("state %d missing %q", state, label)
		}
		_ = tester.Click(label)
	}
	if copies != 3 {
		t.Fatal("copy callback not wired in every state", copies)
	}
}

func TestForcedFocusOnCitationDrawsTheStroke(t *testing.T) {
	tester := ui.NewTester(kittest.Centered(func(c *ui.Context) {
		force.With(force.Focus, func() { Citation(c, CitationView{Index: 1, Title: "来源"}, func() {}, "a") })
	}), 240, 100)
	r, _ := tester.Find("来源")
	if px := kittest.PixelAt(tester, 240, r.X+r.W/2, r.Y+.5); !kittest.Within(px, theme.UI(theme.Light.Accent), 14) {
		t.Fatal("no focus stroke", px)
	}
}
