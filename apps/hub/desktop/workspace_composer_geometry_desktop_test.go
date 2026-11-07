// INPUT: Real composer typing, committed session Settings/reference search and native headless layout at two window sizes.
// OUTPUT: Above-card picker anchors, unshifted toolbar, compact symmetric insets and viewport regressions.
// POS: [L7 Hub UI tests] Checks visible geometry and editing ownership without an OS window or model execution.
//go:build desktop

package hubdesktop

import (
	"fmt"
	"math"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/egoist/mygo/ui"
)

func TestWorkspaceCombinedReferencesAndPendingDictationKeepControlsVisible(t *testing.T) {
	service, _ := fixture(t)
	p := NewNativePage(service)
	p.model.Draft = "第一行\n第二行\n第三行\n第四行\n第五行"
	p.voice.pending, p.voice.pendingSession = strings.Repeat("待采纳语音文字", 100), ""
	p.composerSelections = map[string][]product.ComposerPreparation{"": {}}
	for i := range 8 {
		p.composerSelections[""] = append(p.composerSelections[""], product.ComposerPreparation{EntryID: fmt.Sprintf("goal:%d", i), Kind: product.ComposerGoalEntry, Text: strings.Repeat("长目标名称", 30)})
		p.retainWorkspaceFiles("", []string{filepath.Join(t.TempDir(), fmt.Sprintf("file-%d.txt", i))})
	}
	tester := ui.NewTester(workspaceStaticView(p), 960, 680)
	send, ok := tester.Find("Send")
	message, messageOK := tester.Find("Message")
	refs, refsOK := tester.Find("Composer attachments")
	if !ok || !messageOK || !refsOK || send.H != 28 || send.Y+send.H > 680 || message.H != 112 || refs.H > 126 {
		t.Fatal("combined auxiliary states displaced the message or Send", send, message, refs)
	}
	apply, found := tester.Find("Apply dictation text")
	if !found || math.Abs(float64(apply.H-32)) > .02 || apply.Y < refs.Y || apply.Y+apply.H > refs.Y+refs.H+.02 {
		t.Fatal("pending dictation action must be complete before auxiliary scrolling", apply)
	}
	tester.Scroll(refs.X+refs.W/2, refs.Y+refs.H/2, 0, 32)
	if after, _ := tester.Find("Send"); after != send {
		t.Fatal("auxiliary scrolling moved Send", after, send)
	}
	if service.Runtime.HasActive() {
		t.Fatal("layout executed work")
	}
}

func TestWorkspacePickerStaysAboveComposerAndLeavesControlsReachable(t *testing.T) {
	for _, tc := range []struct {
		name, trigger, prefix string
		conversation          bool
		width, height         int
	}{
		{"home-short-command", "/", "", false, 1240, 850},
		{"home-multiline-reference", "@", "第一行\n第二行\n第三行\n第四行\n", false, 960, 680},
		{"conversation-short-reference", "@", "", true, 1240, 850},
		{"conversation-multiline-command", "/", "第一行\n第二行\n第三行\n第四行\n", true, 960, 680},
	} {
		t.Run(tc.name, func(t *testing.T) {
			service, requests := fixture(t)
			p := NewNativePage(service)
			if tc.conversation {
				session, err := service.Create(t.Context(), "composer geometry", "")
				if err != nil {
					t.Fatal(err)
				}
				if err = os.WriteFile(filepath.Join(session.Workspace, "notes.txt"), []byte("selected original"), 0600); err != nil {
					t.Fatal(err)
				}
				p.selected, p.detail = session.ID, mustPickerDetail(t, service, session.ID)
				p.applyDetail()
			}
			tester := ui.NewTester(workspaceStaticView(p), tc.width, tc.height)
			if tc.conversation {
				// Assigning Detail above does not load the independently observed
				// session model options. Finish their real Settings read before
				// measuring: picker query waits can poll and remove that notice.
				initial, _ := tester.Find("Task composer")
				pendingNotice, _ := tester.Find(p.workspaceSessionModelOptionsProblem())
				p.poll()
				waitWorkspace(t, p, tester, func() bool {
					return !p.polling && p.workspaceSessionModelOptionsProblem() == ""
				})
				loaded, _ := tester.Find("Task composer")
				t.Logf("initial Settings observation: composer %v -> %v; pending notice %v", initial, loaded, pendingNotice)
			}
			if err := tester.Click("Message"); err != nil {
				t.Fatal(err)
			}
			if tc.prefix != "" {
				tester.Type(tc.prefix)
			}
			before, _ := tester.Find("Task composer")
			controls, _ := tester.Find("Send")
			tester.Type(tc.trigger)
			waitWorkspace(t, p, tester, func() bool { return p.picker.open && !p.picker.loading })
			panel, ok := tester.Find("Workspace picker")
			composer, _ := tester.Find("Task composer")
			send, _ := tester.Find("Send")
			if !ok || panel.W == 0 || panel.H == 0 || panel.Y+panel.H > composer.Y-5.5 || math.Abs(float64(panel.X-composer.X)) > .5 {
				t.Fatalf("picker must align above the whole composer: panel=%v composer=%v", panel, composer)
			}
			if panel.X < 0 || panel.Y < 0 || panel.X+panel.W > float32(tc.width) || panel.Y+panel.H > float32(tc.height) {
				t.Fatalf("picker left viewport: %v", panel)
			}
			if before != composer || controls != send {
				t.Fatalf("floating picker moved composer or controls: %v -> %v; %v -> %v", before, composer, controls, send)
			}
			for _, label := range []string{"Add attachments", "Permission policy", "Agent and model", "Send"} {
				control, present := tester.Find(label)
				inset := composer.Y + composer.H - control.Y - control.H
				if !present || control.H != 28 || math.Abs(float64(inset-8)) > .5 {
					t.Fatalf("%s is not compact and bottom aligned: %v within %v (inset=%v)", label, control, composer, inset)
				}
			}
			if math.Abs(float64(composer.X+composer.W-send.X-send.W-8)) > .5 {
				t.Fatal("Send does not share the composer's symmetric inset", composer, send)
			}
			tester.Key(0, ui.KeyEscape)
			if p.picker.open || !tester.Focused("Message") || p.model.Draft != tc.prefix+tc.trigger || service.Runtime.HasActive() || len(requests) != 0 {
				t.Fatal("closing picker changed input, focus or execution")
			}
		})
	}
}

func TestWorkspaceToolbarMenusPreferAboveTheirTrigger(t *testing.T) {
	for _, label := range []string{"Add attachments", "Permission policy", "Agent and model"} {
		t.Run(label, func(t *testing.T) {
			service, requests := fixture(t)
			p := NewNativePage(service)
			p.model.Draft = "保留我的输入"
			tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
			if err := tester.Click(label); err != nil {
				t.Fatal(err)
			}
			trigger, _ := tester.Find(label)
			panel, present := tester.Find("Composer menu")
			if !present || math.Abs(float64(trigger.Y-panel.Y-panel.H-6)) > .5 || panel.Y < 4 || panel.X < 4 || panel.X+panel.W > 1236 {
				t.Fatal("menu is not attached above its trigger within viewport", panel, trigger)
			}
			tester.Key(0, ui.KeyEscape)
			if p.composer.addOpen || p.composer.permissionOpen || p.composer.targetOpen || p.model.Draft != "保留我的输入" || service.Runtime.HasActive() || len(requests) != 0 {
				t.Fatal("dismissal changed the draft or started execution")
			}
		})
	}
}

func TestWorkspaceComposerMenusMatchFoundationInsetsAndRows(t *testing.T) {
	for _, tc := range []struct {
		name, trigger string
		width         float32
		items         []string
	}{
		{"add", "Add attachments", 352, []string{"Attach file", "文件或路径…", "命令与 Skills…", "Plan mode", "New Goal"}},
		{"permission", "Permission policy", 240, []string{"每次询问", "自动审批", "完全访问"}},
	} {
		t.Run(tc.name, func(t *testing.T) {
			service, requests := fixture(t)
			p := NewNativePage(service)
			tester := ui.NewTester(func(c *ui.Context) {
				p.applyPreferences(c)
				ui.Column(c).FillWidth().FillHeight().Justify(ui.Center).Children(func() {
					anchor := ui.ButtonBase(c).Label(tc.trigger).Size(32, 32).OnClick(func() {
						if tc.name == "add" {
							p.composer.addOpen = !p.composer.addOpen
						} else {
							p.composer.permissionOpen = !p.composer.permissionOpen
						}
					})
					if tc.name == "add" {
						p.buildWorkspaceAttachmentMenu(c, anchor)
					} else {
						p.buildNativePermissionMenu(c, anchor)
					}
				})
			}, 1240, 850)
			if err := tester.Click(tc.trigger); err != nil {
				t.Fatal(err)
			}
			panel, present := tester.Find("Composer menu")
			gap := 4
			if tc.name == "add" {
				gap = 0
			}
			wantHeight := float32(16 + len(tc.items)*28 + (len(tc.items)-1)*gap)
			if !present || panel.W != tc.width || panel.H != wantHeight {
				t.Fatal("menu does not match Foundation width and natural single-line height", panel, tc.width, wantHeight)
			}
			for i, label := range tc.items {
				item, found := tester.Find(label)
				if !found || item.H != 28 || item.W != panel.W-16 || item.X != panel.X+8 || item.Y != panel.Y+8+float32(i*(28+gap)) {
					t.Fatal("menu row lost Foundation padding or spacing", label, item, panel)
				}
			}
			if tc.name == "add" {
				for _, label := range []string{"文件、图片或视频…", "计划模式", "新建或选择目标"} {
					text, found := tester.Find(label)
					if !found || text.X != panel.X+40 || text.H != 19.5 || math.Abs(float64(text.Y-panel.Y-12.25)-math.Round(float64(text.Y-panel.Y-12.25)/28)*28) > .5 {
						t.Fatal("Add labels must follow the centered 20px icon and 8px gap", label, text, panel)
					}
				}
			}
			if service.Runtime.HasActive() || len(requests) != 0 {
				t.Fatal("opening presentation-only menus started execution")
			}
		})
	}
}

func TestWorkspacePermissionMenuSelectedColorsMatchFoundation(t *testing.T) {
	for _, dark := range []bool{false, true} {
		t.Run(map[bool]string{false: "light", true: "dark"}[dark], func(t *testing.T) {
			service, _ := fixture(t)
			p := NewNativePage(service)
			p.composer.permissionOpen = true
			var accent, text, selection ui.Color
			tester := ui.NewTester(func(c *ui.Context) {
				p.applyPreferences(c)
				ui.Column(c).FillWidth().FillHeight().Justify(ui.Center).Children(func() {
					p.buildNativePermissionMenu(c, ui.Box(c).Size(32, 32))
				})
				accent, text, selection = c.Theme().Accent, c.Theme().Text, workspacePalette(c).selection
			}, 1240, 850)
			tester.SetDark(dark)
			image := tester.Image()
			for _, tc := range []struct {
				label string
				color ui.Color
			}{
				{"每次询问", accent},
				{"自动审批", text},
			} {
				item, _ := tester.Find(tc.label)
				pixels := 0
				for y := int(item.Y + 8); y < int(item.Y+item.H-8); y++ {
					for x := int(item.X + 36); x < int(item.X+110); x++ {
						pixel := image.RGBAAt(x, y)
						if pixel.R == tc.color.R && pixel.G == tc.color.G && pixel.B == tc.color.B {
							pixels++
						}
					}
				}
				if pixels < 5 {
					t.Fatal("menu label did not retain its explicit selected/default text color", tc.label, tc.color, pixels)
				}
			}
			selected, _ := tester.Find("每次询问")
			pixel := image.RGBAAt(int(selected.X+2), int(selected.Y+selected.H/2))
			if pixel.R != selection.R || pixel.G != selection.G || pixel.B != selection.B {
				t.Fatal("selected permission lost its distinct shallow background", pixel, selection)
			}
		})
	}
}

func TestWorkspacePickerRetainsEditorTabOrderAsCardChanges(t *testing.T) {
	service, _ := fixture(t)
	p := NewNativePage(service)
	tester := ui.NewTester(workspaceStaticView(p), 960, 680)
	if err := tester.Click("Message"); err != nil {
		t.Fatal(err)
	}
	p.openWorkspacePicker("commands")
	waitWorkspace(t, p, tester, func() bool { return p.picker.open && !p.picker.loading })
	for _, tall := range []bool{false, true, false} {
		if tall {
			p.retainWorkspaceFiles("", []string{"/selected-but-not-read.txt"})
			p.model.Draft = "第一行\n第二行\n第三行\n第四行\n第五行"
		} else {
			delete(p.workspaceFiles, "")
			p.model.Draft = "短输入"
		}
		tester.Frame()
		panel, present := tester.Find("Workspace picker")
		card, _ := tester.Find("Task composer")
		if !present || math.Abs(float64(card.Y-panel.Y-panel.H-6)) > .5 || math.Abs(float64(card.X-panel.X)) > .5 {
			t.Fatal("picker did not settle with changed chips/textarea height", panel, card)
		}
		tester.Key(0, ui.KeyTab)
		if !tester.Focused("Search composer items") {
			t.Fatal("Tab skipped the popup to unrelated page controls")
		}
		tester.Key(ui.Shift, ui.KeyTab)
		if !tester.Focused("Message") {
			t.Fatal("reverse Tab lost the input anchor")
		}
	}
}

func TestWorkspacePickerFallsBelowWholeCardWhenTopHasNoRoom(t *testing.T) {
	service, _ := fixture(t)
	p := NewNativePage(service)
	tester := composerTester(p, 540)
	p.openWorkspacePicker("commands")
	// This isolated composer host has no NativePage.Build queue drain.
	deadline := time.After(5 * time.Second)
	for p.picker.loading {
		select {
		case apply := <-p.updates:
			apply()
		case <-deadline:
			t.Fatal("composer query did not return")
		}
	}
	tester.Frame()
	panel, present := tester.Find("Workspace picker")
	card, _ := tester.Find("Task composer")
	if !present || math.Abs(float64(panel.Y-card.Y-card.H-6)) > .5 || panel.Y+panel.H > 516 {
		t.Fatal("fallback placement overlaps toolbar or leaves viewport", panel, card)
	}
}
