// INPUT: Real composer typing, saved-session reference search and native headless layout at two window sizes.
// OUTPUT: Above-card picker anchors, unshifted toolbar, compact symmetric insets and viewport regressions.
// POS: [L7 Hub UI tests] Checks visible geometry and editing ownership without an OS window or model execution.
//go:build desktop

package hubdesktop

import (
	"math"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
)

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
			tester := ui.NewTester(p.Build, tc.width, tc.height)
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
				if !present || control.H != 32 || math.Abs(float64(inset-9)) > .5 {
					t.Fatalf("%s is not compact and bottom aligned: %v within %v (inset=%v)", label, control, composer, inset)
				}
			}
			if math.Abs(float64(composer.X+composer.W-send.X-send.W-9)) > .5 {
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
			tester := ui.NewTester(p.Build, 1240, 850)
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

func TestWorkspacePickerRetainsEditorTabOrderAsCardChanges(t *testing.T) {
	service, _ := fixture(t)
	p := NewNativePage(service)
	tester := ui.NewTester(p.Build, 960, 680)
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
