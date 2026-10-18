// INPUT:  testing, MyGo ui.Tester, native presentation state/view
// OUTPUT: headless native input and presentation regression tests
// POS: [L6 application]    Proves native view input without launching a window or external services
//go:build desktop

package desktop

import (
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
)

func TestNativeComposerRequiresExplicitSend(t *testing.T) {
	m := NewModel(true)
	v := NewNativeView(m)
	sends := 0
	v.Send = func() {
		prompt, err := m.Prompt()
		if err != nil {
			t.Fatal(err)
		}
		sends++
		m.Admitted("test-run", prompt)
	}
	tt := ui.NewTester(v.Build, 900, 720)
	if sends != 0 || !tt.HasText("DEMO · local fixture, no model service") {
		t.Fatal("startup must be truthful and must not send")
	}
	if err := tt.Click("Send"); err != nil {
		t.Fatal(err)
	}
	if sends != 0 {
		t.Fatal("empty Send was not disabled")
	}
	if err := tt.Click("Message"); err != nil {
		t.Fatal(err)
	}
	tt.Compose("中文", 2)
	if _, active := tt.TextCaret(); !active || sends != 0 {
		t.Fatal("native composer did not accept IME preedit")
	}
	tt.Type("中文")
	tt.Compose("", 0)
	tt.Type("🙂 **not sent**")
	tt.Key(0, ui.KeyEnter)
	if sends != 0 || !strings.Contains(m.Draft, "中文🙂") || !strings.Contains(m.Draft, "\n") {
		t.Fatal("typing/Enter must edit only, including CJK text")
	}
	if err := tt.Click("Send"); err != nil {
		t.Fatal(err)
	}
	if sends != 1 || m.ActiveRunID != "test-run" || m.Draft != "" {
		t.Fatal("explicit Send was not reflected")
	}
	if err := tt.Click("Send"); err != nil {
		t.Fatal(err)
	}
	if sends != 1 {
		t.Fatal("running Send was not disabled")
	}
}

func TestNativeHistoryVirtualizationAndReadingAnchor(t *testing.T) {
	m := NewModel(true)
	// Large fixture tests layout, not the foundation's 64-row retention policy.
	for i := 0; i < 10_000; i++ {
		m.Messages = append(m.Messages, Message{
			ID: fmt.Sprintf("message-%d", i), Role: "You", Status: Completed,
			Text: fmt.Sprintf("Row %d\n%s", i, strings.Repeat("variable-height line\n", i%4)),
		})
	}
	v := NewNativeView(m)
	tt := ui.NewTester(v.Build, 900, 720)
	first, last := v.list.Visible()
	if last-first > 128 || !v.list.AtEnd() {
		t.Fatalf("list did not virtualize/follow end: %d..%d", first, last)
	}
	box, ok := tt.Find("Message history")
	if !ok {
		t.Fatal("history list not accessible")
	}
	tt.Scroll(box.X+box.W/2, box.Y+box.H/2, 0, -1200)
	first, _ = v.list.Visible()
	if v.list.AtEnd() {
		t.Fatal("user scroll should leave follow-end")
	}
	anchorID := m.Messages[first].ID
	m.Messages = append(m.Messages, Message{ID: "new-message", Role: "You", Text: "A later arrival", Status: Completed})
	tt.Frame()
	after, _ := v.list.Visible()
	if m.Messages[after].ID != anchorID || v.list.AtEnd() {
		t.Fatal("new message displaced the user's reading anchor")
	}
	if err := tt.Click("Jump to latest"); err != nil {
		t.Fatal(err)
	}
	if !v.list.AtEnd() {
		t.Fatal("Jump to latest did not resume following")
	}
}

func TestNativeStopAndTerminalMarkdown(t *testing.T) {
	m := NewModel(true)
	m.Admitted("run", "show a table")
	m.Observe("run", 1, "**partial**\n\n| A | B |\n|---|---|\n| 1 | 2 |", Running, "")
	v := NewNativeView(m)
	stops := 0
	v.Stop = func() {
		if m.RequestStop() != "" {
			stops++
		}
	}
	tt := ui.NewTester(v.Build, 900, 720)
	if err := tt.Click("Stop"); err != nil {
		t.Fatal(err)
	}
	if stops != 1 || !tt.HasText("Stopping — waiting for runtime acknowledgement") {
		t.Fatal("Stop must wait for runtime acknowledgement")
	}
	m.Observe("run", 2, "**partial**\n\nStopped safely.", Cancelled, "")
	tt.Frame()
	tt.Advance(time.Second)
	if !tt.HasText("Stopped — partial output retained") || !tt.HasText("Stopped safely.") {
		t.Fatalf("terminal Markdown or status missing: %q", tt.Texts())
	}
}
