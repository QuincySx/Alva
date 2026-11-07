// INPUT: Real product SQLite, native composer controls and captured typed submission selections.
// OUTPUT: Pre-Create invalid-home rejection, cold intent, failed admission retention and late-ack draft isolation regressions.
// POS: [L7 Hub UI tests] Uses no external engine, OS window or invented successful execution.
//go:build desktop

package hubdesktop

import (
	"fmt"
	"math"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/egoist/mygo/ui"
)

func TestWorkspacePreparedHomePlanOnExternalFakeRouteCreatesNoSession(t *testing.T) {
	service, _ := fixture(t)
	p := NewNativePage(service)
	plan, err := service.PrepareComposerSelection(t.Context(), "", product.ComposerSelectionRequest{EntryID: "builtin:plan"})
	if err != nil {
		t.Fatal(err)
	}
	foreign := service.DefaultTarget
	foreign.EngineID = "codex"
	service.Targets = append(service.Targets, foreign)
	p.composer.target = foreign
	p.composerSelections = map[string][]product.ComposerPreparation{"": {plan}}
	p.model.Draft = "Keep this plan request"
	if p.workspaceTargetProblem() == "" {
		t.Fatal("known external Plan intent did not block Send")
	}
	p.sendWorkspaceDraft()
	rows, err := service.Store.ListSessions(t.Context(), 8)
	if err != nil || len(rows) != 0 || p.busy || p.selected != "" || p.model.Draft != "Keep this plan request" || len(p.composerSelections[""]) != 1 || service.Runtime.HasActive() {
		t.Fatal("known incompatible Plan created/switched a session or lost its draft", rows, err)
	}
	if err = validateWorkspaceHomeSelections(t.Context(), service, "", foreign, []product.ComposerPreparation{plan}); err == nil {
		t.Fatal("frozen pre-Create validation accepted an external Plan")
	}
}

func TestWorkspacePreparedHomeChangedBuiltinRejectsBeforeSessionCreate(t *testing.T) {
	service, _ := fixture(t)
	p := NewNativePage(service)
	prepared, err := service.PrepareComposerSelection(t.Context(), "", product.ComposerSelectionRequest{EntryID: "builtin:review"})
	if err != nil {
		t.Fatal(err)
	}
	prepared.Text = "altered typed preparation"
	p.composerSelections = map[string][]product.ComposerPreparation{"": {prepared}}
	p.model.Draft = "Keep this original draft"
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	p.sendWorkspaceDraft()
	waitWorkspace(t, p, tester, func() bool { return !p.busy && p.problem != "" })
	rows, err := service.Store.ListSessions(t.Context(), 8)
	if err != nil || len(rows) != 0 || p.selected != "" || p.model.Draft != "Keep this original draft" || len(p.composerSelections[""]) != 1 || service.Runtime.HasActive() {
		t.Fatal("stale typed preparation created a partial session", rows, err)
	}
	fresh, err := service.PrepareComposerSelection(t.Context(), "", product.ComposerSelectionRequest{EntryID: "builtin:review"})
	if err != nil {
		t.Fatal(err)
	}
	if err = validateWorkspaceHomeSelections(t.Context(), service, "", service.DefaultTarget, []product.ComposerPreparation{fresh}); err != nil {
		t.Fatal(err)
	}
}

func TestWorkspacePreparedHomeFailureRetainsCommittedIdentityAndSelection(t *testing.T) {
	service, _ := fixture(t)
	if err := service.Runtime.Close(t.Context()); err != nil {
		t.Fatal(err)
	} // Real admission rejection.
	p := NewNativePage(service)
	prepared, err := service.PrepareComposerSelection(t.Context(), "", product.ComposerSelectionRequest{EntryID: "builtin:plan"})
	if err != nil {
		t.Fatal(err)
	}
	p.composerSelections = map[string][]product.ComposerPreparation{"": {prepared}}
	p.model.Draft = prepared.Text
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	rows, err := service.Store.ListSessions(t.Context(), 8)
	if err != nil || len(rows) != 0 || service.Runtime.HasActive() {
		t.Fatal("preparing command created a session", err)
	}
	if err = tester.Click("Send"); err != nil {
		t.Fatal(err)
	}
	waitWorkspace(t, p, tester, func() bool { return p.selected != "" && p.problem != "" && !p.busy && !p.polling })
	rows, err = service.Store.ListSessions(t.Context(), 8)
	settings, settingsErr := service.Store.Settings(t.Context(), p.selected)
	if err != nil || settingsErr != nil || len(rows) != 1 || !settings.PlanMode || p.model.Draft != prepared.Text || len(p.composerSelections[p.selected]) != 1 || service.Runtime.HasActive() {
		t.Fatal("failed actual admission lost identity, typed intent or prompt", rows, settings, err, settingsErr)
	}
}

func TestWorkspacePreparedAcknowledgmentPreservesReplacementSelection(t *testing.T) {
	service, _ := fixture(t)
	p := NewNativePage(service)
	old, err := service.PrepareComposerSelection(t.Context(), "", product.ComposerSelectionRequest{EntryID: "builtin:review"})
	if err != nil {
		t.Fatal(err)
	}
	p.composerSelections = map[string][]product.ComposerPreparation{"": {old}}
	frozen := p.captureWorkspaceSelections()
	newer := old
	newer.Arguments = map[string]string{"request": "new task"}
	newer.Text += "new task"
	p.composerSelections[""] = []product.ComposerPreparation{newer}
	p.consumeWorkspaceSelections("", frozen)
	if len(p.composerSelections[""]) != 1 || p.composerSelections[""][0].Text != newer.Text {
		t.Fatal("old acknowledgement consumed the replacement draft selection")
	}
	p.consumeWorkspaceSelections("", p.captureWorkspaceSelections())
	if len(p.composerSelections[""]) != 0 {
		t.Fatal("acknowledged selection retained")
	}
}

func TestWorkspacePreparedReferencesUseStaticFoundationBadges(t *testing.T) {
	for _, dark := range []bool{false, true} {
		t.Run(map[bool]string{false: "light", true: "dark"}[dark], func(t *testing.T) {
			service, requests := fixture(t)
			p := NewNativePage(service)
			goal := "目标引用：" + strings.Repeat("保留用户选择的目标", 30)
			p.composerSelections = map[string][]product.ComposerPreparation{"": {
				{EntryID: "builtin:review", Kind: product.ComposerCommandEntry},
				{EntryID: "goal:real-draft", Kind: product.ComposerGoalEntry, Text: goal},
			}}
			p.model.Draft = "保持正文草稿"
			var subtle ui.Color
			tester := ui.NewTester(func(c *ui.Context) {
				c.DisableAnimations()
				p.Build(c)
				subtle = workspacePalette(c).subtle
			}, 720, 680)
			tester.SetDark(dark)
			composer, _ := tester.Find("Task composer")
			for _, tc := range []struct{ id, label string }{{"builtin:review", "/review"}, {"goal:real-draft", goal}} {
				badge, present := tester.Find("Prepared reference badge " + tc.id)
				label, _ := tester.Find(tc.label)
				remove, _ := tester.Find("Remove prepared reference " + tc.label)
				if !present || badge.H != 24.5 || label.H != 16.5 || remove.W != 28 || remove.H != 28 || badge.X < composer.X || remove.X+remove.W > composer.X+composer.W {
					t.Fatal("prepared reference lost Badge/IconButton geometry or expanded the composer", badge, label, remove, composer)
				}
				if math.Abs(float64(label.Y+label.H/2-badge.Y-badge.H/2)) > .5 || math.Abs(float64(remove.Y+remove.H/2-badge.Y-badge.H/2)) > .5 {
					t.Fatal("Badge label and remove action must share their vertical center", badge, label, remove)
				}
				pixel := tester.Image().RGBAAt(int(badge.X+4), int(badge.Y+badge.H/2))
				if pixel.R != subtle.R || pixel.G != subtle.G || pixel.B != subtle.B {
					t.Fatal("Neutral Badge did not use the canonical subtle surface", pixel, subtle)
				}
			}
			if tester.HasText("/goal:") {
				t.Fatal("Goal marker exposed an internal command ID")
			}
			p.busy = true
			tester.Frame()
			if err := tester.Click("Remove prepared reference /review"); err != nil || len(p.composerSelections[""]) != 2 {
				t.Fatal("busy composer allowed reference removal", err)
			}
			p.busy = false
			tester.Frame()
			if err := tester.Click("Remove prepared reference /review"); err != nil || len(p.composerSelections[""]) != 1 || p.composerSelections[""][0].Text != goal || p.model.Draft != "保持正文草稿" {
				t.Fatal("Badge removal changed another reference or the message draft", err)
			}
			if service.Runtime.HasActive() || len(requests) != 0 {
				t.Fatal("displaying/removing a prepared reference executed work")
			}
		})
	}
}

func TestWorkspaceMaximumPreparedAndFileReferencesShareScrollBudget(t *testing.T) {
	for _, lines := range []int{2, 5} {
		t.Run(fmt.Sprintf("%d-message-lines", lines), func(t *testing.T) {
			service, requests := fixture(t)
			p := NewNativePage(service)
			p.model.Draft = strings.Repeat("保留正文\n", lines-1) + "继续编辑"
			p.composerSelections = map[string][]product.ComposerPreparation{"": {}}
			for i := 0; i < 8; i++ {
				p.composerSelections[""] = append(p.composerSelections[""], product.ComposerPreparation{
					EntryID: fmt.Sprintf("goal:reference-%d", i), Kind: product.ComposerGoalEntry,
					Text: fmt.Sprintf("目标引用%d：「%s」", i, strings.Repeat("a", 256)),
				})
				p.retainWorkspaceFiles("", []string{filepath.Join(t.TempDir(), fmt.Sprintf("file-%d.txt", i))})
			}
			draft := p.model.Draft
			tester := ui.NewTester(workspaceStaticView(p), 960, 680)
			viewport, _ := tester.Find("Composer attachments")
			send, sendPresent := tester.Find("Send")
			message, messagePresent := tester.Find("Message")
			badge, _ := tester.Find("Prepared reference badge goal:reference-0")
			if viewport.H != 126 || !sendPresent || send.H != 28 || send.Y+send.H > 680 || !messagePresent || math.Abs(float64(message.H-float32(lines)*22.4)) > .05 || message.Y < viewport.Y+viewport.H || message.Y+message.H > send.Y || badge.H != 24.5 {
				t.Fatal("combined references displaced or clipped the message/toolbar or changed Badge geometry", viewport, message, send, badge)
			}
			// The final Goal remains reachable in the shared viewport before the
			// attachment rows; its independent Remove still owns only that draft.
			tester.Scroll(viewport.X+viewport.W/2, viewport.Y+viewport.H/2, 0, 240)
			lastGoal := p.composerSelections[""][7].Text
			goalRemove, goalPresent := tester.Find("Remove prepared reference " + lastGoal)
			if !goalPresent || goalRemove.H != 28 || goalRemove.Y < viewport.Y || goalRemove.Y+goalRemove.H > viewport.Y+viewport.H {
				t.Fatal("shared scrolling did not expose the final prepared reference Remove action", viewport, goalRemove)
			}
			if err := tester.Click("Remove prepared reference " + lastGoal); err != nil || len(p.composerSelections[""]) != 7 {
				t.Fatal("scrolled Goal Remove did not remove its own draft", err)
			}
			tester.Scroll(viewport.X+viewport.W/2, viewport.Y+viewport.H/2, 0, 1000)
			fileRemove, filePresent := tester.Find("Remove pending file file-7.txt")
			lastCard, _ := tester.Find("Attachment file-7.txt")
			afterSend, _ := tester.Find("Send")
			afterMessage, _ := tester.Find("Message")
			if !filePresent || fileRemove.H != 28 || lastCard.H != 56 || fileRemove.Y < viewport.Y || fileRemove.Y+fileRemove.H > viewport.Y+viewport.H || send != afterSend || message != afterMessage {
				t.Fatal("final attachment Remove was unreachable or scrolling displaced the composer", viewport, fileRemove, lastCard, afterSend, afterMessage)
			}
			if err := tester.Click("Remove pending file file-7.txt"); err != nil || len(p.workspaceFiles[""]) != 7 || p.model.Draft != draft {
				t.Fatal("scrolled file removal changed another draft or message", err)
			}
			if service.Runtime.HasActive() || len(requests) != 0 || len(service.DraftAttachments("")) != 0 {
				t.Fatal("reference layout or removal imported paths or executed work")
			}
		})
	}
}
