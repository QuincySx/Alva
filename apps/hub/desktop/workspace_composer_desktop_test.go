// INPUT: Real Hub service fixtures, MyGo headless input/layout and actual composer state
// OUTPUT: Explicit bounded drafts, truthful policy/target overlays and native toolbar geometry regressions
// POS: [L7 Hub UI] No OS window, external engine, model request or invented permission grant
//go:build desktop

package hubdesktop

import (
	"context"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	view "github.com/QuincySx/Alva/apps/ui/chat"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/egoist/mygo/ui"
)

func composerTester(p *NativePage, width int) *ui.Tester {
	return ui.NewTester(func(c *ui.Context) {
		p.applyPreferences(c)
		ui.Column(c).FillWidth().Padding(16).Children(func() { p.buildWorkspaceComposer(c) })
	}, width, 520)
}

func TestWorkspaceComposerRequiresExplicitBoundedInput(t *testing.T) {
	service, requests := fixture(t)
	p := NewNativePage(service)
	tester := composerTester(p, 760)
	if err := tester.Click("Send"); err != nil {
		t.Fatal(err)
	}
	if p.busy || p.selected != "" {
		t.Fatal("empty input started admission")
	}
	if err := tester.Click("Message"); err != nil {
		t.Fatal(err)
	}
	tester.Compose("中文", 2)
	if _, active := tester.TextCaret(); !active {
		t.Fatal("native multiline input lost its IME caret")
	}
	tester.Type("中文草稿")
	tester.Compose("", 0)
	tester.Key(0, ui.KeyEnter)
	tester.Type("第二行🙂")
	want := "中文草稿\n第二行🙂"
	if p.model.Draft != want || service.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("typing or Enter executed a task or lost multiline Unicode input")
	}
	tester.SetClipboard(strings.Repeat("x", view.MaxDraftBytes+1))
	tester.Command("paste")
	if p.model.Draft != want || p.problem == "" {
		t.Fatal("oversized paste changed the draft without an actionable error")
	}
	p.model.Draft = strings.Repeat("x", view.MaxDraftBytes+1)
	tester.Frame()
	if !tester.HasText("消息超过 32 KiB，请缩短内容或添加文件。") {
		t.Fatal("oversized draft did not expose the submission limit")
	}
	if err := tester.Click("Send"); err != nil {
		t.Fatal(err)
	}
	sessions, err := service.Store.ListSessions(context.Background(), 8)
	if err != nil || len(sessions) != 0 || service.Runtime.HasActive() {
		t.Fatal("invalid input created durable facts or executed")
	}
}

func TestWorkspaceComposerMenusUseActualRoutesAndPolicy(t *testing.T) {
	service, _ := fixture(t)
	other := service.DefaultTarget
	other.EngineID, other.ModelID = "configured-engine", "configured-model"
	service.Targets = append(service.Targets, other)
	p := NewNativePage(service)
	p.model.Draft = "保留我的草稿"
	tester := composerTester(p, 760)
	if err := tester.Click("Permission policy"); err != nil {
		t.Fatal(err)
	}
	if !tester.HasText("每次询问") || tester.HasText("自动审批") || tester.HasText("完全访问") {
		t.Fatal("permission popover invented unsupported permission modes")
	}
	tester.Key(0, ui.KeyEscape)
	if p.composer.permissionOpen || p.model.Draft != "保留我的草稿" || !tester.Focused("Permission policy") {
		t.Fatal("dismissing policy popover lost the draft or trigger focus")
	}
	if err := tester.Click("Agent and model"); err != nil {
		t.Fatal(err)
	}
	if err := tester.Click("Use configured-engine · configured-model"); err != nil {
		t.Fatal(err)
	}
	if p.composer.targetOpen || hub.RouteKey(p.workspaceComposerTarget()) != hub.RouteKey(other) || p.model.Draft != "保留我的草稿" {
		t.Fatal("actual configured home route selection did not preserve the draft")
	}
	if err := tester.Click("Add attachments"); err != nil {
		t.Fatal(err)
	}
	if !tester.HasText("发送首条消息创建会话后，即可添加文件。") || tester.HasText("计划模式") || tester.HasText("协同模式") {
		t.Fatal("attachment menu concealed its real prerequisite or invented unsupported actions")
	}
	if err := tester.Click("Attach file"); err != nil {
		t.Fatal(err)
	}
	sessions, err := service.Store.ListSessions(context.Background(), 8)
	if err != nil || len(sessions) != 0 || p.busy || service.Runtime.HasActive() {
		t.Fatal("opening a menu or choosing a route created or executed a session")
	}
	// Once a session is selected, its actual active branch wins over a home draft route.
	p.selected = "actual-session"
	p.detail = hub.Detail{Session: hub.Session{ID: p.selected, ActiveBranchID: "actual-branch"}, Branches: []hub.Branch{{ID: "actual-branch", Target: service.DefaultTarget}}}
	if hub.RouteKey(p.workspaceComposerTarget()) != hub.RouteKey(service.DefaultTarget) {
		t.Fatal("home route state overrode the selected session's persisted branch")
	}
}

func TestWorkspaceComposerToolbarAlignsAndRunningDraftStaysIndependent(t *testing.T) {
	service, requests := fixture(t)
	p := NewNativePage(service)
	tester := composerTester(p, 540)
	composer, ok := tester.Find("Task composer")
	if !ok {
		t.Fatal("composer lacks a semantic container")
	}
	var center float32
	for i, label := range []string{"Add attachments", "Permission policy", "Agent and model", "Send"} {
		box, found := tester.Find(label)
		if !found || box.X < composer.X+11 || box.X+box.W > composer.X+composer.W-11 {
			t.Fatalf("%s overflows the padded composer: %v within %v", label, box, composer)
		}
		if box.W < 32 || box.H < 32 {
			t.Fatalf("%s lost its minimum click area: %v", label, box)
		}
		if i == 0 {
			center = box.Y + box.H/2
		} else if delta := box.Y + box.H/2 - center; delta < -0.5 || delta > 0.5 {
			t.Fatalf("%s does not share the toolbar center", label)
		}
		if (label == "Send" && (box.W != 36 || box.H != 36)) || (label == "Add attachments" && (box.W != 32 || box.H != 32)) {
			t.Fatalf("%s lost its symmetric icon slot: %v", label, box)
		}
	}
	p.selected = "observed-session"
	p.detail.Session.ID = p.selected
	p.model.ActiveRunID, p.model.Status = "observed-session", view.Running
	p.detail.Session.Status = hub.Running
	tester.Frame()
	if _, found := tester.Find("Send"); found {
		t.Fatal("running composer retained a submission action")
	}
	if !tester.HasText("正在工作…") {
		t.Fatal("actual running status is absent")
	}
	if err := tester.Click("Message"); err != nil {
		t.Fatal(err)
	}
	tester.Type("下一轮草稿")
	if p.model.Draft != "下一轮草稿" || len(requests) != 0 || service.Runtime.HasActive() {
		t.Fatal("running draft editing submitted a second task")
	}
	p.model.Status = view.Stopping
	tester.Frame()
	if !tester.HasText("正在停止…") {
		t.Fatal("stopping did not preserve ownership acknowledgement")
	}
	if err := tester.Click("Stop"); err != nil {
		t.Fatal(err)
	}
	if p.model.Status != view.Stopping || p.model.ActiveRunID == "" {
		t.Fatal("disabled Stop falsely acknowledged completion")
	}
	p.model.ActiveRunID, p.model.Status = "", view.Idle
	p.detail.Session.Status = engine.Waiting
	tester.Frame()
	tester.Advance(time.Second)
	if !tester.HasText("等待你的回应") || p.model.Draft != "下一轮草稿" {
		t.Fatal("waiting did not freeze and preserve the draft")
	}
}
