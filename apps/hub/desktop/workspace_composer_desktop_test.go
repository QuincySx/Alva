// INPUT: Real Hub service fixtures, MyGo headless input/layout and actual composer state
// OUTPUT: Explicit bounded drafts, truthful policy/target overlays and native toolbar geometry regressions
// POS: [L7 Hub UI] No OS window, external engine, model request or invented permission grant
//go:build desktop

package hubdesktop

import (
	"context"
	"fmt"
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
	if !tester.HasText("每次询问") || !tester.HasText("自动审批（Auto） · 不可用") || !tester.HasText("完全访问（Full） · 不可用") {
		t.Fatal("permission popover omitted the current policy or truthful unavailable choices")
	}
	if err := tester.Click("自动审批（Auto）"); err != nil {
		t.Fatal(err)
	}
	if policy, _ := workspacePermissionPolicy(p.workspaceComposerTarget().EngineID); policy != "每次询问" {
		t.Fatal("unavailable permission choice changed the host policy")
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
	if !tester.HasText("文件引用暂仅支持 Alva 引擎。") || !tester.HasText("计划模式") || !tester.HasText("命令与 Skills…") || tester.HasText("协同模式") {
		t.Fatal("attachment menu concealed prerequisites or actual supported actions")
	}
	if err := tester.Click("Plan mode"); err != nil {
		t.Fatal(err)
	}
	if p.workspacePlanEnabled() {
		t.Fatal("external engine accepted native Plan intent")
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
		if !found || box.X < composer.X+7 || box.X+box.W > composer.X+composer.W-7 {
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
		if (label == "Send" && (box.W != 32 || box.H != 32)) || (label == "Add attachments" && (box.W != 32 || box.H != 32)) {
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

func TestWorkspaceComposerTargetKeyboardScrollsConfiguredRoutes(t *testing.T) {
	service, requests := fixture(t)
	for i := range 20 {
		target := service.DefaultTarget
		target.ModelID = fmt.Sprintf("configured-%02d", i)
		service.Targets = append(service.Targets, target)
	}
	p := NewNativePage(service)
	p.model.Draft = "保留草稿🙂"
	tester := composerTester(p, 540)
	tester.SetSize(540, 300)
	if err := tester.Click("Agent and model"); err != nil {
		t.Fatal(err)
	}
	defaultLabel := "Use " + service.DefaultTarget.EngineID + " · " + service.DefaultTarget.ModelID
	if !tester.Focused(defaultLabel) {
		t.Fatal("opening the target menu did not focus its actual selected route")
	}
	tester.Key(0, ui.KeyEnd)
	last := service.Targets[len(service.Targets)-1]
	lastLabel := "Use " + last.EngineID + " · " + last.ModelID
	if !tester.Focused(lastLabel) {
		t.Fatal("End did not focus the last configured route")
	}
	box, ok := tester.Find(lastLabel)
	if !ok || box.Y < 0 || box.Y+box.H > 300 {
		menu, _ := tester.Find("Composer menu")
		scroll, _ := tester.Find("Composer menu contents")
		group, _ := tester.Find("Configured agents and models")
		first, _ := tester.Find(defaultLabel)
		t.Fatalf("active route is outside the short viewport: %v (menu %v scroll %v group %v first %v)", box, menu, scroll, group, first)
	}
	tester.Key(0, ui.KeyUp)
	beforeLast := service.Targets[len(service.Targets)-2]
	if !tester.Focused("Use " + beforeLast.EngineID + " · " + beforeLast.ModelID) {
		t.Fatal("Up did not move among configured routes")
	}
	tester.Key(0, ui.KeyDown)
	tester.Key(0, ui.KeyEnter)
	if p.composer.targetOpen || hub.RouteKey(p.workspaceComposerTarget()) != hub.RouteKey(last) || !tester.Focused("Agent and model") {
		t.Fatal("Enter did not choose the active route and restore its trigger focus")
	}
	if p.model.Draft != "保留草稿🙂" || len(requests) != 0 || service.Runtime.HasActive() {
		t.Fatal("route keyboard gestures changed the draft or executed a request")
	}
	tester.Key(0, ui.KeyEnter)
	if !tester.Focused(lastLabel) {
		t.Fatal("reopening did not focus the newly selected actual route")
	}
	tester.Key(0, ui.KeyEscape)
	if p.composer.targetOpen || !tester.Focused("Agent and model") {
		t.Fatal("Escape did not close the route menu and restore its trigger")
	}
}

func TestWorkspaceComposerSubmissionAllowsEditingAndTruthfulTerminalStatus(t *testing.T) {
	service, requests := fixture(t)
	p := NewNativePage(service)
	p.model.Draft = "already captured"
	tester := composerTester(p, 760)
	p.busy = true
	p.chat.Busy = true
	tester.Frame()
	if err := tester.Click("Message"); err != nil {
		t.Fatal(err)
	}
	tester.Type(" plus new draft")
	if !strings.Contains(p.model.Draft, "plus new draft") {
		t.Fatal("submission/import froze the independent editable text draft")
	}
	if err := tester.Click("Send"); err != nil {
		t.Fatal(err)
	}
	if p.selected != "" || len(requests) != 0 || service.Runtime.HasActive() {
		t.Fatal("editing during admission enabled duplicate Send")
	}
	p.busy, p.chat.Busy = false, false
	p.selected, p.detail.Session.ID = "observed", "observed"
	for _, tc := range []struct {
		outcome engine.Outcome
		want    string
	}{{engine.Succeeded, "已完成"}, {engine.Cancelled, "已停止，已保留部分输出"}, {engine.Failed, "执行失败"}, {engine.Interrupted, "执行失败"}} {
		p.detail.Session.Status = tc.outcome
		tester.Frame()
		if !tester.HasText(tc.want) {
			t.Errorf("observed %s lacks terminal RunStatus %q", tc.outcome, tc.want)
		}
	}
}

func TestWorkspaceComposerContextChangeClosesMenuAndRetainsDraft(t *testing.T) {
	service, _ := fixture(t)
	p := NewNativePage(service)
	p.model.Draft = "draft across context change"
	tester := composerTester(p, 760)
	if err := tester.Click("Agent and model"); err != nil {
		t.Fatal(err)
	}
	p.selected = "new-context"
	p.detail = hub.Detail{Session: hub.Session{ID: p.selected, ActiveBranchID: "branch"}, Branches: []hub.Branch{{ID: "branch", Target: service.DefaultTarget}}}
	tester.Frame()
	if p.composer.targetOpen || p.model.Draft != "draft across context change" {
		t.Fatal("context change retained a stale menu or lost its draft")
	}
	if err := tester.Click("Permission policy"); err != nil {
		t.Fatal(err)
	}
	if err := tester.Click("Message"); err != nil {
		t.Fatal(err)
	}
	if p.composer.permissionOpen || !tester.Focused("Message") {
		t.Fatal("outside click did not dismiss only the menu and focus the actual clicked editor")
	}
}

func TestWorkspaceComposerTooltipsExplainDisabledActionsAndActualPolicy(t *testing.T) {
	service, _ := fixture(t)
	p := NewNativePage(service)
	tester := composerTester(p, 760)
	send, ok := tester.Find("Send unavailable")
	if !ok {
		t.Fatal("empty Send has no focusable explanation")
	}
	tester.Move(send.X+send.W/2, send.Y+send.H/2)
	tester.Advance(time.Second)
	if !tester.HasText("请输入消息后发送；Enter 换行。") {
		t.Fatal("disabled Send does not explain its actual empty-draft reason on hover")
	}
	if err := tester.Click("Message"); err != nil {
		t.Fatal(err)
	}
	for i := 0; i < 8 && !tester.Focused("Send unavailable"); i++ {
		tester.Key(0, ui.KeyTab)
	}
	if !tester.Focused("Send unavailable") || !tester.HasText("请输入消息后发送；Enter 换行。") {
		t.Fatal("keyboard users cannot focus the disabled Send explanation")
	}
	p.busy = true
	p.model.Draft = "new draft"
	tester.Frame()
	if !tester.HasText("正在提交或导入附件，等待真实接收回执。") {
		t.Fatal("pending submission reuses an inaccurate generic Send tooltip")
	}
	// The fixed host policy remains descriptive on focus; focus never grants it.
	tester.Key(ui.Shift, ui.KeyTab)
	_, description := workspacePermissionPolicy(service.DefaultTarget.EngineID)
	if !tester.Focused("Permission policy") || !tester.HasText(description) || p.composer.permissionOpen {
		t.Fatal("permission focus does not explain the actual policy independently of its menu")
	}
	p.busy = false
	p.model.ActiveRunID, p.model.Status = "observed", view.Stopping
	tester.Frame()
	stop, ok := tester.Find("Stop unavailable")
	if !ok {
		t.Fatal("Stopping lacks a disabled Stop explanation")
	}
	tester.Advance(time.Millisecond)
	tester.Move(stop.X+stop.W/2, stop.Y+stop.H/2)
	tester.Advance(time.Second)
	if !tester.HasText("正在停止，等待执行进程和回调清理完成。") || p.model.Status != view.Stopping {
		t.Fatalf("Stop tooltip falsely acknowledged execution cleanup: status=%s texts=%q", p.model.Status, tester.Texts())
	}
}
