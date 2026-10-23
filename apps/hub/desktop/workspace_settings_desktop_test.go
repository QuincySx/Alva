// INPUT: Native MyGo controls, durable Hub fixtures and bounded local execution/playback owners.
// OUTPUT: Shared Settings geometry, origin/draft preservation and media cleanup navigation regressions.
// POS: [L7 Hub UI testing] Exercises real navigation without an OS window, external model or decoder.
//go:build desktop

package hubdesktop

import (
	"bytes"
	"context"
	"image"
	"reflect"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/egoist/mygo/ui"
)

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

func TestWorkspaceSettingsBackStaysAtNavigationTop(t *testing.T) {
	service := preferenceFixture(t)
	for _, size := range []struct{ width, height int }{{1240, 850}, {1000, 720}} {
		page := NewNativePage(service)
		tester := ui.NewTester(page.Build, size.width, size.height)
		openPreferencePage(t, page, tester)
		check := func() {
			t.Helper()
			back, ok := tester.Find("Back to workspace")
			nav, navOK := tester.Find("Settings navigation")
			title, titleOK := tester.Find("设置")
			if !ok || !navOK || !titleOK || nav.X != 0 || nav.W != workspaceSidebarWidth || back.X < 12 || back.Y != 12 || back.W >= workspaceSidebarWidth-24 || title.Y < back.Y+back.H || back.X+back.W > nav.W {
				t.Fatal("shared Back is not a content-width action before the Settings title", back, nav, title)
			}
			if !tester.HasText("返回工作台") || tester.HasText("Back to conversation") || tester.HasText("模型与连接") || tester.HasText("权限设置") {
				t.Fatal("Settings omitted its shared return or advertised an unsupported category", tester.Texts())
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
