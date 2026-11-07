// INPUT: Canonical ComposerMenu/General constraints and real MyGo headless layout.
// OUTPUT: Equal command panes, full single-line rows and flat bounded preference geometry regressions.
// POS: [L7 Hub UI tests] Presentation-only probes preserve keyboard focus, drafts and zero execution.
//go:build desktop

package hubdesktop

import (
	"fmt"
	"math"
	"os"
	"path/filepath"
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestWorkspacePickerSharedFoundationLayout(t *testing.T) {
	for _, tc := range []struct {
		kind          string
		width, height int
	}{{"commands", 1240, 850}, {"commands", 960, 680}, {"commands", 640, 680}, {"references", 1240, 850}, {"references", 960, 680}} {
		t.Run(fmt.Sprintf("%s-%d", tc.kind, tc.width), func(t *testing.T) {
			service, requests := fixture(t)
			p := NewNativePage(service)
			p.workspace = t.TempDir()
			for i := range 12 {
				if err := os.WriteFile(filepath.Join(p.workspace, fmt.Sprintf("notes-%02d.txt", i)), []byte("original"), 0600); err != nil {
					t.Fatal(err)
				}
			}
			tester := ui.NewTester(workspaceStaticView(p), tc.width, tc.height)
			if err := tester.Click("Message"); err != nil {
				t.Fatal(err)
			}
			if tc.width <= 960 {
				tester.Type("第一行\n第二行\n第三行\n第四行\n")
			}
			p.openWorkspacePicker(tc.kind)
			waitWorkspace(t, p, tester, func() bool { return !p.picker.loading && len(p.picker.rows) > 0 })
			panel, _ := tester.Find("Workspace picker")
			list, _ := tester.Find("Composer picker list")
			results, _ := tester.Find("Composer picker results")
			query, _ := tester.Find("Search composer items")
			wantWidth := float32(352)
			if tc.kind == "commands" {
				wantWidth = min(float32(640), float32(tc.width)*.95)
			}
			if panel.W != wantWidth || list.X != panel.X || results.X != list.X+8 || query.X != list.X+16 || query.W != list.W-32 || query.H != 18 || query.Y+query.H != list.Y+list.H-12 {
				t.Fatal("shared menu width, inset or footer changed", panel, list, results, query)
			}
			if tc.kind == "commands" {
				detail, found := tester.Find("Composer active item details")
				if tc.width == 640 {
					if !found || list.W != panel.W || detail.W != panel.W || detail.X != list.X || detail.Y < list.Y+list.H {
						t.Fatal("narrow detail lost stacked access", list, detail)
					}
				} else if !found || list.W != 316 || detail.W != 316 || detail.X-list.X-list.W != 8 || detail.Y != list.Y {
					t.Fatal("commands must use equal shared panes", list, detail)
				}
			} else if _, found := tester.Find("Composer active item details"); found || list.W != 352 {
				t.Fatal("References must retain its single shared list", list)
			}
			if math.Abs(float64(results.H/28)-math.Round(float64(results.H/28))) > .01 {
				t.Fatal("result viewport clips a row", results)
			}
			if tc.kind == "references" && tc.width == 960 && results.H < 3*37 {
				t.Fatal("short viewport lost full candidate rows", results)
			}
			for index, row := range p.picker.rows {
				if index >= 2 {
					break
				}
				label := ""
				if row.file != nil {
					label = "Select @" + row.file.DisplayPath
				}
				if row.command != nil {
					label = "Select /" + row.command.Name
				}
				item, found := tester.Find(label)
				icon, iconFound := tester.Find("Composer item icon " + row.identity())
				if !found || !iconFound || item.H != 28 || icon.W != 16 || icon.H != 16 || icon.X != item.X+8 {
					t.Fatal("row lost its single label or shared icon column", item, icon)
				}
			}
			tester.Key(0, ui.KeyTab)
			if !tester.Focused("Search composer items") {
				t.Fatal("footer search must follow Message in keyboard order")
			}
			tester.Key(ui.Shift, ui.KeyTab)
			if !tester.Focused("Message") {
				t.Fatal("reverse Tab did not restore Message")
			}
			if tc.kind == "references" {
				for range len(p.picker.rows) - 1 {
					tester.Key(0, ui.KeyDown)
				}
				last := p.picker.rows[len(p.picker.rows)-1]
				item, found := tester.Find("Select @" + last.file.DisplayPath)
				if !found || item.H != 28 || item.Y < results.Y || item.Y+item.H > results.Y+results.H+.1 {
					t.Fatal("keyboard cannot reveal the last bounded file candidate", item, results)
				}
			}
			before := p.model.Draft
			tester.Key(0, ui.KeyEscape)
			if p.picker.open || !tester.Focused("Message") || p.model.Draft != before || len(requests) != 0 || service.Runtime.HasActive() {
				t.Fatal("layout changed draft, focus or execution")
			}
		})
	}
}

func TestWorkspaceCommandsWithoutDetailsKeepFullWidth(t *testing.T) {
	for _, state := range []string{"empty", "loading", "error", "no-selection"} {
		t.Run(state, func(t *testing.T) {
			service, _ := fixture(t)
			p := NewNativePage(service)
			tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
			if err := tester.Click("Message"); err != nil {
				t.Fatal(err)
			}
			p.openWorkspacePicker("commands")
			waitWorkspace(t, p, tester, func() bool { return !p.picker.loading })
			p.picker.active = -1
			if state != "no-selection" {
				p.picker.rows = nil
			}
			p.picker.loading = state == "loading"
			if state == "error" {
				p.picker.problem = "搜索失败"
			}
			tester.Frame()
			panel, panelOK := tester.Find("Workspace picker")
			list, listOK := tester.Find("Composer picker list")
			_, detailOK := tester.Find("Composer active item details")
			if !panelOK || !listOK || detailOK || list.W != panel.W || list.X != panel.X {
				t.Fatal("a missing detail reserved an empty column", panel, list, detailOK)
			}
		})
	}
}

func TestPreferencesFlatFoundationLayout(t *testing.T) {
	for _, width := range []int{1440, 1240, 960} {
		t.Run(fmt.Sprint(width), func(t *testing.T) {
			service := preferenceFixture(t)
			p := NewNativePage(service)
			tester := ui.NewTester(workspaceStaticView(p), width, 850)
			tester.SetTitleBar(ui.TitleBar{Height: 52, Left: 86})
			openPreferencePage(t, p, tester)
			title, _ := tester.Find("General page title")
			appearance, _ := tester.Find("Preferences appearance")
			save, _ := tester.Find("Save preferences")
			motion, _ := tester.Find("Reduce application motion")
			wantWidth := min(float32(752), float32(width)-280)
			main, _ := tester.Find("Settings main surface")
			back, backOK := tester.Find("Back to workspace")
			// B3: root/nav gap8, header36 above the paper at y52, then the
			// existing body24, 36px title and 16px section spacing.
			if main != (ui.Rect{X: 224, Y: 52, W: float32(width - 232), H: 790}) || !backOK || back.X != 232 || back.Y != 10 || back.H != 32 ||
				title.X != 248 || title.Y != 76 || appearance.X != 248 || appearance.Y != 122 || appearance.W != wantWidth || appearance.H != 169 ||
				save.Y != 307 || save.X+save.W != title.X+title.W || motion.X != save.X+30 || motion.Y != 259 || motion.W != 38 || motion.H != 24 {
				t.Fatal("General no longer follows flat bounded saved rows", title, appearance, save, motion)
			}
		})
	}
}
