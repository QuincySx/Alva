// INPUT: Real bootstrap/SQLite services, MyGo full-page controls and delayed actual evidence reads.
// OUTPUT: Formal reading geometry, distinct same-named sources, Markdown/raw/integrity/find and durable observer-tab regressions.
// POS: [L7 Hub UI tests] Exercises resource observation without OS windows, user files or external accounts.
//go:build desktop

package hubdesktop

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/platform/storage"
	"github.com/egoist/mygo/ui"
)

func resourcePageFixture(t *testing.T) (*NativePage, hub.Session) {
	t.Helper()
	s, _ := fixture(t)
	port, ok := s.Store.(hub.WorkspaceViewStore)
	if !ok {
		t.Fatal("fixture workspace view capability missing")
	}
	var err error
	s.WorkspaceViews, err = product.NewWorkspaceViewsService(port)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = s.WorkspaceViews.Close(context.Background()) })
	session, err := s.Create(t.Context(), "Resource task", "")
	if err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(s)
	p.selected = session.ID
	p.detail, err = s.Store.Detail(t.Context(), session.ID)
	if err != nil {
		t.Fatal(err)
	}
	p.acceptWorkspaceNavigationDetail(session.ID, p.generation)
	p.selectWorkspaceResources(session.ID)
	return p, session
}

func resourceTester(p *NativePage, width, height int) *ui.Tester {
	return ui.NewTester(func(c *ui.Context) {
		c.DisableAnimations() // This harness checks settled resource geometry and data.
		p.applyPreferences(c)
		for i := 0; i < 8; i++ {
			select {
			case update := <-p.updates:
				update()
			default:
				i = 8
			}
		}
		ui.Row(c).Fill().Children(func() {
			if !p.workspaceResourcesFullWidth(float32(width)) {
				ui.Box(c).Grow(1).FillHeight()
			}
			p.buildWorkspaceResources(c, true)
			p.buildWorkspaceResourceToggle(c, 0, 0)
		})
	}, width, height)
}

func TestWorkspaceResourcePopupReturnAndPageActionsStayVisible(t *testing.T) {
	for _, width := range []int{960, 1240} {
		t.Run(fmt.Sprintf("width=%d", width), func(t *testing.T) {
			p, session := resourcePageFixture(t)
			tester := resourceTester(p, width, 680)
			settleResource(t, p, tester, func() bool { return p.workspaceResourceSession().loaded })
			file, err := p.Service.UploadReader(t.Context(), session.ID, "paged.txt", strings.NewReader(strings.Repeat("bounded range\n", 900)))
			if err != nil {
				t.Fatal(err)
			}
			p.openWorkspaceResource(hub.WorkspaceTab{Kind: "artifact", Title: "Paged text", URI: file.Reference.URI, BranchID: session.ActiveBranchID})
			settleResource(t, p, tester, func() bool { return !p.resources.loading && p.resources.page.Body.HasMore })
			sha := p.resources.page.Body.SHA256
			if sha == "" || tester.HasText("SHA-256 "+sha) {
				t.Fatal("artifact integrity fact is missing or displaces default reading")
			}
			connectionClick(t, tester, "Resource reading options")
			if err := tester.ChooseMenuItem("查看校验信息"); err != nil {
				t.Fatal(err)
			}
			if !tester.HasText("SHA-256 " + sha) {
				t.Fatal("explicit integrity view lost the actual stored artifact SHA")
			}
			start, found := tester.Find("起始位置")
			next, nextFound := tester.Find("下一范围")
			if !found || !nextFound || start.W < 80 || next.W < 80 || next.X < start.X+start.W || next.X+next.W > float32(width) || next.Y+next.H > 680 {
				t.Fatal("pagination lost one of its two complete action targets", start, next)
			}
			connectionClick(t, tester, "下一范围")
			settleResource(t, p, tester, func() bool { return !p.resources.loading && p.workspaceActiveResource().Offset > 0 })
			connectionClick(t, tester, "起始位置")
			settleResource(t, p, tester, func() bool { return !p.resources.loading && p.workspaceActiveResource().Offset == 0 })
			calls := 0
			p.resources.popout = session.ID + "\x00" + p.workspaceActiveResource().ID
			p.returnResourceWindow = func() { calls++ }
			tester.Frame() // Builds the actual popup header and resolves its SVG.
			button, found := tester.Find("More actions")
			if !found || button.W != 28 || button.H != 28 || button.X < 0 || button.X+button.W > float32(width) {
				t.Fatal("popup return action is missing or clipped", button)
			}
			if err := workspaceTestMenu(tester, "收回资料窗口"); err != nil {
				t.Fatal(err)
			}
			if calls != 1 || p.selected != session.ID || p.Service.Runtime.HasActive() {
				t.Fatal("popup return did not dispatch its independent window action")
			}
		})
	}
}

func TestWorkspaceResourcesSelectedLongTabRevealsCloseAndUsesSurface(t *testing.T) {
	for _, dark := range []bool{false, true} {
		for _, width := range []int{420, 1240} {
			t.Run(fmt.Sprintf("dark=%v/width=%d", dark, width), func(t *testing.T) {
				p, session := resourcePageFixture(t)
				tester := resourceTester(p, width, 680)
				tester.SetDark(dark)
				settleResource(t, p, tester, func() bool { return p.workspaceResourceSession().loaded })
				p.openWorkspaceResource(hub.WorkspaceTab{Kind: "files", Title: "first-long-" + strings.Repeat("name", 25), BranchID: session.ActiveBranchID})
				p.openWorkspaceResource(hub.WorkspaceTab{Kind: "artifacts", Title: "second-long-" + strings.Repeat("name", 25), BranchID: session.ActiveBranchID})
				settleResource(t, p, tester, func() bool { return !p.resources.loading })
				title := p.workspaceActiveResource().Title
				close, found := tester.Find("Close resource " + title)
				tabs, tabsFound := tester.Find("Resource tabs")
				header, headerFound := tester.Find("Resource header")
				if !found || !tabsFound || !headerFound || header.H != 36 || tabs.H != 28 || close.W != 16 || close.H != 28 ||
					close.X < tabs.X || close.X+close.W > tabs.X+tabs.W+.5 {
					t.Fatal("active tab reveal excludes its independent close action or adds another header row", header, tabs, close)
				}
				active, found := tester.Find("Select resource " + title)
				panel, panelFound := tester.Find("Resource workspace")
				if !found || !panelFound || active.W < 20 {
					t.Fatal("selected resource lost its activation target", active, panel)
				}
				selected := tester.Image().RGBAAt(int(active.X+active.W/2), int(active.Y+2))
				surface, subtle := ui.Hex("#FFFFFF"), ui.Hex("#F5F6F8")
				if dark {
					surface, subtle = ui.Hex("#1C1F26"), ui.Hex("#252932")
				}
				background := tester.Image().RGBAAt(int(panel.X+panel.W/2), int(panel.Y+panel.H-4))
				if selected.R != surface.R || selected.G != surface.G || selected.B != surface.B ||
					background.R != subtle.R || background.G != subtle.G || background.B != subtle.B {
					t.Fatal("selected tab and resource background lost their independent Foundation roles", selected, background)
				}
				connectionClick(t, tester, "Close resource "+title)
				if len(p.workspaceResourceSession().view.Tabs) != 1 || p.Service.Runtime.HasActive() {
					t.Fatal("visible close did not remove only its observation tab")
				}
			})
		}
	}
}

func TestWorkspaceResourceFormalCompositionUsesRealFileAndSQLite(t *testing.T) {
	// Native shell: Foundation 25e27c5ec18d7f7f9ff6f42cf002d4bc38869f3a6ebb30eabb8891b6ecd56cc3,
	// Product 2dbf464cac3cfc7a9a61435ffc2690760a5227cd802be39e1e4d3f53e96605f7.
	for _, native := range []struct {
		name string
		bar  ui.TitleBar
		top  float32
	}{{name: "no-native-facts", top: 12}, {name: "mac-native", bar: ui.TitleBar{Height: 52, Left: 86}, top: 8}} {
		for _, dark := range []bool{false, true} {
			t.Run(fmt.Sprintf("%s/dark=%v", native.name, dark), func(t *testing.T) {
				config := bootstrap.Config{Demo: true, DataDirectory: filepath.Join(t.TempDir(), "private"), OpenStores: bootstrap.LocalStores}
				open := func() *bootstrap.Application {
					app, err := bootstrap.Open(t.Context(), config)
					if err != nil {
						t.Fatal(err)
					}
					t.Cleanup(func() { _ = app.Close(context.Background()) })
					return app
				}
				app := open()
				workspace := t.TempDir()
				content := "# Alva 工作台方案\n\n以对话为中心，让资料、执行与成果各有位置。\n\n## 01 模型连接\n\n选择模型来源，再启用需要的模型。\n\n## 02 长期伙伴\n\n伙伴保留职责、记忆与 Skills。\n"
				if err := os.WriteFile(filepath.Join(workspace, "工作台方案.md"), []byte(content), 0600); err != nil {
					t.Fatal(err)
				}
				session, err := app.Service.Create(t.Context(), "整理桌面工作台", workspace)
				if err != nil {
					t.Fatal(err)
				}
				seedWorkspaceResourceConversation(t, app, session)
				p := NewNativePage(app.Service)
				tester := ui.NewTester(workspaceStaticView(p), 1440, 960)
				tester.SetTitleBar(native.bar)
				tester.SetDark(dark)
				p.toggleWorkspaceResources()
				if _, shown := tester.Find("Resource workspace"); shown {
					t.Fatal("homepage toggle created an unowned empty resource pane")
				}
				p.selectSession(session.ID)
				p.refreshWorkspaceNavigation()
				settleResource(t, p, tester, func() bool {
					s := p.workspaceResourceSession()
					return s != nil && s.loaded && p.detail.Session.ID == session.ID && !p.busy &&
						len(p.model.Messages) == 2 && len(p.sessions) == 1 && p.navigation.loaded
				})
				if _, shown := tester.Find("Resource workspace"); shown {
					t.Fatal("a selected session without resource tabs must keep the chat layout")
				}
				openWorkspaceFilesForTest(t, p, tester)
				settleResource(t, p, tester, func() bool { return !p.resources.loading && len(p.resources.page.Entries) == 1 })
				if p.resources.page.Processing == "" || !tester.HasText(p.resources.page.Processing) {
					t.Fatal("default resource index hid its actual bounded-scan status")
				}
				search, searchFound := tester.Find("Search resource names or evidence")
				searchRow, _ := tester.Find("Resource path and find")
				if !searchFound || search.Y < searchRow.Y || search.Y+search.H > searchRow.Y+searchRow.H {
					t.Fatal("shared resource search control escaped its intrinsic row", searchRow, search)
				}
				connectionClick(t, tester, "Open resource 工作台方案.md")
				settleResource(t, p, tester, func() bool { return !p.resources.loading && p.resources.page.Body.Title != "" })
				if string(p.resources.page.Body.Data) != content || !tester.HasText("Alva 工作台方案") {
					t.Fatal("reading view did not render the actual file Markdown")
				}
				panel, found := tester.Find("Resource workspace")
				header, _ := tester.Find("Resource header")
				path, _ := tester.Find("Resource path and find")
				document, _ := tester.Find("Resource document")
				main, mainFound := tester.Find("Workspace main surface")
				pageHeader, pageHeaderFound := tester.Find("Workspace header")
				paperY := b3PaperY(native.bar)
				if !mainFound || main != (ui.Rect{X: 236, Y: paperY, W: 756, H: 960 - paperY - 8}) ||
					!pageHeaderFound || pageHeader.Y != b3Top(native.bar) || pageHeader.H != 36 ||
					!found || panel != (ui.Rect{X: 1000, Y: b3Top(native.bar), W: 432, H: 960 - b3Top(native.bar) - 8}) ||
					header.Y != panel.Y || header.H != 36 || path.Y != paperY+1 || path.H != 32 ||
					document.X != panel.X+17 || document.Y != path.Y+path.H+16 || document.W != panel.W-34 {
					t.Fatal("real page composition diverged from formal22 native-shell geometry", main, pageHeader, panel, header, path, document)
				}
				connectionClick(t, tester, "Collapse session navigation")
				collapsedMain, _ := tester.Find("Workspace main surface")
				collapsedPanel, _ := tester.Find("Resource workspace")
				_, sidebarShown := tester.Find("Session navigation")
				_, splitterShown := tester.Find("Resize session navigation")
				if sidebarShown || splitterShown || collapsedMain != (ui.Rect{X: 8, Y: paperY, W: 984, H: 960 - paperY - 8}) || collapsedPanel != panel {
					t.Fatal("collapsed navigation left a rail or changed the formal reading panel", collapsedMain, collapsedPanel)
				}
				connectionClick(t, tester, "Expand session navigation")
				title, titleFound := tester.Find("Alva 工作台方案")
				section, sectionFound := tester.Find("01 模型连接")
				lead, leadFound := tester.Find("以对话为中心，让资料、执行与成果各有位置。")
				if !titleFound || !sectionFound || !leadFound || title.Y != document.Y || title.H <= 0 || section.H <= 0 || lead.Y != title.Y+title.H+24 {
					t.Fatal("Markdown title/lead spacing or content bounds diverged from the reading layout", title, section, lead, document)
				}
				if tester.HasText("SHA-256 " + p.resources.page.Body.SHA256) {
					t.Fatal("byte metadata displaced the default document reading view")
				}
				connectionClick(t, tester, "Find in loaded resource content")
				connectionClick(t, tester, "Find in currently loaded content")
				tester.Type("伙伴")
				if !tester.HasText("当前载入内容：2 处匹配") || string(p.resources.page.Body.Data) != content || p.resources.loading {
					t.Fatalf("current-range find: query=%q open=%v loading=%v unchanged=%v texts=%q", p.resources.findQuery, p.resources.find, p.resources.loading, string(p.resources.page.Body.Data) == content, tester.Texts())
				}
				connectionClick(t, tester, "Close resource find")
				connectionClick(t, tester, "Resource reading options")
				if err := tester.ChooseMenuItem("查看校验信息"); err != nil {
					t.Fatal(err)
				}
				bytes := fmt.Sprintf("0–%d / %d bytes", len(content), len(content))
				if !tester.HasText(bytes) || !tester.HasText(p.resources.page.Body.Processing) {
					t.Fatal("explicit verification action lost actual file range metadata")
				}
				connectionClick(t, tester, "Resource reading options")
				if err := tester.ChooseMenuItem("查看原始内容"); err != nil {
					t.Fatal(err)
				}
				if !tester.HasText(content) {
					t.Fatal("raw view did not preserve the exact loaded Markdown")
				}
				connectionClick(t, tester, "Resize resource workspace")
				tester.Key(0, ui.KeyLeft)
				settleResource(t, p, tester, func() bool { s := p.workspaceResourceSession(); return !s.saving && s.changed == s.saved })
				if p.workspaceResourceSession().view.Width != 448 {
					t.Fatal("composition resize did not preserve the actual width preference")
				}
				tester.SetSize(960, 680)
				panel, _ = tester.Find("Resource workspace")
				if _, shown := tester.Find("Task composer"); shown || panel != (ui.Rect{X: 236, Y: b3Top(native.bar), W: 716, H: 680 - b3Top(native.bar) - 8}) {
					t.Fatal("narrow expanded navigation lost its bounded focused resource", panel)
				}
				if _, shown := tester.Find("Collapse resource workspace"); !shown {
					t.Fatal("narrow resource focus lost its return action")
				}
				connectionClick(t, tester, "Collapse session navigation")
				panel, _ = tester.Find("Resource workspace")
				if _, shown := tester.Find("Task composer"); !shown || panel != (ui.Rect{X: 504, Y: b3Top(native.bar), W: 448, H: 680 - b3Top(native.bar) - 8}) || p.workspaceResourceSession().view.Width != 448 || string(p.resources.page.Body.Data) != content {
					t.Fatal("narrow collapsed navigation did not restore the 432px conversation beside the clamped resource", panel)
				}
				connectionClick(t, tester, "Expand session navigation")
				if _, shown := tester.Find("Task composer"); shown {
					t.Fatal("narrow expanded navigation no longer focuses the resource")
				}
				connectionClick(t, tester, "Collapse resource workspace")
				if _, shown := tester.Find("Task composer"); !shown || len(p.resources.page.Body.Data) != 0 {
					t.Fatal("collapse failed to return to chat and release the observed body")
				}
				p.toggleWorkspaceResources()
				settleResource(t, p, tester, func() bool { return !p.resources.loading && p.resources.page.Body.Title != "" })
				tester.SetSize(1440, 960)
				panel, _ = tester.Find("Resource workspace")
				if panel.W != 448 || p.resources.metadata || p.resources.raw || p.resources.find {
					t.Fatal("widen/reopen lost the saved width or retained stale transient reading state", panel)
				}
				connectionClick(t, tester, "Close resource 工作台方案.md")
				settleResource(t, p, tester, func() bool {
					s := p.workspaceResourceSession()
					return !p.resources.loading && !s.saving && s.changed == s.saved
				})
				if p.workspaceActiveResource().Kind != "files" || app.Runtime.HasActive() {
					t.Fatal("closing the file changed execution rather than only its observation tab")
				}
				if err := app.Close(context.Background()); err != nil {
					t.Fatal(err)
				}
				reopened := open()
				restored := NewNativePage(reopened.Service)
				restoredTester := ui.NewTester(workspaceStaticView(restored), 1440, 960)
				restoredTester.SetTitleBar(native.bar)
				restoredTester.SetDark(dark)
				restored.selectSession(session.ID)
				settleResource(t, restored, restoredTester, func() bool {
					s := restored.workspaceResourceSession()
					return s != nil && s.loaded && !restored.resources.loading && restored.detail.Session.ID == session.ID
				})
				panel, found = restoredTester.Find("Resource workspace")
				if !found || panel.W != 448 || len(restored.workspaceResourceSession().view.Tabs) != 1 || reopened.Runtime.HasActive() {
					t.Fatal("SQLite reopen lost actual tab/width state or executed", panel, restored.workspaceResourceSession())
				}
			})
		}
	}
}

func TestWorkspaceResourcesSameNamedFilesKeepDistinctRowsAndExactReads(t *testing.T) {
	p, session := resourcePageFixture(t)
	paths := []string{"README.md", "docs/README.md", "integration/protocol/aep/python/README.md"}
	for _, path := range paths {
		name := filepath.Join(session.Workspace, filepath.FromSlash(path))
		if err := os.MkdirAll(filepath.Dir(name), 0700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(name, []byte("Original from "+path), 0600); err != nil {
			t.Fatal(err)
		}
	}
	tester := ui.NewTester(workspaceStaticView(p), 1440, 960)
	settleResource(t, p, tester, func() bool { return p.workspaceResourceSession().loaded })
	openWorkspaceFilesForTest(t, p, tester)
	settleResource(t, p, tester, func() bool { return !p.resources.loading && p.resources.page.Processing != "" })
	indexID := p.workspaceActiveResource().ID
	connectionClick(t, tester, "Search resource names or evidence")
	tester.Type("README.md")
	tester.Key(0, ui.KeyEnter)
	settleResource(t, p, tester, func() bool {
		return !p.resources.loading && p.workspaceActiveResource().Query == "README.md"
	})
	if len(p.resources.page.Entries) != len(paths) {
		t.Fatal("metadata search lost same-named sources", p.resources.page)
	}
	viewport, _ := tester.Find("Resource contents")
	lastY := viewport.Y
	for _, path := range paths {
		row, found := tester.Find("Open resource " + path)
		if !found || row.Y < lastY || row.Y+row.H > viewport.Y+viewport.H {
			t.Fatal("same-named file row is missing, overlapping or outside the viewport", path, row, viewport)
		}
		lastY = row.Y + row.H
	}
	opened := make(map[string]string, len(paths))
	for _, path := range paths {
		connectionClick(t, tester, "Open resource "+path)
		settleResource(t, p, tester, func() bool { return !p.resources.loading && p.resources.page.Body.Title != "" })
		if tab := p.workspaceActiveResource(); tab.File == nil || tab.File.Path != path || string(p.resources.page.Body.Data) != "Original from "+path {
			t.Fatal("same-named row opened another source", path, tab, p.resources.page.Body)
		}
		opened[path] = p.workspaceActiveResource().ID
		p.activateWorkspaceResource(indexID)
		settleResource(t, p, tester, func() bool { return !p.resources.loading && len(p.resources.page.Entries) == len(paths) })
		if p.workspaceActiveResource().Query != "README.md" {
			t.Fatal("returning to the index lost its explicit query")
		}
	}
	p.resources.reveal = opened["docs/README.md"]
	tester.Frame()
	connectionClick(t, tester, "Select resource docs/README.md")
	settleResource(t, p, tester, func() bool { return !p.resources.loading && p.resources.page.Body.Title != "" })
	if p.workspaceActiveResource().ID != opened["docs/README.md"] || string(p.resources.page.Body.Data) != "Original from docs/README.md" {
		t.Fatal("path-labelled tab selected another same-named source")
	}
	connectionClick(t, tester, "Close resource docs/README.md")
	remaining := map[string]bool{}
	for _, tab := range p.workspaceResourceSession().view.Tabs {
		remaining[tab.ID] = true
	}
	if remaining[opened["docs/README.md"]] || !remaining[indexID] || !remaining[opened["README.md"]] || !remaining[opened["integration/protocol/aep/python/README.md"]] || len(remaining) != 3 {
		t.Fatal("path-labelled close removed a different same-named tab", remaining)
	}
	if p.Service.Runtime.HasActive() {
		t.Fatal("browsing or explicitly reading a file started execution")
	}
}

func TestWorkspaceResourceMarkdownHeadingReferencesUseWholeDocument(t *testing.T) {
	app, err := bootstrap.Open(t.Context(), bootstrap.Config{Demo: true, DataDirectory: filepath.Join(t.TempDir(), "private"), OpenStores: bootstrap.LocalStores})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = app.Close(context.Background()) })
	workspace := t.TempDir()
	content := "# [说明][jump] [^note]\n\n[jump]: #target\n\n" + strings.Repeat("用于滚动的文档正文。\n\n", 36) +
		"## Target\n\n目标内容。\n\n[^note]: 独立注释内容。\n"
	if len(content) > product.WorkspaceResourcePageBytes {
		t.Fatal("reference fixture must fit one actual bounded range")
	}
	if err := os.WriteFile(filepath.Join(workspace, "references.md"), []byte(content), 0600); err != nil {
		t.Fatal(err)
	}
	session, err := app.Service.Create(t.Context(), "标题引用与脚注", workspace)
	if err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(app.Service)
	tester := ui.NewTester(workspaceStaticView(p), 1440, 960)
	p.selectSession(session.ID)
	settleResource(t, p, tester, func() bool {
		s := p.workspaceResourceSession()
		return s != nil && s.loaded && p.workspaceNavigationDetailReady() && !p.busy
	})
	openWorkspaceFilesForTest(t, p, tester)
	settleResource(t, p, tester, func() bool { return !p.resources.loading && len(p.resources.page.Entries) == 1 })
	connectionClick(t, tester, "Open resource references.md")
	settleResource(t, p, tester, func() bool { return !p.resources.loading && p.resources.page.Body.Title != "" })
	viewport, found := tester.Find("Resource contents")
	if !found || !tester.HasText("说明") || tester.HasText("[说明][jump]") {
		t.Fatal("reference-style heading was reparsed without its definition")
	}
	assertTarget := func() bool {
		target, found := tester.Find("Target")
		scroll := p.workspaceResourceSession().scroll[p.workspaceActiveResource().ID]
		return found && scroll.Y > 0 && target.Y >= viewport.Y && target.Y+target.H <= viewport.Y+viewport.H
	}
	connectionClick(t, tester, "说明")
	settleResource(t, p, tester, assertTarget)
	if len(tester.OpenedURLs()) != 0 || p.resources.loading || string(p.resources.page.Body.Data) != content {
		t.Fatal("heading anchor opened an external destination or reread/replaced the observed file")
	}
	// Native footnotes belong to this same document, not to miniature
	// heading views. Compare against another single paragraph so the test
	// does not depend on the tester's label/text duplication convention.
	tester.Scroll(viewport.X+viewport.W/2, viewport.Y+viewport.H/2, 0, 10000)
	texts := tester.Texts()
	count := func(value string) int {
		n := 0
		for _, text := range texts {
			if text == value {
				n++
			}
		}
		return n
	}
	if notes, paragraph := count("独立注释内容。"), count("目标内容。"); notes == 0 || notes != paragraph {
		t.Fatal("heading references lost or duplicated the native document footnotes", notes, paragraph)
	}
	tester.Scroll(viewport.X+viewport.W/2, viewport.Y+viewport.H/2, 0, -10000)
	p.resources.markdown.Anchor = "#target"
	p.resources.markdown.AnchorVersion++
	settleResource(t, p, tester, assertTarget)
	if app.Runtime.HasActive() {
		t.Fatal("reading document anchors started an execution")
	}
}

func TestWorkspaceResourcesDropTargetKeepsVisibleCue(t *testing.T) {
	p, session := resourcePageFixture(t)
	var accent ui.Color
	tester := ui.NewTester(func(c *ui.Context) {
		c.DisableAnimations() // This fixture asserts a settled tab drop cue.
		p.applyPreferences(c)
		accent = c.Theme().Accent
		for i := 0; i < 8; i++ {
			select {
			case update := <-p.updates:
				update()
			default:
				i = 8
			}
		}
		ui.Row(c).Fill().Children(func() {
			ui.Box(c).Grow(1).FillHeight()
			p.buildWorkspaceResources(c, true)
			p.buildWorkspaceResourceToggle(c, 0, 0)
		})
	}, 1600, 680)
	settleResource(t, p, tester, func() bool { return p.workspaceResourceSession().loaded })
	p.openWorkspaceResource(hub.WorkspaceTab{Kind: "files", Title: "Alpha", BranchID: session.ActiveBranchID})
	p.openWorkspaceResource(hub.WorkspaceTab{Kind: "artifacts", Title: "Beta", BranchID: session.ActiveBranchID})
	settleResource(t, p, tester, func() bool { return !p.resources.loading })
	source, sourceFound := tester.Find("Select resource Beta")
	target, targetFound := tester.Find("Select resource Alpha")
	closeTarget, closeFound := tester.Find("Close resource Alpha")
	if !sourceFound || !targetFound || !closeFound {
		t.Fatal("fixture did not expose its two complete resource tabs")
	}
	tester.Press(source.X+20, source.Y+16)
	tester.Move(source.X+8, source.Y+16)
	tester.Move(target.X+20, target.Y+16)
	tester.Frame()
	count := 0
	// Tester.Image converts the whole frame on each call; scan one copy.
	frame := tester.Image()
	for y := int(target.Y); y < int(target.Y+target.H); y++ {
		for x := int(target.X - 8); x < int(closeTarget.X+closeTarget.W+8); x++ {
			pixel := frame.RGBAAt(x, y)
			if pixel.R == accent.R && pixel.G == accent.G && pixel.B == accent.B {
				count++
			}
		}
	}
	tester.Release(target.X+20, target.Y+16)
	if p.workspaceResourceSession().view.Tabs[0].Title != "Beta" || count == 0 {
		t.Fatal("dragging must show its destination before moving the actual tab order", count)
	}
}

func TestWorkspaceResourcesPointerAndKeyboardReorderResizePersistAndClamp(t *testing.T) {
	p, session := resourcePageFixture(t)
	tester := resourceTester(p, 1600, 680)
	settleResource(t, p, tester, func() bool { return p.workspaceResourceSession().loaded })
	for _, item := range []struct{ kind, title string }{{"files", "Project"}, {"artifacts", "Originals"}, {"inspector", "Inspector"}} {
		p.openWorkspaceResource(hub.WorkspaceTab{Kind: item.kind, Title: item.title, BranchID: session.ActiveBranchID})
	}
	settleResource(t, p, tester, func() bool {
		s := p.workspaceResourceSession()
		return !p.resources.loading && !s.saving && s.changed == s.saved
	})
	s := p.workspaceResourceSession()
	active := s.view.Active
	// All three tabs remain accessible in the horizontally scrolling strip.
	// Reveal the first two, drag the second onto the first, then use the
	// focused first tab's keyboard shortcut without changing selection.
	p.resources.reveal = s.view.Tabs[0].ID
	tester.Frame()
	source, ok := tester.Find("Select resource Originals")
	if !ok {
		t.Fatal("missing drag source")
	}
	target, ok := tester.Find("Select resource Project")
	if !ok {
		t.Fatal("missing drop target")
	}
	tester.Press(source.X+20, source.Y+16)
	tester.Move(source.X+8, source.Y+16)
	tester.Move(target.X+20, target.Y+16)
	tester.Release(target.X+20, target.Y+16)
	if s.view.Tabs[0].Title != "Originals" || s.view.Active != active {
		t.Fatal("pointer reorder lost order or active identity", s.view)
	}
	if err := tester.Click("Select resource Originals"); err != nil {
		t.Fatal(err)
	}
	tester.Key(ui.Alt|ui.Shift, ui.KeyRight)
	if s.view.Tabs[1].Title != "Originals" || p.workspaceActiveResource().Title != "Originals" {
		t.Fatal("keyboard reorder lost tab selection", s.view)
	}
	edge, ok := tester.Find("Resize resource workspace")
	if !ok {
		t.Fatal("split pane has no resize handle")
	}
	panel, _ := tester.Find("Resource workspace")
	if panel.W != 432 {
		t.Fatal("pane no longer uses Foundation default", panel)
	}
	tester.Press(edge.X+edge.W/2, edge.Y+80)
	tester.Move(edge.X+edge.W/2-40, edge.Y+80)
	tester.Move(edge.X+edge.W/2-80, edge.Y+80)
	tester.Release(edge.X+edge.W/2-80, edge.Y+80)
	if s.view.Width != 512 {
		t.Fatal("pointer resize did not preserve actual width", s.view.Width)
	}
	tester.Key(0, ui.KeyLeft)
	if s.view.Width != 528 {
		t.Fatal("focused splitter has no keyboard resize", s.view.Width)
	}
	settleResource(t, p, tester, func() bool { return !s.saving && s.saved == s.changed && !p.resources.loading })
	saved, err := p.Service.WorkspaceViews.Read(t.Context(), session.ID)
	if err != nil || saved.Width != 528 || saved.Tabs[1].Title != "Originals" {
		t.Fatal("drag/keyboard geometry did not use existing CAS", saved, err)
	}
	reopened := NewNativePage(p.Service)
	reopened.selected, reopened.detail = session.ID, p.detail
	reopened.selectWorkspaceResources(session.ID)
	narrower := resourceTester(reopened, 1180, 680)
	settleResource(t, reopened, narrower, func() bool {
		s := reopened.workspaceResourceSession()
		return s != nil && s.loaded && !reopened.resources.loading
	})
	// Preferred width survives viewport changes even when display is clamped.
	reopened.workspaceResourceSession().view.Width = 900
	narrower.Frame()
	clamped, _ := narrower.Find("Resource workspace")
	if clamped.W != 444 || reopened.workspaceResourceSession().view.Width != 900 {
		t.Fatal("viewport clamp rewrote preferred width", clamped, reopened.workspaceResourceSession().view.Width)
	}
	wide := resourceTester(reopened, 1800, 680)
	widened, _ := wide.Find("Resource workspace")
	if widened.W != 900 {
		t.Fatal("widening did not restore preferred width", widened)
	}
	if err = wide.Click("Resize resource workspace"); err != nil {
		t.Fatal(err)
	}
	wide.Key(0, ui.KeyHome)
	if reopened.workspaceResourceSession().view.Width != 0 {
		t.Fatal("Home did not restore Foundation default")
	}
}

type delayedWorkspaceViewReadStore struct {
	hub.WorkspaceViewStore
	id                         string
	reads                      atomic.Int32
	entered, canceled, release chan struct{}
}

func (s *delayedWorkspaceViewReadStore) WorkspaceView(ctx context.Context, id string) (hub.WorkspaceView, error) {
	view, err := s.WorkspaceViewStore.WorkspaceView(ctx, id)
	if id == s.id && s.reads.Add(1) == 1 {
		close(s.entered)
		<-ctx.Done()
		close(s.canceled)
		<-s.release
	}
	return view, err
}

func TestWorkspaceResourcesMetadataReadCancelsAndIsolatesSessionABA(t *testing.T) {
	p, a := resourcePageFixture(t)
	tester := resourceTester(p, 420, 680)
	settleResource(t, p, tester, func() bool { return p.workspaceResourceSession().loaded })
	b, err := p.Service.Create(t.Context(), "B", "")
	if err != nil {
		t.Fatal(err)
	}
	if err = p.Service.WorkspaceViews.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	store := &delayedWorkspaceViewReadStore{WorkspaceViewStore: p.Service.Store.(hub.WorkspaceViewStore), id: a.ID, entered: make(chan struct{}), canceled: make(chan struct{}), release: make(chan struct{})}
	p.Service.WorkspaceViews, err = product.NewWorkspaceViewsService(store)
	if err != nil {
		t.Fatal(err)
	}
	p.resources.sessions = nil
	p.reloadWorkspaceResources()
	<-store.entered
	p.selected = b.ID
	p.selectWorkspaceResources(b.ID)
	select {
	case <-store.canceled:
	case <-time.After(time.Second):
		t.Fatal("leaving session did not cancel its old metadata reader")
	}
	p.selected = a.ID
	p.selectWorkspaceResources(a.ID)
	settleResource(t, p, tester, func() bool { return p.workspaceResourceSession().loaded })
	current := p.workspaceResourceSession()
	close(store.release)
	settleResource(t, p, tester, func() bool { return len(p.updates) == 0 })
	if p.workspaceResourceSession() != current || current.problem != "" || current.loading || p.resources.session != a.ID {
		t.Fatal("old A read contaminated the replacement A observation", current)
	}
}

func TestWorkspaceResourcesCollapsePendingOpenCancelsWithoutLateReopen(t *testing.T) {
	p, session := resourcePageFixture(t)
	tester := resourceTester(p, 1600, 680)
	settleResource(t, p, tester, func() bool { return p.workspaceResourceSession().loaded })
	if err := p.Service.WorkspaceViews.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	store := &delayedWorkspaceViewReadStore{WorkspaceViewStore: p.Service.Store.(hub.WorkspaceViewStore), id: session.ID, entered: make(chan struct{}), canceled: make(chan struct{}), release: make(chan struct{})}
	var err error
	p.Service.WorkspaceViews, err = product.NewWorkspaceViewsService(store)
	if err != nil {
		t.Fatal(err)
	}
	p.resources.sessions = nil
	p.reloadWorkspaceResources()
	<-store.entered
	p.openWorkspaceResources()
	tester.Frame()
	if err = tester.Click("Collapse resource workspace"); err != nil {
		t.Fatal(err)
	}
	select {
	case <-store.canceled:
	case <-time.After(time.Second):
		t.Fatal("collapse did not cancel pending metadata read")
	}
	if p.workspaceResourcesVisible() || p.resources.openRequested {
		t.Fatal("collapsed pending open remained visible")
	}
	close(store.release)
	// A new explicit open gets a new metadata owner. The canceled initial
	// read must not replace it, reopen itself or publish a stale error.
	openWorkspaceFilesForTest(t, p, tester)
	settleResource(t, p, tester, func() bool {
		s := p.workspaceResourceSession()
		return s != nil && s.loaded && !p.resources.loading && len(s.view.Tabs) == 1
	})
	if p.workspaceResourceSession().problem != "" || p.workspaceActiveResource().Kind != "files" {
		t.Fatal("explicit reopen inherited canceled read state", p.workspaceResourceSession())
	}
}

type delayedWorkspaceViewSaveStore struct {
	hub.WorkspaceViewStore
	saves            atomic.Int32
	entered, release chan struct{}
}

func (s *delayedWorkspaceViewSaveStore) SaveWorkspaceView(ctx context.Context, view hub.WorkspaceView, expected uint64) (hub.WorkspaceView, error) {
	if s.saves.Add(1) == 1 {
		close(s.entered)
		<-s.release
	}
	return s.WorkspaceViewStore.SaveWorkspaceView(ctx, view, expected)
}

func TestWorkspaceResourcesOldSaveCannotReplaceNewSessionABAEdits(t *testing.T) {
	p, a := resourcePageFixture(t)
	tester := resourceTester(p, 1600, 680)
	settleResource(t, p, tester, func() bool { return p.workspaceResourceSession().loaded })
	b, err := p.Service.Create(t.Context(), "B", "")
	if err != nil {
		t.Fatal(err)
	}
	if err = p.Service.WorkspaceViews.Close(t.Context()); err != nil {
		t.Fatal(err)
	}
	store := &delayedWorkspaceViewSaveStore{WorkspaceViewStore: p.Service.Store.(hub.WorkspaceViewStore), entered: make(chan struct{}), release: make(chan struct{})}
	p.Service.WorkspaceViews, err = product.NewWorkspaceViewsService(store)
	if err != nil {
		t.Fatal(err)
	}
	p.openWorkspaceResource(hub.WorkspaceTab{Kind: "files", Title: "Project", BranchID: a.ActiveBranchID})
	<-store.entered
	p.selected = b.ID
	p.selectWorkspaceResources(b.ID)
	settleResource(t, p, tester, func() bool { return p.workspaceResourceSession().loaded })
	p.selected = a.ID
	p.selectWorkspaceResources(a.ID)
	s := p.workspaceResourceSession()
	p.openWorkspaceResource(hub.WorkspaceTab{Kind: "artifacts", Title: "Originals", BranchID: a.ActiveBranchID})
	p.moveWorkspaceResource(-1)
	p.resizeWorkspaceResources(s, 640, 1600)
	close(store.release)
	settleResource(t, p, tester, func() bool { return !s.saving && s.changed == s.saved && !p.resources.loading })
	saved, err := p.Service.WorkspaceViews.Read(t.Context(), a.ID)
	if err != nil || len(saved.Tabs) != 2 || saved.Tabs[0].Title != "Originals" || saved.Active != saved.Tabs[0].ID || saved.Width != 640 || saved.Revision != 2 {
		t.Fatal("old accepted save overwrote newer same-session edits", saved, err)
	}
	other, err := p.Service.WorkspaceViews.Read(t.Context(), b.ID)
	if err != nil || other.Revision != 0 || len(other.Tabs) != 0 {
		t.Fatal("old save crossed logical sessions", other, err)
	}
}

type countedWorkspaceArtifactRepository struct {
	storage.Repository
	reads atomic.Int32
	limit atomic.Int32
}

func (r *countedWorkspaceArtifactRepository) ReadArtifact(ctx context.Context, owner, id, uri string, offset int64, limit int) (storage.ArtifactPage, error) {
	r.reads.Add(1)
	r.limit.Store(int32(limit))
	return r.Repository.ReadArtifact(ctx, owner, id, uri, offset, limit)
}

func TestWorkspaceResourcesHiddenLargeOriginalAndBuildNeverReadBody(t *testing.T) {
	p, session := resourcePageFixture(t)
	attachment, err := p.Service.UploadReader(t.Context(), session.ID, "large.txt", strings.NewReader(strings.Repeat("large source line\n", 65536)))
	if err != nil {
		t.Fatal(err)
	}
	repository := &countedWorkspaceArtifactRepository{Repository: p.Service.Repository}
	p.Service.Repository = repository
	tester := resourceTester(p, 420, 680)
	settleResource(t, p, tester, func() bool { return p.workspaceResourceSession().loaded })
	p.openWorkspaceResource(hub.WorkspaceTab{Kind: "artifact", Title: "Large original", BranchID: session.ActiveBranchID, URI: attachment.Reference.URI})
	settleResource(t, p, tester, func() bool { return !p.resources.loading })
	if len(p.resources.page.Body.Data) != product.WorkspaceResourcePageBytes || repository.reads.Load() != 1 || repository.limit.Load() != product.WorkspaceResourcePageBytes {
		t.Fatal("opening large original was not a bounded explicit range", repository.reads.Load(), repository.limit.Load())
	}
	for i := 0; i < 12; i++ {
		tester.Frame()
	}
	if repository.reads.Load() != 1 {
		t.Fatal("Build reread resource bodies", repository.reads.Load())
	}
	if err = tester.Click("Collapse resource workspace"); err != nil {
		t.Fatal(err)
	}
	settleResource(t, p, tester, func() bool { s := p.workspaceResourceSession(); return !s.saving && s.changed == s.saved })
	if len(p.resources.page.Body.Data) != 0 {
		t.Fatal("collapse retained large original body")
	}
	reopened := NewNativePage(p.Service)
	reopened.selected, reopened.detail = session.ID, p.detail
	reopened.selectWorkspaceResources(session.ID)
	reopenedTester := resourceTester(reopened, 420, 680)
	settleResource(t, reopened, reopenedTester, func() bool { s := reopened.workspaceResourceSession(); return s != nil && s.loaded })
	if reopened.workspaceResourcesVisible() || repository.reads.Load() != 1 {
		t.Fatal("reopening hidden observation read body", repository.reads.Load())
	}
	reopened.openWorkspaceResources()
	settleResource(t, reopened, reopenedTester, func() bool { return !reopened.resources.loading })
	if repository.reads.Load() != 2 || len(reopened.resources.page.Body.Data) != product.WorkspaceResourcePageBytes {
		t.Fatal("explicit reopen did not read one bounded range", repository.reads.Load())
	}
}

func openWorkspaceFilesForTest(t *testing.T, p *NativePage, tester *ui.Tester) {
	t.Helper()
	p.openWorkspaceResources()
	settleResource(t, p, tester, func() bool { s := p.workspaceResourceSession(); return s != nil && s.loaded })
	if len(p.workspaceResourceSession().view.Tabs) == 0 {
		connectionClick(t, tester, "Open resource tab")
		if err := tester.ChooseMenuItem("项目文件"); err != nil {
			t.Fatal(err)
		}
	}
}

func settleResource(t *testing.T, p *NativePage, tester *ui.Tester, done func() bool) {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		tester.Frame()
		if done() {
			return
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatalf("resource did not settle: read=%v error=%s session=%+v", p.resources.loading, p.resources.problem, p.workspaceResourceSession())
}

func TestWorkspaceResourcesActualFilesTabsSwitchCloseAndRestore(t *testing.T) {
	p, session := resourcePageFixture(t)
	content := strings.Repeat("actual project content\n", 700)
	if err := os.WriteFile(filepath.Join(session.Workspace, "source.txt"), []byte(content), 0600); err != nil {
		t.Fatal(err)
	}
	tester := resourceTester(p, 420, 680)
	settleResource(t, p, tester, func() bool { s := p.workspaceResourceSession(); return s != nil && s.loaded })
	openWorkspaceFilesForTest(t, p, tester)
	settleResource(t, p, tester, func() bool { return !p.resources.loading && len(p.resources.page.Entries) == 1 })
	if err := tester.Click("Open resource source.txt"); err != nil {
		t.Fatal(err)
	}
	settleResource(t, p, tester, func() bool { return !p.resources.loading && p.resources.page.Body.Title != "" })
	if string(p.resources.page.Body.Data) != content[:product.WorkspaceResourcePageBytes] || len(p.workspaceResourceSession().view.Tabs) != 2 {
		t.Fatal("file click did not open actual independent tab")
	}
	if err := tester.Click("下一范围"); err != nil {
		t.Fatal(err)
	}
	settleResource(t, p, tester, func() bool {
		return !p.resources.loading && p.resources.page.Body.Offset == product.WorkspaceResourcePageBytes
	})
	viewport, ok := tester.Find("Resource contents")
	if !ok {
		t.Fatal("resource body has no independent viewport")
	}
	tester.Scroll(viewport.X+viewport.W/2, viewport.Y+viewport.H/2, 0, 128)
	p.observeWorkspaceResources()
	scrolled := p.workspaceResourceSession().scroll[p.workspaceActiveResource().ID].Y
	if scrolled <= 0 {
		t.Fatal("actual resource body did not scroll")
	}
	if err := tester.Click("Select resource 项目文件"); err != nil {
		t.Fatal(err)
	}
	settleResource(t, p, tester, func() bool { return !p.resources.loading && len(p.resources.page.Entries) == 1 })
	if err := tester.Click("Select resource source.txt"); err != nil {
		t.Fatal(err)
	}
	settleResource(t, p, tester, func() bool {
		return !p.resources.loading && p.resources.page.Body.Offset == product.WorkspaceResourcePageBytes
	})
	p.observeWorkspaceResources()
	settleResource(t, p, tester, func() bool { s := p.workspaceResourceSession(); return !s.saving && s.saved == s.changed })
	saved, err := p.Service.WorkspaceViews.Read(t.Context(), session.ID)
	if err != nil || len(saved.Tabs) != 2 || saved.Tabs[1].Offset != product.WorkspaceResourcePageBytes || saved.Tabs[1].ScrollY != scrolled {
		t.Fatal("positions were not durable", saved, err)
	}
	reopened := NewNativePage(p.Service)
	reopened.selected = session.ID
	reopened.detail = p.detail
	reopened.selectWorkspaceResources(session.ID)
	reopenedTester := resourceTester(reopened, 420, 680)
	settleResource(t, reopened, reopenedTester, func() bool {
		s := reopened.workspaceResourceSession()
		return s != nil && s.loaded && !reopened.resources.loading && reopened.resources.page.Body.Title != ""
	})
	if reopened.workspaceActiveResource().Offset != product.WorkspaceResourcePageBytes || reopened.workspaceResourceSession().scroll[reopened.workspaceActiveResource().ID].Y != scrolled {
		t.Fatal("new observer lost committed range or scroll position")
	}
	reopened.closeWorkspaceResources()
	other, err := p.Service.Create(t.Context(), "Other", "")
	if err != nil {
		t.Fatal(err)
	}
	p.observeWorkspaceResources()
	p.selected = other.ID
	p.selectWorkspaceResources(other.ID)
	settleResource(t, p, tester, func() bool { s := p.workspaceResourceSession(); return s != nil && s.loaded })
	if p.workspaceResourcesVisible() || len(p.resources.page.Body.Data) != 0 {
		t.Fatal("other session retained source contents or tabs")
	}
	p.selected = session.ID
	p.selectWorkspaceResources(session.ID)
	settleResource(t, p, tester, func() bool {
		return !p.resources.loading && p.resources.page.Body.Offset == product.WorkspaceResourcePageBytes
	})
	if err = tester.Click("Close resource source.txt"); err != nil {
		t.Fatal(err)
	}
	settleResource(t, p, tester, func() bool {
		return !p.resources.loading && p.workspaceActiveResource() != nil && p.workspaceActiveResource().Kind == "files"
	})
	if _, err = os.Stat(filepath.Join(session.Workspace, "source.txt")); err != nil {
		t.Fatal("closing source tab deleted original", err)
	}
	if p.Service.Runtime.HasActive() {
		t.Fatal("reading tabs executed")
	}
}

func TestWorkspaceResourcesClosingAndCollapsingNeverStopsExecution(t *testing.T) {
	p, session := resourcePageFixture(t)
	owner := &settingsRunningEngine{entered: make(chan struct{}), release: make(chan struct{})}
	if err := p.Service.Runtime.Register("alva", owner); err != nil {
		t.Fatal(err)
	}
	_, done, err := p.Service.Runtime.Start(t.Context(), session.ID, "Explicit local test execution")
	if err != nil {
		t.Fatal(err)
	}
	defer func() { close(owner.release); <-done }()
	<-owner.entered
	tester := resourceTester(p, 420, 680)
	settleResource(t, p, tester, func() bool { return p.workspaceResourceSession().loaded })
	openWorkspaceFilesForTest(t, p, tester)
	settleResource(t, p, tester, func() bool { return !p.resources.loading })
	if err = tester.Click("Collapse resource workspace"); err != nil {
		t.Fatal(err)
	}
	if p.workspaceResourcesVisible() || !p.Service.Runtime.HasActive() || owner.cancels.Load() != 0 {
		t.Fatal("collapse stopped execution")
	}
	openWorkspaceFilesForTest(t, p, tester)
	tester.Frame()
	p.closeWorkspaceResource(p.workspaceActiveResource().ID)
	p.closeWorkspaceResources()
	if p.workspaceResourcesVisible() || !p.Service.Runtime.HasActive() || owner.cancels.Load() != 0 {
		t.Fatal("closing tabs stopped execution")
	}
}

type delayedWorkspaceArtifactRepository struct {
	storage.Repository
	entered, release, canceled chan struct{}
}

func (r *delayedWorkspaceArtifactRepository) ReadArtifact(ctx context.Context, owner, id, uri string, offset int64, limit int) (storage.ArtifactPage, error) {
	page, err := r.Repository.ReadArtifact(ctx, owner, id, uri, offset, limit)
	close(r.entered)
	<-ctx.Done()
	close(r.canceled)
	<-r.release
	return page, err
}

func TestWorkspaceResourcesLateClosedReadCannotOverwriteAnotherTab(t *testing.T) {
	p, session := resourcePageFixture(t)
	attachment, err := p.Service.UploadReader(t.Context(), session.ID, "original.txt", strings.NewReader("late original must stay hidden"))
	if err != nil {
		t.Fatal(err)
	}
	delayed := &delayedWorkspaceArtifactRepository{Repository: p.Service.Repository, entered: make(chan struct{}), release: make(chan struct{}), canceled: make(chan struct{})}
	p.Service.Repository = delayed
	tester := resourceTester(p, 420, 680)
	settleResource(t, p, tester, func() bool { return p.workspaceResourceSession().loaded })
	p.openWorkspaceResource(hub.WorkspaceTab{Kind: "artifact", Title: "original.txt", BranchID: session.ActiveBranchID, URI: attachment.Reference.URI})
	select {
	case <-delayed.entered:
	case <-time.After(time.Second):
		t.Fatal("actual read did not start")
	}
	p.closeWorkspaceResource(p.workspaceActiveResource().ID)
	select {
	case <-delayed.canceled:
	case <-time.After(time.Second):
		t.Fatal("closing did not cancel actual read")
	}
	p.openWorkspaceResource(hub.WorkspaceTab{Kind: "files", Title: "Project", BranchID: session.ActiveBranchID})
	close(delayed.release)
	settleResource(t, p, tester, func() bool { return !p.resources.loading })
	if len(p.resources.page.Body.Data) != 0 || p.workspaceActiveResource().Kind != "files" || p.resources.problem != "" {
		t.Fatal("late body/read error overwrote replacement tab", p.resources.page, p.resources.problem)
	}
}

func TestWorkspaceResourcesNarrowLongTabsStayBoundedAndCanReorderFocusPopout(t *testing.T) {
	p, session := resourcePageFixture(t)
	tester := resourceTester(p, 420, 680)
	settleResource(t, p, tester, func() bool { return p.workspaceResourceSession().loaded })
	long := strings.Repeat("very-long-resource-", 24)
	p.openWorkspaceResource(hub.WorkspaceTab{Kind: "files", Title: long, BranchID: session.ActiveBranchID})
	p.openWorkspaceResource(hub.WorkspaceTab{Kind: "artifacts", Title: "Originals", BranchID: session.ActiveBranchID})
	settleResource(t, p, tester, func() bool {
		s := p.workspaceResourceSession()
		return !p.resources.loading && !s.saving && s.changed == s.saved
	})
	for _, label := range []string{"Open resource tab", "Collapse resource workspace", "Resource tabs"} {
		r, ok := tester.Find(label)
		if !ok || r.X < 0 || r.X+r.W > 420.5 || r.Y < 0 || r.Y+r.H > 680.5 {
			t.Fatalf("unbounded %s: %+v", label, r)
		}
	}
	if !p.workspaceResourcesFullWidth(960) || p.workspaceResourcesFullWidth(1240) {
		t.Fatal("narrow resources did not get bounded focus region")
	}
	if err := tester.Click("Open resource tab"); err != nil {
		t.Fatal(err)
	}
	if err := tester.ChooseMenuItem("专注资料"); err != nil {
		t.Fatal(err)
	}
	if !p.workspaceResourcesFullWidth(1240) {
		t.Fatal("explicit focus ignored")
	}
	p.moveWorkspaceResource(-1)
	if p.workspaceResourceSession().view.Tabs[0].Title != "Originals" {
		t.Fatal("tab reorder did not move active identity")
	}
	opened := ""
	p.openWindow = func(id string) error { opened = id; return nil }
	settleResource(t, p, tester, func() bool { s := p.workspaceResourceSession(); return !s.saving && s.changed == s.saved })
	if err := workspaceTestMenu(tester, "在新窗口打开资料"); err != nil {
		t.Fatal(err)
	}
	if opened != session.ID {
		t.Fatal("popout changed session identity")
	}
}
