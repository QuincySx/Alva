// INPUT: context, path/filepath, testing, time, hub runtime with stub engines, product SQLite store, native session repository and synthetic history
// OUTPUT: Manual compaction intent, context compaction blocks and latest context meter regressions
// POS: [L7 Hub product] Intents need the engine capability and stay one-shot; the meter shows only measured reports from the newest attempt.
package product

import (
	"context"
	"path/filepath"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	productstore "github.com/QuincySx/Alva/apps/hub/sqlite"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage/session"
)

type capabilityEngine struct{ compaction bool }

func (capabilityEngine) Execute(context.Context, engine.Request, func(engine.Event) error) error {
	return engine.ErrUnsupported
}
func (capabilityEngine) Cancel(context.Context, string) error { return nil }
func (capabilityEngine) RespondPermission(context.Context, string, string, engine.PermissionDecision) error {
	return engine.ErrUnsupported
}
func (e capabilityEngine) Capabilities() engine.Capabilities {
	return engine.Capabilities{ContextCompaction: e.compaction}
}

func contextService(t *testing.T, compaction bool) (*HubService, hub.Session) {
	t.Helper()
	directory := filepath.Join(t.TempDir(), "private")
	store, err := productstore.Open(t.Context(), filepath.Join(directory, "product"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = store.Close() })
	repo, err := session.Open(t.Context(), filepath.Join(directory, "native"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = repo.Close() })
	runtime := hub.New(store)
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		_ = runtime.Close(ctx)
	})
	s, err := NewHubService(store, runtime, repo, hub.Target{EngineID: "alva", ModelID: "fixture", ProviderID: "local", AccountSource: "test", BillingRoute: "no-billing"}, directory, true)
	if err != nil {
		t.Fatal(err)
	}
	if err = runtime.Register("alva", capabilityEngine{compaction: compaction}); err != nil {
		t.Fatal(err)
	}
	created, err := s.Create(t.Context(), "context", "")
	if err != nil {
		t.Fatal(err)
	}
	return s, created
}

func TestManualCompactionIntentNeedsTheEngineCapabilityAndIsOneShot(t *testing.T) {
	s, created := contextService(t, true)
	if s.ContextCompactionPending(created.ID) {
		t.Fatal("fresh session already pending")
	}
	if err := s.RequestContextCompaction(t.Context(), created.ID); err != nil {
		t.Fatal(err)
	}
	if !s.ContextCompactionPending(created.ID) {
		t.Fatal("intent not recorded")
	}
	s.CancelContextCompaction(created.ID)
	if s.ContextCompactionPending(created.ID) {
		t.Fatal("cancel kept the intent")
	}
	if err := s.RequestContextCompaction(t.Context(), created.ID); err != nil {
		t.Fatal(err)
	}
	s.Context.Clear(created.ID)
	if s.ContextCompactionPending(created.ID) {
		t.Fatal("admission did not clear the intent")
	}
	if err := s.RequestContextCompaction(t.Context(), "missing-session"); err == nil {
		t.Fatal("intent accepted for an unknown session")
	}
	plain, other := contextService(t, false)
	if err := plain.RequestContextCompaction(t.Context(), other.ID); err != ErrContextCompactionUnsupported || plain.ContextCompactionPending(other.ID) {
		t.Fatal("intent accepted for an engine that cannot compact", err)
	}
}

func contextEntry(seq uint64, attempt string, report engine.ContextReport) hub.HistoryEntry {
	return hub.HistoryEntry{Sequence: seq, TurnID: "turn-" + attempt, AttemptID: attempt, Event: engine.Event{Kind: engine.ContextUpdated, SessionID: "native", Context: &report}}
}

func TestContextReportsProjectCompactionBlocksAndTheLatestMeter(t *testing.T) {
	measured := func(input uint64) engine.ContextReport {
		return engine.ContextReport{Usage: &engine.ContextUsage{InputTokens: input, OutputTokens: 10, WindowTokens: 1000, Parts: &engine.ContextParts{System: 10, Tools: 20, Summary: 30, Recent: input - 60 + 10}}}
	}
	compacted := engine.ContextReport{Compaction: &engine.ContextCompaction{Status: engine.ContextCompacted, Trigger: "auto", FromSequence: 1, ThroughSequence: 40, Messages: 30, Reference: &engine.Reference{SessionID: "native", RecordID: "native:41", Sequence: 41}}}
	corrupt := engine.ContextReport{Compaction: &engine.ContextCompaction{Status: engine.ContextSummaryCorrupt, Trigger: "auto", Reference: &engine.Reference{SessionID: "native", RecordID: "native:9", Sequence: 9}}}
	w := NewTranscriptWindow(0)
	w.Reset(chunkOf(
		hub.HistoryEntry{Sequence: 1, TurnID: "turn-a", AttemptID: "a", Input: "question"},
		contextEntry(2, "a", corrupt),
		contextEntry(3, "a", compacted),
		contextEntry(4, "a", measured(400)),
		contextEntry(5, "a", measured(600)),
		hub.HistoryEntry{Sequence: 6, TurnID: "turn-a", AttemptID: "a", Event: engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded}},
	))
	block := blockByID(t, w, "a/context/3")
	if block.Kind != BlockKindStatus || block.Name != "context" || block.Context == nil || block.Context.Messages != 30 || block.Ref == nil || block.Ref.Sequence != 41 || block.Status != BlockStatusCompleted {
		t.Fatal("compaction did not project as a typed status block", block)
	}
	if failed := blockByID(t, w, "a/context/2"); failed.Status != BlockStatusFailed || failed.Context.Status != engine.ContextSummaryCorrupt {
		t.Fatal("corrupt summary not a failed status block", failed)
	}
	meter := w.Context()
	if meter == nil || meter.InputTokens != 600 || meter.UsedTokens() != 610 {
		t.Fatal("meter does not show the newest measurement", meter)
	}
	meter.InputTokens = 1
	if w.Context().InputTokens != 600 {
		t.Fatal("meter value is shared with the window")
	}
	// A newer attempt that completed without measuring makes the meter stale.
	w.ApplyNewer(chunkOf(hub.HistoryEntry{Sequence: 7, TurnID: "turn-b", AttemptID: "b", Input: "next"}, hub.HistoryEntry{Sequence: 8, TurnID: "turn-b", AttemptID: "b", Event: engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded}}))
	if w.Context() != nil {
		t.Fatal("stale measurement still shown after an unmeasured attempt")
	}
	// The newest window decides; older pages only fill an undecided one.
	w.Reset(chunkOf(hub.HistoryEntry{Sequence: 20, TurnID: "turn-c", AttemptID: "c", Input: "running"}))
	w.ApplyOlder(chunkOf(contextEntry(10, "a", measured(500))))
	if meter := w.Context(); meter == nil || meter.InputTokens != 500 {
		t.Fatal("older measurement not used while the newest attempt has not reported", meter)
	}
	w.Reset(chunkOf(contextEntry(30, "d", measured(700))))
	w.ApplyOlder(chunkOf(contextEntry(10, "a", measured(500))))
	if meter := w.Context(); meter == nil || meter.InputTokens != 700 {
		t.Fatal("older page replaced the newest measurement", meter)
	}
}
