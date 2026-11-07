// INPUT:  testing, MyGo ui.Tester, native presentation state/view
// OUTPUT: headless native input and presentation regression tests
// POS: [L6 application]    Proves native view input without launching a window or external services
//go:build desktop

package chat

import (
	"fmt"
	"math"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
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
	if sends != 0 || !tt.HasText("Type a message below. Nothing has been sent.") {
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

func TestNativeConversationRendererSharesFallbackAndLeavesDirectView(t *testing.T) {
	m := NewModel(true)
	m.Messages = []Message{
		{ID: "tool", Role: "Tool", Text: "Hidden source", Status: Completed},
		{ID: "reply", Role: "Alva", Text: "Ordinary paragraph", Status: Completed},
	}
	v := NewNativeView(m)
	customRows := 0
	v.RenderMessage = func(c *ui.Context, row Message) bool {
		if row.Role != "Tool" {
			return false
		}
		customRows++
		ui.Text(c, "Compact tool")
		return true
	}
	theme := &markdown.Theme{FontSize: 18, LineHeight: 1.4}
	v.SetMarkdownOptions(&markdown.Options{Theme: theme})
	conversation := true
	tt := ui.NewTester(func(c *ui.Context) {
		if conversation {
			v.BuildMessages(c)
		} else {
			v.BuildChat(c)
		}
	}, 1100, 720)
	if customRows == 0 || !tt.HasText("Compact tool") || tt.HasText("Hidden source") || !tt.HasText("Ordinary paragraph") {
		t.Fatal("custom row bypassed shared list or ordinary Markdown fallback", tt.Texts())
	}
	viewport, found := tt.Find("Message history")
	paragraph, paragraphFound := tt.Find("Ordinary paragraph")
	if !found || !paragraphFound || viewport.W != 1100 || paragraph.W > 760 || paragraph.X < 170 {
		t.Fatal("conversation constrained its viewport instead of its centered reading rows", viewport, paragraph)
	}
	if v.markdown.Theme != theme || theme.FontSize != 18 || theme.LineHeight != 1.4 {
		t.Fatal("conversation mutated the caller's Markdown theme")
	}
	before := customRows
	conversation = false
	tt.Frame()
	if customRows != before || !tt.HasText("Hidden source") || !tt.HasText("Ordinary paragraph") {
		t.Fatal("product row renderer changed the direct chat presentation")
	}
	directParagraph, ok := tt.Find("Ordinary paragraph")
	if !ok || math.Abs(float64(directParagraph.H-18*1.4)) > .01 {
		t.Fatal("conversation typography replaced the direct caller's Markdown options", directParagraph)
	}
}

func TestNativeConversationMessageGapDefaultsAndLeavesDirectView(t *testing.T) {
	m := NewModel(true)
	m.Messages = []Message{
		{ID: "first", Role: "You", Text: "First paragraph", Status: Completed},
		{ID: "second", Role: "You", Text: "Second paragraph", Status: Completed},
	}
	v := NewNativeView(m)
	v.RenderMessage = func(c *ui.Context, row Message) bool {
		ui.Box(c).Label("Custom row " + row.ID).FillWidth().Height(20)
		return true
	}
	conversation := true
	tester := ui.NewTester(func(c *ui.Context) {
		if conversation {
			v.BuildMessages(c)
		} else {
			v.BuildChat(c)
		}
	}, 900, 480)
	for _, sample := range []struct{ value, want float32 }{{0, 16}, {24, 24}, {-1, 16}} {
		v.MessageGap = sample.value
		tester.Frame()
		first, firstOK := tester.Find("Custom row first")
		second, secondOK := tester.Find("Custom row second")
		if !firstOK || !secondOK || math.Abs(float64(second.Y-first.Y-first.H-sample.want)) > .01 {
			t.Fatal("conversation did not use its default or explicit row gap", sample, first, second)
		}
	}
	conversation, v.MessageGap = false, 24
	tester.Frame()
	before, ok := tester.Find("First paragraph")
	if !ok {
		t.Fatal("direct view did not retain its own row renderer")
	}
	v.MessageGap = 0
	tester.Frame()
	after, ok := tester.Find("First paragraph")
	if !ok || before != after {
		t.Fatal("conversation row spacing changed the original direct view", before, after)
	}
}

func TestNativeConversationMessageGapRetainsVirtualReadingAnchor(t *testing.T) {
	m := NewModel(true)
	for i := range 1000 {
		m.Messages = append(m.Messages, Message{ID: fmt.Sprintf("message-%d", i), Role: "You", Text: fmt.Sprintf("Row %d\n%s", i, strings.Repeat("variable-height line\n", i%4)), Status: Completed})
	}
	v := NewNativeView(m)
	v.MessageGap = 24
	tester := ui.NewTester(v.BuildMessages, 900, 480)
	first, last := v.list.Visible()
	if last-first > 128 || !v.AtEnd() {
		t.Fatal("explicit conversation row gap lost virtualization or follow-end", first, last)
	}
	viewport, ok := tester.Find("Message history")
	if !ok {
		t.Fatal("conversation viewport unavailable")
	}
	tester.Scroll(viewport.X+viewport.W/2, viewport.Y+viewport.H/2, 0, -1200)
	first, _ = v.list.Visible()
	if v.AtEnd() {
		t.Fatal("scrolling conversation did not leave follow-end")
	}
	anchorID := m.Messages[first].ID
	m.Messages = append(m.Messages, Message{ID: "new-message", Role: "You", Text: "A later arrival", Status: Completed})
	tester.Frame()
	after, _ := v.list.Visible()
	if m.Messages[after].ID != anchorID || v.AtEnd() {
		t.Fatal("explicit conversation row gap displaced the user's reading anchor", anchorID, m.Messages[after].ID)
	}
	v.ScrollToEnd()
	tester.Frame()
	if !v.AtEnd() {
		t.Fatal("conversation did not resume following after an explicit jump")
	}
}

func TestNativeConversationLateVirtualRowsKeepReadingTypography(t *testing.T) {
	m := NewModel(true)
	for i := range 100 {
		role := "Compact"
		if i%20 == 0 {
			role = "Alva"
		} else if i%20 == 1 {
			role = "You"
		}
		m.Messages = append(m.Messages, Message{ID: fmt.Sprint(i), Role: role, Text: fmt.Sprintf("Paragraph %d", i), Status: Completed})
	}
	m.Messages[82] = Message{ID: "82", Role: "Alva", Status: Running}
	v := NewNativeView(m)
	theme := &markdown.Theme{FontSize: 18, LineHeight: 1.4}
	v.SetMarkdownOptions(&markdown.Options{Theme: theme})
	v.RenderMessage = func(c *ui.Context, row Message) bool {
		if row.Role != "Compact" {
			return false
		}
		ui.Box(c).Height(1)
		return true
	}
	size := float32(14)
	tester := ui.NewTester(func(c *ui.Context) {
		effective := ui.LightTheme()
		effective.FontSize = size
		c.SetTheme(effective)
		v.BuildMessages(c)
	}, 900, 480)
	for _, next := range []float32{14, 16, 24} {
		size = next
		v.list.ScrollTo(80, ui.Start)
		tester.Frame()
		for _, label := range []string{"Paragraph 80", "Paragraph 81", "正在整理思路…"} {
			paragraph, ok := tester.Find(label)
			if !ok || math.Abs(float64(paragraph.H-size*1.75)) > .01 {
				t.Fatal("late virtual row did not consume effective theme body size", size, label, paragraph, ok)
			}
		}
		if v.markdown.Theme != theme || theme.FontSize != 18 || theme.LineHeight != 1.4 {
			t.Fatal("reading typography changed the caller-owned Markdown theme", theme)
		}
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
