// INPUT: Real Hub service fixture, MyGo headless text input with IME composition and the composer's key owner
// OUTPUT: Enter sends, Shift+Enter breaks the line, and Enter during IME composition or on blocked/blank drafts sends nothing
// POS: [L7 Hub UI] Keys reach the same admission as the Send button; no window, engine process or model request
//go:build desktop

package hubdesktop

import (
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestWorkspaceEnterSendsAndShiftEnterBreaksTheLine(t *testing.T) {
	service, _ := fixture(t)
	p := NewNativePage(service)
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	if err := tester.Click("Message"); err != nil {
		t.Fatal(err)
	}
	tester.Type("第一行")
	tester.Key(ui.Shift, ui.KeyEnter)
	tester.Type("第二行")
	if p.model.Draft != "第一行\n第二行" || p.busy || p.selected != "" {
		t.Fatal("Shift+Enter must only break the line", p.model.Draft, p.busy)
	}
	tester.Key(0, ui.KeyEnter)
	waitWorkspace(t, p, tester, func() bool { return p.selected != "" && !p.busy && p.pendingFirstSend == "" })
	rows, err := service.Store.ListSessions(t.Context(), 8)
	if err != nil || len(rows) != 1 || rows[0].Title != "第一行" {
		t.Fatal("Enter did not send once with the shared first-line title", rows, err)
	}
}

func TestWorkspaceEnterDuringIMECompositionNeverSends(t *testing.T) {
	service, requests := fixture(t)
	p := NewNativePage(service)
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	if err := tester.Click("Message"); err != nil {
		t.Fatal(err)
	}
	tester.Type("已有文字")
	tester.Compose("nihao", 5) // Pinyin pre-edit: Enter here chooses a candidate
	tester.Key(0, ui.KeyEnter)
	tester.Frame()
	if p.busy || p.selected != "" || service.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("Enter that picked an IME candidate sent the message")
	}
	rows, err := service.Store.ListSessions(t.Context(), 8)
	if err != nil || len(rows) != 0 {
		t.Fatal("IME Enter created a session", rows, err)
	}
	tester.Compose("", 0)
	tester.Type("你好")
	tester.Key(0, ui.KeyEnter) // composition ended: now it sends
	waitWorkspace(t, p, tester, func() bool { return p.selected != "" && !p.busy && p.pendingFirstSend == "" })
}

func TestWorkspaceEnterOnBlankOrBlockedDraftSendsNothingAndKeepsDraft(t *testing.T) {
	service, requests := fixture(t)
	p := NewNativePage(service)
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	if err := tester.Click("Message"); err != nil {
		t.Fatal(err)
	}
	tester.Key(0, ui.KeyEnter)
	tester.Type("   ")
	tester.Key(0, ui.KeyEnter)
	if p.busy || p.selected != "" || len(requests) != 0 {
		t.Fatal("Enter on a blank draft started work")
	}
	p.model.ActiveRunID = "observed-run" // a running task blocks sending
	tester.Frame()
	p.model.Draft = "保留的草稿"
	tester.Frame()
	tester.Key(0, ui.KeyEnter)
	if p.selected != "" || len(requests) != 0 || service.Runtime.HasActive() {
		t.Fatal("Enter bypassed the running-task admission")
	}
	if !strings.Contains(p.model.Draft, "保留") || !strings.Contains(p.model.Draft, "草稿") {
		t.Fatal("blocked Enter damaged the draft", p.model.Draft)
	}
}
