// INPUT: Native MyGo controls, durable Hub fixtures and bounded local execution/playback owners.
// OUTPUT: Shared Settings geometry, origin/draft preservation and media cleanup navigation regressions.
// POS: [L7 Hub UI testing] Exercises real navigation without an OS window, external model or decoder.
//go:build desktop

package hubdesktop

import (
	"bytes"
	"context"
	"fmt"
	"image"
	"reflect"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/egoist/mygo/ui"
)

func TestWorkspaceSettingsScopedCategorySelectsExistingOwnerAndUsesRealPolicy(t *testing.T) {
	service := preferenceFixture(t)
	owner, err := service.Create(t.Context(), "Existing settings owner", "")
	if err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(service)
	p.model.Draft = "home draft remains local"
	p.acceptWorkspaceHomePermission(hub.PermissionFull)
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	openPreferencePage(t, p, tester)
	connectionClick(t, tester, "Permissions and security")
	if p.pane != "settings-security" || !tester.HasText("尚未选择任务") {
		t.Fatal("session-scoped security pretended to have an owner")
	}
	connectionClick(t, tester, "Choose settings session")
	settleWorkspaceNavigation(t, p, tester)
	if len(p.settingsScope.page.Sessions) != 1 || p.selected != "" {
		t.Fatal("opening chooser changed scope before explicit selection")
	}
	connectionClick(t, tester, "Select scope session "+owner.ID)
	settleWorkspaceNavigation(t, p, tester)
	if p.pane != "settings-scope" || p.detail.Session.ID != owner.ID || p.drafts[""] != "home draft remains local" {
		t.Fatal("scope selection lost its draft or skipped the explicit open step")
	}
	p.loadWorkspaceScopeSessions(hub.SessionCursor{})
	if p.workspaceScopeReady() || p.selected != owner.ID {
		t.Fatal("refresh kept Open bound to an old row or silently switched the workspace")
	}
	settleWorkspaceNavigation(t, p, tester)
	connectionClick(t, tester, "Select scope session "+owner.ID)
	settleWorkspaceNavigation(t, p, tester)
	connectionClick(t, tester, "Open selected scope")
	if p.pane != "settings-security" {
		t.Fatal("explicit scope open did not return to its category")
	}
	connectionClick(t, tester, "Settings permission policy")
	connectionClick(t, tester, "自动审批")
	settleWorkspaceNavigation(t, p, tester)
	settings, err := service.Store.Settings(t.Context(), owner.ID)
	if err != nil || settings.PermissionPolicy != hub.PermissionAuto || p.permissionPolicies[""] != hub.PermissionFull || service.Runtime.HasActive() {
		t.Fatal("Settings policy did not use the selected real owner or changed home policy", settings, err)
	}
	rows, err := service.Store.ListSessions(t.Context(), 10)
	if err != nil || len(rows) != 1 {
		t.Fatal("scope chooser created another session", rows, err)
	}
}

func TestWorkspaceHomePanelChooserRequiresExplicitExistingScope(t *testing.T) {
	source, owner := resourcePageFixture(t)
	p := NewNativePage(source.Service)
	p.model.Draft = "unsubmitted home draft"
	tester := ui.NewTester(workspaceStaticView(p), 1440, 960)
	connectionClick(t, tester, "Open resource workspace")
	settleWorkspaceNavigation(t, p, tester)
	if p.pane != "" || p.selected != "" || !p.workspaceResourcesVisible() || !tester.HasText("选择任务后查看资料") {
		t.Fatal("home opener navigated or selected a task instead of revealing an empty pane")
	}
	connectionClick(t, tester, "选择任务")
	settleWorkspaceNavigation(t, p, tester)
	if p.pane != "workspace-scope" || p.selected != "" || p.workspaceResourcesVisible() {
		t.Fatal("explicit task chooser selected a session implicitly")
	}
	connectionClick(t, tester, "Select scope session "+owner.ID)
	settleWorkspaceNavigation(t, p, tester)
	if p.selected != owner.ID || !p.workspaceScopeReady() || p.workspaceResourcesVisible() {
		t.Fatal("selection did not wait for actual detail or opened the resource without confirmation")
	}
	connectionClick(t, tester, "Open selected scope")
	settleResource(t, p, tester, func() bool { return p.workspaceResourcesVisible() })
	if p.pane != "" || p.drafts[""] != "unsubmitted home draft" || p.Service.Runtime.HasActive() {
		t.Fatal("opening existing resources changed the home draft or execution")
	}
	rows, err := p.Service.Store.ListSessions(t.Context(), 10)
	if err != nil || len(rows) != 1 {
		t.Fatal("Panel chooser created an empty session", rows, err)
	}
}

func switchSettingsCategory(t *testing.T, tester *ui.Tester) {
	t.Helper()
	// A build without the separately developed Developer feature has only
	// General. Every category actually present must retain the same origin.
	if _, ok := tester.Find("Developer options"); ok {
		if err := tester.Click("Developer options"); err != nil {
			t.Fatal(err)
		}
		if err := tester.Click("General settings"); err != nil {
			t.Fatal(err)
		}
	}
}

func TestWorkspaceSettingsReturnKeepsOriginalPaneAndDrafts(t *testing.T) {
	for _, pane := range []string{"home", "chat", "inspector"} {
		t.Run(pane, func(t *testing.T) {
			service := preferenceFixture(t)
			page := NewNativePage(service)
			if pane != "home" {
				session, err := service.Create(t.Context(), "Original task", "")
				if err != nil {
					t.Fatal(err)
				}
				page.selected = session.ID
				page.detail, err = service.Store.Detail(t.Context(), session.ID)
				if err != nil {
					t.Fatal(err)
				}
				page.applyDetail()
			}
			if pane == "inspector" {
				page.pane = "inspector"
			}
			page.model.Draft = "继续编辑的任务草稿"
			page.inspectQuery, page.inspectLoadedQuery, page.inspectGroup = "draft search", "saved search", "Tools"
			page.inspectBranch, page.inspectURI, page.inspectCursor = "retained-branch", "context://record", 39
			page.inspectPage = product.InspectorPage{NativeID: "retained-child", HighWater: 100, Next: 45, HasMore: true}
			page.inspectPath = []product.ChildHop{{LinkSequence: 17}}
			page.inspectAfter = []uint64{0, 19}
			page.inspectFrames = []InspectorFrame{{Cursor: 19, Query: "parent search"}}
			wantPane, wantID, wantPage := page.pane, page.selected, page.inspectPage
			wantPath, wantAfter, wantFrames := page.inspectPath, page.inspectAfter, page.inspectFrames
			tester, _ := preferenceTester(page)
			openPreferencePage(t, page, tester)
			base := page.preferences.base
			choosePreference(t, tester, "Color theme", "dark")
			switchSettingsCategory(t, tester)
			if err := tester.Click("Back to workspace"); err != nil {
				t.Fatal(err)
			}
			if page.pane != wantPane || page.selected != wantID || page.model.Draft != "继续编辑的任务草稿" || page.preferences.theme != "dark" || page.preferences.base != base {
				t.Fatal("Settings return changed its origin or discarded an unsaved draft", page.pane, page.selected, page.preferences)
			}
			if !reflect.DeepEqual(page.inspectPage, wantPage) || !reflect.DeepEqual(page.inspectPath, wantPath) || !reflect.DeepEqual(page.inspectAfter, wantAfter) || !reflect.DeepEqual(page.inspectFrames, wantFrames) || page.inspectCursor != 39 || page.inspectGroup != "Tools" || page.inspectURI != "context://record" || page.inspectQuery != "draft search" || page.inspectLoadedQuery != "saved search" {
				t.Fatal("return reconstructed Inspector instead of retaining its existing view")
			}
			if service.Preferences.Snapshot() != base || service.Runtime.HasActive() {
				t.Fatal("navigation saved preferences or started execution")
			}
			rows, err := service.Store.ListSessions(t.Context(), 10)
			wantRows := 1
			if pane == "home" {
				wantRows = 0
			}
			if err != nil || len(rows) != wantRows {
				t.Fatal("Settings navigation created another task", rows, err)
			}
		})
	}
}

func TestWorkspaceSettingsBackStaysInMainHeader(t *testing.T) {
	service := preferenceFixture(t)
	for _, size := range []struct{ width, height int }{{1240, 850}, {1000, 720}} {
		page := NewNativePage(service)
		tester := ui.NewTester(workspaceStaticView(page), size.width, size.height)
		openPreferencePage(t, page, tester)
		check := func() {
			t.Helper()
			back, ok := tester.Find("Back to workspace")
			nav, navOK := tester.Find("Settings navigation")
			title, titleOK := tester.Find("设置")
			header, headerOK := tester.Find("Settings header")
			if !ok || !navOK || !titleOK || !headerOK || nav.X != 8 || nav.Y != 52 || nav.W != workspaceSettingsNavWidth ||
				header.X != 224 || header.Y != 8 || header.H != 36 || back.X != header.X+8 || back.Y != header.Y+2 || back.H != 32 ||
				title.X != 16 || title.Y != 15.5 || title.H != 21 || back.X < nav.X+nav.W+8 {
				t.Fatal("shared Back is not in the main header beside the compact settings navigation", back, nav, title, header)
			}
			if !tester.HasText("返回工作台") || tester.HasText("Back to conversation") || tester.HasText("权限设置") {
				t.Fatal("Settings omitted its shared return", tester.Texts())
			}
			for _, category := range []string{"通用", "模型与连接", "插件与工具", "规则与 Skills", "数据与存储", "权限与安全", "Gateway", "开发者"} {
				if !tester.HasText(category) {
					t.Fatal("formal Settings category missing", category)
				}
			}
		}
		check()
		switchSettingsCategory(t, tester)
		check()
	}
}

type settingsPlayback struct {
	entered, release chan struct{}
	complete         atomic.Bool
}

func (p *settingsPlayback) Close(ctx context.Context) error {
	close(p.entered)
	select {
	case <-p.release:
		p.complete.Store(true)
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}
func (p *settingsPlayback) CleanupComplete() bool { return p.complete.Load() }

func TestWorkspaceSettingsMediaReturnRestoresIndexWithoutBodyOrPlayback(t *testing.T) {
	service := preferenceFixture(t)
	session, err := service.Create(t.Context(), "Media task", "")
	if err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"first.wav", "second.wav"} {
		if _, err = service.UploadReader(t.Context(), session.ID, name, bytes.NewReader(canonicalTestWAV())); err != nil {
			t.Fatal(err)
		}
	}
	page := NewNativePage(service)
	page.selected = session.ID
	page.detail, err = service.Store.Detail(t.Context(), session.ID)
	if err != nil {
		t.Fatal(err)
	}
	page.applyDetail()
	tester, _ := preferenceTester(page)
	page.openMedia()
	settlePage(t, tester, page)
	if len(page.media.Page.Entries) != 2 {
		t.Fatal("fixture did not load the original metadata index", page.problem)
	}
	page.loadMedia(page.media.Page.Entries[0].Sequence, page.media.Page.HighWater)
	settlePage(t, tester, page)
	if len(page.media.Page.Entries) != 1 {
		t.Fatal("fixture did not retain a later metadata page", page.problem)
	}
	page.media.Back = []uint64{0}
	branch, cursor, highWater, entryID := page.media.Branch, page.media.Cursor, page.media.Page.HighWater, page.media.Page.Entries[0].Info.ID
	page.media.JSON, page.media.PreviewBytes = "private media body", 123
	page.media.Bitmap = ui.NewBitmap(image.NewRGBA(image.Rect(0, 0, 1, 1)))
	player := &settingsPlayback{entered: make(chan struct{}), release: make(chan struct{})}
	page.player, page.playerBusy = player, true
	var release sync.Once
	defer release.Do(func() { close(player.release) })
	openPreferencePage(t, page, tester)
	select {
	case <-player.entered:
	case <-time.After(time.Second):
		t.Fatal("leaving media did not ask its playback owner to join")
	}
	if page.media.Bitmap != nil || page.media.JSON != "" || page.media.PreviewBytes != 0 || page.player != player || !page.playerBusy || !page.playerStopping || player.CleanupComplete() {
		t.Fatal("hidden media body survived or playback cleanup was acknowledged early")
	}
	switchSettingsCategory(t, tester)
	if err = tester.Click("Back to workspace"); err != nil {
		t.Fatal(err)
	}
	settlePage(t, tester, page)
	if page.pane != "media" || page.media.Branch != branch || page.media.Mode != "sources" || page.media.Cursor != cursor || page.media.Page.HighWater != highWater || len(page.media.Page.Entries) != 1 || page.media.Page.Entries[0].Info.ID != entryID || !reflect.DeepEqual(page.media.Back, []uint64{0}) || page.media.Bitmap != nil || page.media.JSON != "" || page.media.PlaybackURI != "" {
		t.Fatal("return lost the metadata location or restored a hidden media body", page.media, page.problem)
	}
	release.Do(func() { close(player.release) })
	awaitPlayback(t, tester, page, func() bool { return !page.playerBusy })
	if !player.CleanupComplete() || page.player != nil || service.Runtime.HasActive() {
		t.Fatal("Settings return leaked playback or started execution")
	}
}

type settingsRunningEngine struct {
	entered, release chan struct{}
	starts, cancels  atomic.Int32
}

func (r *settingsRunningEngine) Execute(ctx context.Context, _ engine.Request, emit func(engine.Event) error) error {
	r.starts.Add(1)
	if err := emit(engine.Event{Kind: engine.SessionStarted, SessionID: "settings-local-run"}); err != nil {
		return err
	}
	close(r.entered)
	outcome := engine.Succeeded
	select {
	case <-r.release:
	case <-ctx.Done():
		outcome = engine.Cancelled
	}
	return emit(engine.Event{Kind: engine.Completed, Outcome: outcome})
}
func (r *settingsRunningEngine) Cancel(context.Context, string) error { r.cancels.Add(1); return nil }
func (*settingsRunningEngine) RespondPermission(context.Context, string, string, engine.PermissionDecision) error {
	return engine.ErrUnsupported
}
func (*settingsRunningEngine) Capabilities() engine.Capabilities {
	return engine.Capabilities{Cancel: true}
}

func TestWorkspaceSettingsNavigationKeepsBackgroundExecution(t *testing.T) {
	service := preferenceFixture(t)
	runtime := &settingsRunningEngine{entered: make(chan struct{}), release: make(chan struct{})}
	if err := service.Runtime.Register("alva", runtime); err != nil {
		t.Fatal(err)
	}
	session, err := service.Create(t.Context(), "Running task", "")
	if err != nil {
		t.Fatal(err)
	}
	_, done, err := service.Runtime.Start(t.Context(), session.ID, "explicit local fixture")
	if err != nil {
		t.Fatal(err)
	}
	defer func() { close(runtime.release); <-done }()
	select {
	case <-runtime.entered:
	case <-time.After(time.Second):
		t.Fatal("local run did not start")
	}
	page := NewNativePage(service)
	page.selected = session.ID
	page.detail, err = service.Store.Detail(t.Context(), session.ID)
	if err != nil {
		t.Fatal(err)
	}
	page.applyDetail()
	page.model.Draft = "next prompt"
	tester, _ := preferenceTester(page)
	openPreferencePage(t, page, tester)
	switchSettingsCategory(t, tester)
	if err = tester.Click("Back to workspace"); err != nil {
		t.Fatal(err)
	}
	if !service.Runtime.HasActive() || runtime.starts.Load() != 1 || runtime.cancels.Load() != 0 || page.pane != "" || page.selected != session.ID || page.model.Draft != "next prompt" {
		t.Fatal("Settings return stopped/restarted the admitted execution or lost the conversation")
	}
}

func TestWorkspaceSettingsPendingConnectionFormSharesRemainingHeight(t *testing.T) {
	for _, size := range []struct {
		width, height int
		bar           ui.TitleBar
	}{
		{1240, 850, ui.TitleBar{}}, {960, 680, ui.TitleBar{}}, {960, 680, ui.TitleBar{Height: 52, Left: 86}}, {960, 680, ui.TitleBar{Height: 52, Right: 138}},
	} {
		for _, source := range []string{"Custom", "Local", "OpenAI"} {
			t.Run(fmt.Sprintf("%dx%d/%s/bar=%v", size.width, size.height, source, size.bar), func(t *testing.T) {
				p, pending := workspaceInteractionPage(t)
				store := &connectionFaultStore{ConnectionsStore: p.Service.Store.(hub.ConnectionsStore)}
				var err error
				p.Service.Connections, err = product.NewConnectionsService(t.Context(), store)
				if err != nil {
					t.Fatal(err)
				}
				t.Cleanup(func() { _ = p.Service.Connections.Close(context.Background()) })
				p.openConnections()
				tester := ui.NewTester(workspaceStaticView(p), size.width, size.height)
				tester.SetTitleBar(size.bar)
				settleConnections(t, p, tester)
				p.chooseConnectionSource(source)
				for i := range 16 {
					p.connections.draft.Models = append(p.connections.draft.Models, hub.ConnectionModel{ID: fmt.Sprintf("pending-model-%02d", i)})
				}
				tester.Frame()
				if source == "OpenAI" {
					connectionClick(t, tester, "Connection details")
				}
				viewport := func(label string) ui.Rect {
					t.Helper()
					r, ok := tester.Find(label)
					if !ok || r.W <= 0 || r.H <= 0 || r.X < 0 || r.Y < 0 || r.X+r.W > float32(size.width) || r.Y+r.H > float32(size.height) {
						t.Fatalf("%s must retain positive viewport geometry: %+v", label, r)
					}
					return r
				}
				questions := viewport("Native human questions")
				form := viewport("Connection form viewport")
				viewport("Allow native once")
				viewport("Deny native once")
				if questions.Y+questions.H > form.Y || size.bar.Height > 0 && questions.Y < size.bar.Height {
					t.Fatalf("pending questions and form viewports overlap: questions=%+v form=%+v", questions, form)
				}
				// Scroll each actual viewport to its lower actions without
				// answering the native request or submitting the connection.
				tester.Scroll(questions.X+questions.W/2, questions.Y+questions.H/2, 0, 10000)
				viewport("Send native answers")
				viewport("Skip native questions")
				// The outer gutter belongs to the full-dialog viewport, not
				// its nested field scroller. Its footer must remain reachable
				// even when both pending panels leave less than dialog chrome.
				tester.Scroll(form.X+form.W-4, form.Y+form.H/2, 0, 10000)
				footer := viewport("Connection form footer")
				cancel, save := viewport("Cancel connection draft"), viewport("Save connection")
				details := viewport("Connection form details")
				if details.Y+details.H > footer.Y || cancel.H != 32 || save.H != 32 || footer.Y+footer.H > form.Y+form.H {
					t.Fatalf("form footer overlaps details, shrinks actions or leaves its viewport: details=%+v footer=%+v cancel=%+v save=%+v form=%+v", details, footer, cancel, save, form)
				}
				before, _ := tester.Find("Enable model pending-model-15")
				tester.Scroll(details.X+details.W/2, details.Y+details.H/2, 0, 10000)
				model := viewport("Enable model pending-model-15")
				if before.H > 0 && model.Y >= before.Y || model.Y < details.Y || model.Y+model.H > details.Y+details.H {
					t.Fatalf("connection detail viewport did not scroll to the last draft model: before=%+v model=%+v details=%+v", before, model, details)
				}
				viewport("Cancel connection draft")
				viewport("Save connection")
				if pending.questionCalls.Load() != 0 || pending.permissionCalls.Load() != 0 || store.writes.Load() != 0 || len(p.Service.Connections.Snapshot()) != 0 || !p.Service.Runtime.HasActive() {
					t.Fatal("layout or scrolling implicitly answered, saved, or stopped the real pending owner")
				}
			})
		}
	}
}

func TestWorkspaceSettingsConnectionFormRetainsUncontendedHeight(t *testing.T) {
	for _, size := range []struct{ width, height int }{{1240, 850}, {960, 680}} {
		for _, source := range []string{"OpenAI", "Custom", "Local"} {
			t.Run(fmt.Sprintf("%dx%d/%s", size.width, size.height, source), func(t *testing.T) {
				service, store := connectionsFixture(t)
				p := NewNativePage(service)
				p.openConnections()
				tester := ui.NewTester(workspaceStaticView(p), size.width, size.height)
				settleConnections(t, p, tester)
				p.chooseConnectionSource(source)
				tester.Frame()
				check := func(height float32) {
					t.Helper()
					dialog, ok := tester.Find("Connection form dialog")
					body, bodyOK := tester.Find("Connection sheet body")
					wantY := body.Y + 24 + (body.H-48-height)/2
					if !ok || !bodyOK || dialog.W != 800 || dialog.H != height || dialog.Y != wantY || dialog.Y+dialog.H > body.Y+body.H-24 {
						t.Fatalf("uncontended form lost its centered remaining-body dimensions: dialog=%+v body=%+v want height=%v y=%v", dialog, body, height, wantY)
					}
				}
				if source == "OpenAI" {
					check(553)
					connectionClick(t, tester, "Connection details")
				}
				check(float32(size.height) - 100)
				if store.writes.Load() != 0 || service.Runtime.HasActive() {
					t.Fatal("sizing an untouched form saved or executed")
				}
			})
		}
	}
}

func TestWorkspaceSettingsPendingSourcesKeepEveryChoiceReachable(t *testing.T) {
	for _, bar := range []ui.TitleBar{{Height: 52, Left: 86}, {Height: 52, Right: 138}} {
		t.Run(fmt.Sprint(bar), func(t *testing.T) {
			p, pending := workspaceInteractionPage(t)
			store := &connectionFaultStore{ConnectionsStore: p.Service.Store.(hub.ConnectionsStore)}
			var err error
			p.Service.Connections, err = product.NewConnectionsService(t.Context(), store)
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() { _ = p.Service.Connections.Close(context.Background()) })
			p.openConnections()
			tester := ui.NewTester(workspaceStaticView(p), 960, 680)
			tester.SetTitleBar(bar)
			settleConnections(t, p, tester)
			p.connections.mode = "sources"
			tester.Frame()
			for _, source := range []string{"OpenAI", "Anthropic", "Gemini", "Custom", "Local"} {
				view, viewOK := tester.Find("Connection sources viewport")
				questions, questionsOK := tester.Find("Native human questions")
				body, bodyOK := tester.Find("Connection sheet body")
				if !viewOK || !questionsOK || !bodyOK || view.X != 32 || view.W != 896 || view.H <= 0 ||
					questions.Y+questions.H != view.Y || view.Y+view.H != body.Y+body.H-24 || questions.Y < bar.Height {
					t.Fatal("sources viewport overlaps pending or escapes its remaining body", view, questions, body)
				}
				label := "Add " + source + " connection"
				row, ok := tester.Find(label)
				if !ok {
					t.Fatal("missing source option", source)
				}
				tester.Scroll(view.X+view.W-4, view.Y+view.H/2, 0, row.Y-view.Y-8)
				row, ok = tester.Find(label)
				wantHeight := float32(32)
				if source == "Custom" || source == "Local" {
					wantHeight = 48
				}
				if !ok || row.X != 144 || row.W != 672 || row.H != wantHeight || row.Y < view.Y || row.Y+row.H > view.Y+view.H {
					t.Fatal("source option is not fully reachable after scrolling", source, row, view)
				}
				connectionClick(t, tester, label)
				if p.connections.mode != "form" || p.connections.source != source {
					t.Fatal("source action was clipped or selected another provider", source, p.connections.mode)
				}
				connectionClick(t, tester, "Back from connection form")
				if p.connections.mode != "sources" {
					t.Fatal("source return lost its original destination")
				}
			}
			view, _ := tester.Find("Connection sources viewport")
			tester.Scroll(view.X+view.W-4, view.Y+view.H/2, 0, -10000)
			close, closeOK := tester.Find("Cancel connection sources")
			if !closeOK || close.X != 788 || close.Y != view.Y+24 || close.W != 28 || close.H != 28 {
				t.Fatal("source close did not remain reachable at the top of its card", close, view)
			}
			questions, _ := tester.Find("Native human questions")
			tester.Scroll(questions.X+questions.W/2, questions.Y+questions.H/2, 0, 10000)
			for _, label := range []string{"Send native answers", "Skip native questions", "Allow native once", "Deny native once"} {
				action, ok := tester.Find(label)
				if !ok || action.H != 32 || action.Y < bar.Height || action.Y+action.H > view.Y {
					t.Fatal("pending action became unreachable beside the sources viewport", label, action)
				}
			}
			connectionClick(t, tester, "Cancel connection sources")
			if p.connections.mode != "" {
				t.Fatal("source close stopped using its original cancel handler")
			}
			p.connections.mode = "sources"
			tester.Frame()
			connectionClick(t, tester, "返回模型与连接")
			if p.connections.mode != "" || pending.questionCalls.Load() != 0 || pending.permissionCalls.Load() != 0 || store.writes.Load() != 0 ||
				len(p.Service.Connections.Snapshot()) != 0 || !p.Service.Runtime.HasActive() {
				t.Fatal("scrolling, choosing or returning implicitly answered, saved or stopped the actual pending owner")
			}
		})
	}
}

func TestWorkspaceSettingsSourcesKeepUncontendedCardCentered(t *testing.T) {
	for _, bar := range []ui.TitleBar{{}, {Height: 52, Left: 86}, {Height: 52, Right: 138}} {
		service, store := connectionsFixture(t)
		p := NewNativePage(service)
		p.openConnections()
		tester := ui.NewTester(workspaceStaticView(p), 960, 680)
		tester.SetTitleBar(bar)
		settleConnections(t, p, tester)
		p.connections.mode = "sources"
		tester.Frame()
		body, bodyOK := tester.Find("Connection sheet body")
		dialog, dialogOK := tester.Find("Connection source dialog")
		if !bodyOK || !dialogOK || dialog.X != 120 || dialog.W != 720 || dialog.H != 419 || dialog.Y != body.Y+(body.H-dialog.H)/2 ||
			store.writes.Load() != 0 || service.Runtime.HasActive() {
			t.Fatal("bounded source scrolling changed the normal card size, center or cold behavior", bar, body, dialog)
		}
	}
}
