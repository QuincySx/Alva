// INPUT: Product transcript pages with engine context reports, a capability-only stub engine, the connection form and headless MyGo frames
// OUTPUT: Context meter chip/popover, manual compaction intent, compaction status lines and per-model context-window input regressions with optional screenshots
// POS: [L7 Hub UI tests] Presents only measured context; no model, process or OS window runs
//go:build desktop

package hubdesktop

import (
	"context"
	"image/png"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/egoist/mygo/ui"
)

type compactingStubEngine struct{}

func (compactingStubEngine) Execute(context.Context, engine.Request, func(engine.Event) error) error {
	return engine.ErrUnsupported
}
func (compactingStubEngine) Cancel(context.Context, string) error { return nil }
func (compactingStubEngine) RespondPermission(context.Context, string, string, engine.PermissionDecision) error {
	return engine.ErrUnsupported
}
func (compactingStubEngine) Capabilities() engine.Capabilities {
	return engine.Capabilities{ContextCompaction: true}
}

func contextUsageEntry(session string, seq uint64, input, output, window uint64) hub.HistoryEntry {
	parts := &engine.ContextParts{System: input / 10, Tools: input / 5, Summary: input / 4}
	parts.Recent = input + output - parts.System - parts.Tools - parts.Summary
	report := engine.ContextReport{Usage: &engine.ContextUsage{InputTokens: input, OutputTokens: output, WindowTokens: window, Parts: parts}}
	return hub.HistoryEntry{Sequence: seq, SessionID: session, TurnID: "turn", AttemptID: "attempt", Event: engine.Event{Kind: engine.ContextUpdated, SessionID: "native-context", Context: &report}}
}

func contextPage(t *testing.T, compaction bool, entries func(session string) []hub.HistoryEntry) (*NativePage, *ui.Tester) {
	t.Helper()
	service, requests := fixture(t)
	if compaction {
		if err := service.Runtime.Register("alva", compactingStubEngine{}); err != nil {
			t.Fatal(err)
		}
	}
	session, err := service.Create(t.Context(), "Context", "")
	if err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(service)
	p.ctx = t.Context()
	p.selected = session.ID
	if p.detail, err = service.Store.Detail(t.Context(), session.ID); err != nil {
		t.Fatal(err)
	}
	replaceTranscript(t, p, entries(session.ID)...)
	p.applyDetail()
	tester := ui.NewTester(workspaceStaticView(p), 1240, 900)
	t.Cleanup(func() {
		if len(requests) != 0 || service.Runtime.HasActive() {
			t.Error("context presentation started execution")
		}
	})
	return p, tester
}

func saveContextScreenshot(t *testing.T, tester *ui.Tester, name string) {
	t.Helper()
	dir := os.Getenv("ALVA_CONTEXT_SCREENSHOT_DIR")
	if dir == "" {
		return
	}
	if err := os.MkdirAll(dir, 0700); err != nil {
		t.Fatal(err)
	}
	f, err := os.Create(filepath.Join(dir, name))
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	if err := png.Encode(f, tester.Image()); err != nil {
		t.Fatal(err)
	}
}

func contextHistory(session string, usage ...hub.HistoryEntry) []hub.HistoryEntry {
	entries := []hub.HistoryEntry{
		{Sequence: 1, SessionID: session, TurnID: "turn", Input: "继续整理这周的发布记录"},
		{Sequence: 2, SessionID: session, AttemptID: "attempt", Event: engine.Event{Kind: engine.SessionStarted, SessionID: "native-context"}},
		{Sequence: 3, SessionID: session, AttemptID: "attempt", Event: engine.Event{Kind: engine.Message, ID: "reply", Text: "已整理完成。"}},
	}
	return append(entries, usage...)
}

func TestWorkspaceContextMeterShowsMeasuredShareAndComposition(t *testing.T) {
	p, tester := contextPage(t, true, func(s string) []hub.HistoryEntry {
		return contextHistory(s, contextUsageEntry(s, 4, 78_360, 1_000, 128_000))
	})
	chip, ok := tester.Find("Context usage")
	if !ok || !tester.HasText("上下文 62%") {
		t.Fatal("context chip missing or not a percentage of the declared window", tester.Texts())
	}
	composer, ok := tester.Find("Task composer")
	if !ok || chip.Y < composer.Y || chip.Y+chip.H > composer.Y+composer.H {
		t.Fatal("context chip escaped the composer", chip, composer)
	}
	saveContextScreenshot(t, tester, "context-chip-light.png")
	if err := tester.Click("Context usage"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	for _, text := range []string{"已用 62% · 79,360 / 128,000 tokens", "系统", "工具定义", "摘要", "最近消息", "构成按最近一次请求的实际总数等比估算。", "压缩"} {
		if !tester.HasText(text) {
			t.Fatal("popover text missing", text, tester.Texts())
		}
	}
	if tester.HasText("接近上限，下次请求前会自动压缩早期对话。") {
		t.Fatal("warning shown below the trigger")
	}
	saveContextScreenshot(t, tester, "context-popover-light.png")
	tester.SetDark(true)
	tester.Frame()
	saveContextScreenshot(t, tester, "context-popover-dark.png")
	if !p.composer.contextOpen {
		t.Fatal("popover state lost")
	}
}

func TestWorkspaceContextMeterWithoutDeclaredWindowShowsOnlyTokens(t *testing.T) {
	_, tester := contextPage(t, false, func(s string) []hub.HistoryEntry {
		return contextHistory(s, contextUsageEntry(s, 4, 41_800, 200, 0))
	})
	if !tester.HasText("上下文 4.2万") || tester.HasText("%") {
		t.Fatal("undeclared window shown as a percentage", tester.Texts())
	}
	if err := tester.Click("Context usage"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !tester.HasText("已用 42,000 tokens · 模型窗口未声明") || tester.HasText("压缩") {
		t.Fatal("undeclared popover misreported or offered compaction without the capability", tester.Texts())
	}
	saveContextScreenshot(t, tester, "context-undeclared-light.png")
}

func TestWorkspaceContextMeterWarnsAndRequestsCompaction(t *testing.T) {
	p, tester := contextPage(t, true, func(s string) []hub.HistoryEntry {
		return contextHistory(s, contextUsageEntry(s, 4, 110_000, 2_000, 128_000))
	})
	if !tester.HasText("上下文 88%") {
		t.Fatal("near-limit chip missing", tester.Texts())
	}
	if err := tester.Click("Context usage"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if !tester.HasText("接近上限，下次请求前会自动压缩早期对话。") {
		t.Fatal("near-limit warning missing")
	}
	saveContextScreenshot(t, tester, "context-warning-light.png")
	if err := tester.Click("Compact context before the next request"); err != nil {
		t.Fatal(err)
	}
	select {
	case update := <-p.updates:
		update()
	case <-time.After(3 * time.Second):
		t.Fatal("compaction request did not settle")
	}
	tester.Frame()
	if !p.Service.ContextCompactionPending(p.selected) || !tester.HasText("下次发送前压缩") || !tester.HasText("取消压缩") {
		t.Fatal("manual compaction intent not shown", tester.Texts())
	}
	saveContextScreenshot(t, tester, "context-pending-light.png")
	if err := tester.Click("Cancel context compaction"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if p.Service.ContextCompactionPending(p.selected) || tester.HasText("下次发送前压缩") {
		t.Fatal("cancel kept the intent")
	}
}

func TestWorkspaceContextMeterHiddenWithoutMeasurement(t *testing.T) {
	_, tester := contextPage(t, true, func(s string) []hub.HistoryEntry { return contextHistory(s) })
	if _, ok := tester.Find("Context usage"); ok {
		t.Fatal("meter shown without a measured report")
	}
}

func TestWorkspaceCompactionStatusLines(t *testing.T) {
	_, tester := contextPage(t, true, func(s string) []hub.HistoryEntry {
		compacted := engine.ContextReport{Compaction: &engine.ContextCompaction{Status: engine.ContextCompacted, Trigger: "manual", FromSequence: 1, ThroughSequence: 40, Messages: 30, Reference: &engine.Reference{SessionID: "native-context", RecordID: "native-context:41", Sequence: 41}}}
		corrupt := engine.ContextReport{Compaction: &engine.ContextCompaction{Status: engine.ContextSummaryCorrupt, Trigger: "auto", Reference: &engine.Reference{SessionID: "native-context", RecordID: "native-context:9", Sequence: 9}}}
		failed := engine.ContextReport{Compaction: &engine.ContextCompaction{Status: engine.ContextCompactionFailed, Trigger: "auto", Error: "provider down"}}
		entries := contextHistory(s)
		for i, report := range []engine.ContextReport{corrupt, compacted, failed} {
			report := report
			entries = append(entries, hub.HistoryEntry{Sequence: uint64(4 + i), SessionID: s, TurnID: "turn", AttemptID: "attempt", Event: engine.Event{Kind: engine.ContextUpdated, SessionID: "native-context", Context: &report}})
		}
		return entries
	})
	for _, text := range []string{"已压缩早期对话 · 覆盖 30 条消息（记录 #1–#40） · 手动", "摘要记录损坏，本次按近期对话截断", "压缩失败，本次按近期对话截断", "查看记录"} {
		if !tester.HasText(text) {
			t.Fatal("status line missing", text, tester.Texts())
		}
	}
	saveContextScreenshot(t, tester, "context-status-lines-light.png")
	tester.SetDark(true)
	tester.Frame()
	saveContextScreenshot(t, tester, "context-status-lines-dark.png")
}

func TestNativeConnectionsContextWindowInputAndCatalogPrecedence(t *testing.T) {
	s, _ := connectionsFixture(t)
	p := NewNativePage(s)
	tester := openConnectionTestPage(t, p, 1240, 850)
	startOpenAIConnection(t, tester)
	connectionType(t, tester, "Edit API Key", "dummy-key-never-a-real-credential")
	connectionType(t, tester, "Edit 模型 ID", "operator-model")
	connectionClick(t, tester, "Add model to connection draft")
	connectionType(t, tester, "Context window tokens operator-model", "131072")
	p.connections.draft.Models = append(p.connections.draft.Models, hub.ConnectionModel{ID: "catalog-model", Enabled: true, CatalogContextWindow: 200000})
	tester.Frame()
	if !tester.HasText("上下文 200,000 · 目录") {
		t.Fatal("catalog-stated window not shown in place of the input", tester.Texts())
	}
	if _, ok := tester.Find("Context window tokens catalog-model"); ok {
		t.Fatal("catalog-stated window is editable")
	}
	saveContextScreenshot(t, tester, "connection-context-window-light.png")
	connectionClick(t, tester, "Save connection")
	settleConnections(t, p, tester)
	entries := s.Connections.Snapshot()
	if len(entries) != 1 || len(entries[0].Models) != 2 || entries[0].Models[0].ContextWindow != 131072 || entries[0].Models[1].DeclaredContextWindow() != 200000 {
		t.Fatal("context windows not saved", entries)
	}
	connectionClick(t, tester, "Edit connection")
	connectionType(t, tester, "Context window tokens operator-model", "about 100k")
	connectionClick(t, tester, "Save connection")
	settleConnections(t, p, tester)
	if s.Connections.Snapshot()[0].Models[0].ContextWindow != 131072 || p.connections.problem == "" {
		t.Fatal("an invalid window was saved or not rejected visibly", p.connections.problem)
	}
}
