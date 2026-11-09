// INPUT: Actual product SQLite history through the shared TranscriptService and headless MyGo wheel scrolling
// OUTPUT: Top-of-list automatic paging with a fixed first visible row, no pull-down on new content, trimmed-window reload and an opt-in frame-time gate
// POS: [L7 Hub UI tests] Message Stream scrolling regressions without a model, process or OS window
//go:build desktop

package hubdesktop

import (
	"fmt"
	"os"
	"sort"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/egoist/mygo/ui"
)

// seedTranscriptTurns commits ordinary finished turns through the real store.
func seedTranscriptTurns(t *testing.T, service *product.HubService, session string, from, count int) {
	t.Helper()
	for turn := from; turn < from+count; turn++ {
		claim, err := service.Store.BeginTurn(t.Context(), session, fmt.Sprintf("Question %d", turn))
		if err != nil {
			t.Fatal(err)
		}
		for _, event := range []engine.Event{
			{Kind: engine.SessionStarted, SessionID: "native-scroll"},
			{Kind: engine.Message, SessionID: "native-scroll", ID: "assistant-0", Text: fmt.Sprintf("Answer %d. ", turn) + strings.Repeat("A committed paragraph keeps each row tall enough to scroll. ", 3)},
			{Kind: engine.Completed, SessionID: "native-scroll", Outcome: engine.Succeeded},
		} {
			if err := service.Store.Record(t.Context(), claim.Attempt.ID, event, false); err != nil {
				t.Fatal(err)
			}
		}
		if err := service.Store.FinishTurn(t.Context(), claim.Turn.ID, engine.Succeeded); err != nil {
			t.Fatal(err)
		}
	}
}

func waitTranscript(t *testing.T, tester *ui.Tester, ready func() bool) {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		tester.Frame()
		if ready() {
			return
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("transcript did not settle")
}

func transcriptScrollPage(t *testing.T, turns int) (*NativePage, *ui.Tester, hub.Session) {
	t.Helper()
	service, _ := fixture(t)
	session, err := service.Create(t.Context(), "Scrolling", "")
	if err != nil {
		t.Fatal(err)
	}
	seedTranscriptTurns(t, service, session.ID, 0, turns)
	p := NewNativePage(service)
	p.ctx = t.Context()
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	p.selectSession(session.ID)
	waitTranscript(t, tester, func() bool { return !p.polling && !p.chat.Busy && len(p.model.Messages) > 0 })
	return p, tester, session
}

// wheelTranscript scrolls the message list (negative delta goes up) until stop reports true.
func wheelTranscript(t *testing.T, tester *ui.Tester, delta float32, stop func() bool) {
	t.Helper()
	list, ok := tester.Find("Message history")
	if !ok {
		t.Fatal("message list is not rendered")
	}
	for range 400 {
		if stop() {
			return
		}
		tester.Scroll(list.X+list.W/2, list.Y+list.H/2, 0, delta)
		tester.Frame()
	}
	t.Fatal("wheel scrolling never reached its target")
}

func TestWorkspaceTranscriptTopLoadsOlderAndKeepsFirstVisibleRow(t *testing.T) {
	p, tester, _ := transcriptScrollPage(t, 120)
	if !p.transcript.MoreBefore() || len(p.model.Messages) >= 240 {
		t.Fatal("opening did not leave earlier history to page", len(p.model.Messages))
	}
	loaded := len(p.model.Messages)
	anchor := ""
	wheelTranscript(t, tester, -240, func() bool {
		first, last := p.chat.VisibleRows()
		if last >= first && first < 5 {
			anchor = p.model.Messages[first].ID
			return true
		}
		return false
	})
	waitTranscript(t, tester, func() bool { return !p.polling && len(p.model.Messages) > loaded })
	tester.Frame()
	first, last := p.chat.VisibleRows()
	if last < first || p.model.Messages[first].ID != anchor {
		t.Fatal("loading earlier blocks moved the first visible row", anchor, first, p.model.Messages[first].ID)
	}
	seen := map[string]bool{}
	for _, row := range p.model.Messages {
		if seen[row.ID] {
			t.Fatal("paging duplicated a block", row.ID)
		}
		seen[row.ID] = true
	}
	// Keep scrolling: every earlier turn is eventually reachable.
	wheelTranscript(t, tester, -240, func() bool {
		return !p.polling && !p.transcript.MoreBefore()
	})
	if p.model.Messages[0].Text != "Question 0" {
		t.Fatal("scrolling to the top did not reach the first turn", p.model.Messages[0].Text)
	}
}

func TestWorkspaceTranscriptNewContentDoesNotPullReviewerDown(t *testing.T) {
	p, tester, session := transcriptScrollPage(t, 40)
	wheelTranscript(t, tester, -240, func() bool {
		first, last := p.chat.VisibleRows()
		return last >= first && first > 0 && first < len(p.model.Messages)-20 && !p.chat.AtEnd()
	})
	first, _ := p.chat.VisibleRows()
	anchor := p.model.Messages[first].ID
	before := p.transcript.Last()
	seedTranscriptTurns(t, p.Service, session.ID, 40, 2)
	p.poll()
	waitTranscript(t, tester, func() bool { return !p.polling && p.transcript.Last() > before })
	tester.Frame()
	first, last := p.chat.VisibleRows()
	if p.chat.AtEnd() || last < first || p.model.Messages[first].ID != anchor || p.model.Messages[len(p.model.Messages)-1].RunID == "" {
		t.Fatal("new content pulled the reader back to the bottom", anchor, p.model.Messages[first].ID, p.chat.AtEnd())
	}
}

func TestWorkspaceTranscriptTrimmedWindowReloadsLatestAtBottom(t *testing.T) {
	p, tester, session := transcriptScrollPage(t, 120)
	// A tight window shows the MoreAfter path that a 64 MiB one reaches only
	// on very long sessions.
	p.transcript = product.NewTranscriptWindow(24 << 10)
	p.loadTail()
	waitTranscript(t, tester, func() bool { return !p.polling })
	wheelTranscript(t, tester, -240, func() bool { return !p.polling && p.transcript.MoreAfter() })
	before := p.transcript.Last()
	seedTranscriptTurns(t, p.Service, session.ID, 120, 1)
	p.poll()
	waitTranscript(t, tester, func() bool { return !p.polling })
	if p.transcript.Last() != before {
		t.Fatal("a trimmed window kept following new text")
	}
	wheelTranscript(t, tester, 240, func() bool { return !p.polling && !p.transcript.MoreAfter() })
	waitTranscript(t, tester, func() bool { return !p.polling && p.transcript.Last() > before })
	if last := p.model.Messages[len(p.model.Messages)-1]; !strings.HasPrefix(last.Text, "Answer 120.") {
		t.Fatal("reaching the bottom did not reload the latest messages", last.Text)
	}
}

func TestWorkspaceTranscriptMenuDropsEarlierMessages(t *testing.T) {
	p, tester, _ := transcriptScrollPage(t, 2)
	if err := workspaceTestOpenMenu(tester); err != nil {
		t.Fatal(err)
	}
	if err := tester.ChooseMenuItem("更早的消息"); err == nil {
		t.Fatal("the manual earlier-messages menu item still exists")
	}
	tester.Frame()
	if err := workspaceTestMenu(tester, "最新消息"); err != nil {
		t.Fatal("latest messages menu item disappeared", err)
	}
	waitTranscript(t, tester, func() bool { return !p.polling })
}

// ALVA_UI_BENCH=1 measures headless build/layout/paint CPU while scrolling 20k
// rows; it does not measure GPU present or real input latency.
func TestWorkspaceTranscriptScrollFrameBudget(t *testing.T) { transcriptScrollFrameBudget(t, false) }

// The same gate with a version switcher on every user message: the worst case
// of a conversation whose each Turn was edited or regenerated.
func TestWorkspaceTranscriptScrollFrameBudgetWithVersionSwitchers(t *testing.T) {
	transcriptScrollFrameBudget(t, true)
}

func transcriptScrollFrameBudget(t *testing.T, versions bool) {
	if os.Getenv("ALVA_UI_BENCH") != "1" {
		t.Skip("set ALVA_UI_BENCH=1 to run the frame-time gate")
	}
	service, _ := fixture(t)
	p := NewNativePage(service)
	p.ctx = t.Context()
	p.selected = "bench"
	var entries []hub.HistoryEntry
	for turn := 0; turn < 10000; turn++ {
		id, attempt := fmt.Sprintf("turn-%d", turn), fmt.Sprintf("attempt-%d", turn)
		entries = append(entries,
			hub.HistoryEntry{Sequence: uint64(2*turn + 1), SessionID: "bench", TurnID: id, AttemptID: attempt, Input: fmt.Sprintf("第 %d 个问题", turn)},
			hub.HistoryEntry{Sequence: uint64(2*turn + 2), SessionID: "bench", TurnID: id, AttemptID: attempt, Event: engine.Event{Kind: engine.Message, ID: "assistant-0", Text: fmt.Sprintf("## 回答 %d\n\n", turn) + strings.Repeat("这是一段包含 **粗体** 和 `代码` 的正文。 ", 4+turn%12)}})
	}
	service.Transcripts = &product.TranscriptService{Store: fixedHistory(entries)}
	for p.transcript.Last() < uint64(len(entries)) {
		appendTranscript(t, p, entries...)
	}
	p.applyDetail()
	if versions {
		p.thread.session = p.selected
		p.thread.switches = map[string]product.VersionSwitch{}
		for turn := 0; turn < 10000; turn++ {
			id := fmt.Sprintf("turn-%d", turn)
			p.thread.switches[id] = product.VersionSwitch{TurnID: id, Index: 2, Count: 3, Choices: []product.VersionChoice{{TurnID: "a", LineID: "l1"}, {TurnID: id, LineID: "l2", Active: true}, {TurnID: "c", LineID: "l3"}}}
		}
	}
	if len(p.model.Messages) != 20000 {
		t.Fatal("bench window does not hold 20k rows", len(p.model.Messages))
	}
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	list, ok := tester.Find("Message history")
	if !ok {
		t.Fatal("message list is not rendered")
	}
	var frames []time.Duration
	for _, delta := range []float32{-240, 240, -2400} {
		before, _ := p.chat.VisibleRows()
		for range 200 {
			start := time.Now()
			tester.Scroll(list.X+list.W/2, list.Y+list.H/2, 0, delta)
			tester.Frame()
			frames = append(frames, time.Since(start))
		}
		if after, _ := p.chat.VisibleRows(); after == before {
			t.Fatal("bench frames did not scroll the list", delta, before)
		}
	}
	sort.Slice(frames, func(i, j int) bool { return frames[i] < frames[j] })
	p95 := frames[len(frames)*95/100]
	t.Logf("20k rows headless scroll (version switchers=%v): p50=%s p95=%s max=%s n=%d", versions, frames[len(frames)/2], p95, frames[len(frames)-1], len(frames))
	if p95 > 8*time.Millisecond {
		t.Fatal("scroll frame CPU p95 exceeds 8 ms", p95)
	}
}
