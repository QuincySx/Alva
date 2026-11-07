// INPUT: Real SQLite workspace views and MyGo native-control geometry.
// OUTPUT: Directional left motion, fixed right controls and preserved draft/view ownership regressions.
// POS: [L7 Hub UI tests] No system window, model request or user-file read.
//go:build desktop

package hubdesktop

import (
	"crypto/sha256"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
)

func TestWorkspacePaneMotionSettlesAndHonorsReduction(t *testing.T) {
	for _, reduced := range []bool{false, true} {
		t.Run(map[bool]string{false: "animated", true: "reduced"}[reduced], func(t *testing.T) {
			p := NewNativePage(preferenceFixture(t))
			// Motion is exercised through the real native renderer, without the
			// settled-layout adapter used by the geometry/business tests.
			tester := ui.NewTester(p.Build, 1240, 850)
			tester.SetPreferences(ui.Preferences{ReduceMotion: reduced})
			tester.SetTitleBar(ui.TitleBar{Height: 52, Left: 86})
			tester.Advance(500 * time.Millisecond)
			find := func(label string) ui.Rect {
				t.Helper()
				r, ok := tester.Find(label)
				if !ok {
					t.Fatal("missing animated target", label)
				}
				return r
			}
			leftBefore := find("Collapse session navigation")
			mainBefore := find("Workspace main surface")
			titleBefore := find("Workspace title 新任务")
			if err := tester.Click("Collapse session navigation"); err != nil {
				t.Fatal(err)
			}
			start := sha256.Sum256(tester.Image().Pix)
			if !reduced && (find("Expand session navigation") != leftBefore || find("Workspace main surface") != mainBefore || find("Workspace title 新任务") != titleBefore) {
				t.Fatal("left transition jumped before moving")
			}
			tester.Advance(70 * time.Millisecond)
			middle := sha256.Sum256(tester.Image().Pix)
			leftMiddle := find("Expand session navigation")
			mainMiddle := find("Workspace main surface")
			titleMiddle := find("Workspace title 新任务")
			tester.Advance(500 * time.Millisecond)
			leftAfter := find("Expand session navigation")
			mainAfter := find("Workspace main surface")
			titleAfter := find("Workspace title 新任务")
			if leftBefore.X != 192 || leftAfter.X != 94 || mainAfter.X != workspaceWindowInset || titleAfter.X < leftAfter.X+leftAfter.W+8 {
				t.Fatal("wrong left endpoints or title clearance", leftBefore, leftAfter, mainAfter, titleAfter)
			}
			if !reduced && !(leftBefore.X > leftMiddle.X && leftMiddle.X > leftAfter.X &&
				mainBefore.X > mainMiddle.X && mainMiddle.X > mainAfter.X &&
				titleBefore.X > titleMiddle.X && titleMiddle.X > titleAfter.X) {
				t.Fatal("collapse moved the control, main frame or title in the wrong direction", leftBefore, leftMiddle, leftAfter, mainBefore, mainMiddle, mainAfter, titleBefore, titleMiddle, titleAfter)
			}
			end := sha256.Sum256(tester.Image().Pix)
			if reduced && (start != middle || middle != end) {
				t.Fatal("reduced motion continued changing the pane")
			}
			if !reduced && (start == middle || middle == end) {
				t.Fatal("pane did not pass through a bounded intermediate frame")
			}
			if tester.FramePending() || !p.layout.sidebarCollapsed {
				t.Fatal("settled pane kept requesting animation frames", tester.FramePending(), p.layout.sidebarCollapsed)
			}
			if err := tester.Click("Expand session navigation"); err != nil {
				t.Fatal(err)
			}
			tester.Advance(70 * time.Millisecond)
			if err := tester.Click("Collapse session navigation"); err != nil {
				t.Fatal("toggle lost its hit target during a reversed transition", err)
			}
			tester.Advance(500 * time.Millisecond)
			if tester.FramePending() || !p.layout.sidebarCollapsed || p.Service.Runtime.HasActive() {
				t.Fatal("reversed motion failed to settle or started execution", tester.FramePending(), p.layout.sidebarCollapsed)
			}
			// The right control never belongs to either animated surface.
			// It must stay fixed on every opening/closing frame, not just endpoints.
			right := find("Open resource workspace")
			for _, action := range []string{"Open resource workspace", "Collapse resource workspace"} {
				before := find("Workspace main surface")
				if err := tester.Click(action); err != nil {
					t.Fatal(err)
				}
				label := "Collapse resource workspace"
				if action == label {
					label = "Open resource workspace"
				}
				for _, delay := range []time.Duration{0, 40 * time.Millisecond, 40 * time.Millisecond, 80 * time.Millisecond, 500 * time.Millisecond} {
					tester.Advance(delay)
					if actual := find(label); actual != right {
						t.Fatal("right toggle moved during pane transition", action, delay, right, actual)
					}
					if action == "Open resource workspace" && delay > 0 {
						panel := find("Resource workspace")
						main := find("Workspace main surface")
						gap := panel.X - main.X - main.W
						if panel.X+panel.W != 1232 || gap < -0.01 || gap > workspaceRegionGap+0.01 {
							t.Fatal("right pane crossed the main surface or lost its fixed outer edge", panel, main, gap)
						}
					}
				}
				after := find("Workspace main surface")
				if action == "Open resource workspace" && after.W >= before.W || action == "Collapse resource workspace" && after.W <= before.W {
					t.Fatal("right pane did not resize the main frame", action, before, after)
				}
				_, more := tester.Find("More actions")
				if more != (action == "Open resource workspace") || tester.FramePending() {
					t.Fatal("right actions or animation did not settle", action, more, tester.FramePending())
				}
			}
		})
	}
}

func TestWorkspaceIndependentPaneControlsKeepPlacementAndDraft(t *testing.T) {
	p, session := resourcePageFixture(t)
	p.model.Draft = "保留这个草稿"
	tester := ui.NewTester(workspaceStaticView(p), 1440, 850)
	tester.SetTitleBar(ui.TitleBar{Height: 52, Left: 86})
	settleResource(t, p, tester, func() bool { return p.workspaceResourceSession().loaded })
	left, _ := tester.Find("Collapse session navigation")
	right, _ := tester.Find("Open resource workspace")
	if _, shown := tester.Find("More actions"); shown || p.workspaceResourcesVisible() {
		t.Fatal("uninitialized resource view opened without an explicit request")
	}
	connectionClick(t, tester, "Open resource workspace")
	main, _ := tester.Find("Workspace main surface")
	panel, panelOK := tester.Find("Resource workspace")
	header, _ := tester.Find("Workspace header")
	bar, _ := tester.Find("Resource header")
	closeRight, _ := tester.Find("Collapse resource workspace")
	resourceSurface, surfaceOK := tester.Find("Resource surface")
	if !panelOK || !surfaceOK || panel.X != main.X+main.W+workspaceRegionGap || panel.Y != b3Top(ui.TitleBar{Height: 52, Left: 86}) || bar.H != 36 ||
		bar.Y+bar.H/2 != header.Y+header.H/2 || closeRight != right {
		t.Fatal("right pane did not shrink the main frame or retain the opener anchor", main, panel, header, bar, right, closeRight)
	}
	if _, shown := tester.Find("More actions"); !shown {
		t.Fatal("opened resource tabbar has no reachable actions")
	}
	sidebar, _ := tester.Find("Session navigation")
	pixels := tester.Image()
	canvas := pixels.RGBAAt(0, 0)
	for _, box := range []ui.Rect{sidebar, main, resourceSurface} {
		if pixels.RGBAAt(int(box.X+1), int(box.Y+1)) != canvas || pixels.RGBAAt(int(box.X+16), int(box.Y+24)) == canvas {
			t.Fatal("one of the three panes lost its inset rounded surface", box)
		}
	}
	if len(p.workspaceResourceSession().view.Tabs) != 0 || p.resources.loading {
		t.Fatal("opening an empty view created a tab or started a content read")
	}
	connectionClick(t, tester, "Collapse session navigation")
	showLeft, _ := tester.Find("Expand session navigation")
	if left.X != 192 || showLeft.X != 94 || showLeft.Y != left.Y || !p.workspaceResourcesVisible() {
		t.Fatal("left toggle did not move from the sidebar corner to the native-safe anchor", left, showLeft)
	}
	connectionClick(t, tester, "Collapse resource workspace")
	showRight, _ := tester.Find("Open resource workspace")
	if showRight != right || !p.layout.sidebarCollapsed {
		t.Fatal("right toggle moved or changed the left pane", right, showRight)
	}
	connectionClick(t, tester, "Expand session navigation")
	settleResource(t, p, tester, func() bool { return !p.workspaceResourceSession().saving })
	saved, err := p.Service.WorkspaceViews.Read(t.Context(), session.ID)
	if err != nil || !saved.Collapsed || len(saved.Tabs) != 0 || p.model.Draft != "保留这个草稿" || p.selected != session.ID || p.Service.Runtime.HasActive() {
		t.Fatal("view toggles changed draft, session, persisted tabs or execution", saved, err)
	}
	// At minimum width, keep the same top-right return while preserving the
	// existing resource focus policy and the independent left navigation.
	tester.SetSize(960, 850)
	right, _ = tester.Find("Open resource workspace")
	connectionClick(t, tester, "Open resource workspace")
	closeRight, _ = tester.Find("Collapse resource workspace")
	if closeRight != right || !p.workspaceResourcesFullWidth(960) {
		t.Fatal("focused resources lost the stationary return", right, closeRight)
	}
	connectionClick(t, tester, "Collapse session navigation")
	composer, shown := tester.Find("Task composer")
	if !shown || composer.W < workspaceConversationMinWidth || p.workspaceResourcesFullWidth(960) {
		t.Fatal("hiding the left navigation did not restore a usable split", composer)
	}
	p.setPane("handoffs")
	tester.Frame()
	if p.workspaceResourcesVisible() {
		t.Fatal("explicit advanced navigation did not expose its destination")
	}
	p.openWorkspacePanel()
	tester.Frame()
	if p.pane != "handoffs" || !p.workspaceResourcesVisible() || p.model.Draft != "保留这个草稿" {
		t.Fatal("opening only the right pane changed advanced navigation or draft")
	}
}

func TestWorkspaceHomeRightPaneIsViewOnly(t *testing.T) {
	service, requests := fixture(t)
	p := NewNativePage(service)
	p.model.Draft = "尚未发送"
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	connectionClick(t, tester, "Open resource workspace")
	if _, shown := tester.Find("Resource empty state"); !shown || p.pane != "" || p.selected != "" || !tester.HasText("选择任务后查看资料") {
		t.Fatal("home opener navigated to task selection instead of opening the right pane")
	}
	if _, shown := tester.Find("More actions"); !shown {
		t.Fatal("view-only home pane lost the application actions")
	}
	connectionClick(t, tester, "Collapse resource workspace")
	if _, shown := tester.Find("More actions"); shown {
		t.Fatal("closed right pane left actions in the main header")
	}
	rows, err := service.Store.ListSessions(t.Context(), 128)
	if err != nil || len(rows) != 0 || len(requests) != 0 || service.Runtime.HasActive() || p.model.Draft != "尚未发送" {
		t.Fatal("home view toggle created a session, executed or changed the draft", rows, err)
	}
}
