// INPUT: Actual SQLite product claims, scripted EngineRuntime streams and an injected persistence fault
// OUTPUT: Coalesced delta rows, preserved event order, bounded visibility latency, contract rejection and latched persistence regressions
// POS: [L7 Hub testing] Stream coalescing changes only how many delta rows are written, never their bytes, order or failure semantics
package hub_test

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/sqlite"
	"github.com/QuincySx/Alva/integration/engine"
)

func coalesceHistory(t *testing.T, store *sqlite.Store, session string) []hub.HistoryEntry {
	t.Helper()
	var all []hub.HistoryEntry
	var after uint64
	for {
		page, err := store.History(t.Context(), session, after, 128)
		if err != nil {
			t.Fatal(err)
		}
		all = append(all, page...)
		if len(page) < 128 {
			return all
		}
		after = page[len(page)-1].Sequence
	}
}

func coalesceStart(t *testing.T, store hub.ExecutionStore, session string, run func(context.Context, engine.Request, func(engine.Event) error) error) <-chan error {
	t.Helper()
	runtime := hub.New(store)
	ownIdentityRuntime(t, runtime)
	if err := runtime.Register("fixture", identityEngine{run: run, cancels: make(chan string, 1)}); err != nil {
		t.Fatal(err)
	}
	_, done, err := runtime.Start(t.Context(), session, "stream fixture")
	if err != nil {
		t.Fatal(err)
	}
	return done
}

func coalesceRun(t *testing.T, store hub.ExecutionStore, session string, run func(context.Context, engine.Request, func(engine.Event) error) error) error {
	t.Helper()
	return identityReceive(t, coalesceStart(t, store, session, run))
}

func TestRuntimeCoalescesFastDeltasWithoutChangingBytes(t *testing.T) {
	store, session := identityStore(t)
	var original strings.Builder
	err := coalesceRun(t, store, session.ID, func(_ context.Context, _ engine.Request, emit func(engine.Event) error) error {
		if err := emit(engine.Event{Kind: engine.SessionStarted, SessionID: "coalesce-native"}); err != nil {
			return err
		}
		for i := 0; i < 1000; i++ {
			text := fmt.Sprintf("片段%d;", i)
			original.WriteString(text)
			if err := emit(engine.Event{Kind: engine.MessageDelta, ID: "assistant-0", Text: text}); err != nil {
				return err
			}
		}
		return emit(engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded})
	})
	if err != nil {
		t.Fatal(err)
	}
	var joined strings.Builder
	rows := 0
	for _, entry := range coalesceHistory(t, store, session.ID) {
		if entry.Event.Kind == engine.MessageDelta {
			rows++
			if len(entry.Event.Text) > 16<<10 {
				t.Fatal("coalesced delta exceeded the byte budget", len(entry.Event.Text))
			}
			joined.WriteString(entry.Event.Text)
		}
	}
	if rows == 0 || rows > 50 || joined.String() != original.String() {
		t.Fatalf("1000 deltas were not coalesced byte-exactly: rows=%d bytes=%d/%d", rows, joined.Len(), original.Len())
	}
}

func TestRuntimeCoalescingKeepsInterleavedEventOrder(t *testing.T) {
	store, session := identityStore(t)
	script := []engine.Event{{Kind: engine.SessionStarted, SessionID: "order-native"}}
	delta := func(kind engine.Kind, id string, count int) {
		for i := 0; i < count; i++ {
			script = append(script, engine.Event{Kind: kind, ID: id, Text: fmt.Sprintf("%s-%d ", id, i)})
		}
	}
	delta(engine.ReasoningDelta, "reasoning-0", 4)
	delta(engine.MessageDelta, "assistant-0", 5)
	script = append(script, engine.Event{Kind: engine.ToolStart, ID: "call-1", Name: "read", Effect: engine.EffectRead}, engine.Event{Kind: engine.ToolEnd, ID: "call-1", Name: "read", Text: "result"})
	delta(engine.MessageDelta, "assistant-0", 3)
	delta(engine.MessageDelta, "assistant-1", 3)
	script = append(script, engine.Event{Kind: engine.ToolStart, ID: "call-2", Name: "read", Effect: engine.EffectRead})
	delta(engine.ReasoningDelta, "reasoning-1", 2)
	script = append(script, engine.Event{Kind: engine.ToolEnd, ID: "call-2", Name: "read", Text: "second"}, engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded})
	err := coalesceRun(t, store, session.ID, func(_ context.Context, _ engine.Request, emit func(engine.Event) error) error {
		for _, event := range script {
			if err := emit(event); err != nil {
				return err
			}
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	// Collapse consecutive same-key deltas: that is the only change allowed.
	type step struct {
		kind engine.Kind
		id   string
		text string
	}
	var want, got []step
	for _, event := range script {
		if n := len(want); n > 0 && (event.Kind == engine.MessageDelta || event.Kind == engine.ReasoningDelta) && want[n-1].kind == event.Kind && want[n-1].id == event.ID {
			want[n-1].text += event.Text
			continue
		}
		want = append(want, step{event.Kind, event.ID, event.Text})
	}
	for _, entry := range coalesceHistory(t, store, session.ID) {
		if entry.Input != "" {
			continue
		}
		event := entry.Event
		if n := len(got); n > 0 && (event.Kind == engine.MessageDelta || event.Kind == engine.ReasoningDelta) && got[n-1].kind == event.Kind && got[n-1].id == event.ID {
			got[n-1].text += event.Text
			continue
		}
		got = append(got, step{event.Kind, event.ID, event.Text})
	}
	if fmt.Sprint(got) != fmt.Sprint(want) {
		t.Fatalf("coalescing reordered durable events\n got %v\nwant %v", got, want)
	}
}

func TestRuntimeCoalescedDeltaVisibleDuringModelPause(t *testing.T) {
	t.Cleanup(hub.SetStreamCoalescingForTest(100*time.Millisecond, 20*time.Millisecond, 16<<10))
	store, session := identityStore(t)
	emitted, resume := make(chan time.Time, 1), make(chan struct{})
	result := coalesceStart(t, store, session.ID, func(_ context.Context, _ engine.Request, emit func(engine.Event) error) error {
		if err := emit(engine.Event{Kind: engine.SessionStarted, SessionID: "pause-native"}); err != nil {
			return err
		}
		if err := emit(engine.Event{Kind: engine.MessageDelta, ID: "assistant-0", Text: "visible before the pause ends"}); err != nil {
			return err
		}
		emitted <- time.Now()
		select {
		case <-resume:
		case <-time.After(2 * time.Second):
		}
		if err := emit(engine.Event{Kind: engine.MessageDelta, ID: "assistant-0", Text: " and continued"}); err != nil {
			return err
		}
		return emit(engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded})
	})
	start := identityReceive(t, emitted)
	visible := false
	for !visible && time.Since(start) < 400*time.Millisecond {
		for _, entry := range coalesceHistory(t, store, session.ID) {
			visible = visible || entry.Event.Kind == engine.MessageDelta && entry.Event.Text == "visible before the pause ends"
		}
		if !visible {
			time.Sleep(10 * time.Millisecond)
		}
	}
	close(resume)
	if err := identityReceive(t, result); err != nil {
		t.Fatal(err)
	}
	if !visible {
		t.Fatal("received delta stayed invisible while the model paused")
	}
}

func TestRuntimeCoalescingKeepsContractRejection(t *testing.T) {
	for _, tc := range []struct {
		name  string
		after func(func(engine.Event) error) error
	}{
		{"delta-after-completed", func(emit func(engine.Event) error) error {
			if err := emit(engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded}); err != nil {
				return err
			}
			return emit(engine.Event{Kind: engine.MessageDelta, ID: "assistant-0", Text: "late"})
		}},
		{"delta-without-identity", func(emit func(engine.Event) error) error {
			return emit(engine.Event{Kind: engine.MessageDelta, Text: "anonymous"})
		}},
	} {
		t.Run(tc.name, func(t *testing.T) {
			store, session := identityStore(t)
			var rejected error
			err := coalesceRun(t, store, session.ID, func(_ context.Context, _ engine.Request, emit func(engine.Event) error) error {
				if err := emit(engine.Event{Kind: engine.SessionStarted, SessionID: "contract-native"}); err != nil {
					return err
				}
				if err := emit(engine.Event{Kind: engine.MessageDelta, ID: "assistant-0", Text: "accepted"}); err != nil {
					return err
				}
				rejected = tc.after(emit)
				// A faulty adapter that ignores the rejection still fails.
				return nil
			})
			if rejected == nil || err == nil {
				t.Fatal("invalid delta was accepted", rejected, err)
			}
			accepted := false
			for _, entry := range coalesceHistory(t, store, session.ID) {
				if entry.Event.Text == "late" || entry.Event.Text == "anonymous" {
					t.Fatal("rejected delta reached durable history")
				}
				accepted = accepted || entry.Event.Kind == engine.MessageDelta && entry.Event.Text == "accepted"
			}
			if !accepted {
				t.Fatal("delta accepted before the rejection was not persisted")
			}
		})
	}
}

type coalesceFaultStore struct {
	*sqlite.Store
	mu      sync.Mutex
	records int
}

func (s *coalesceFaultStore) Record(ctx context.Context, attempt string, event engine.Event, sideEffects bool) error {
	s.mu.Lock()
	s.records++
	s.mu.Unlock()
	if event.Kind == engine.MessageDelta {
		return errors.New("injected delta persistence failure")
	}
	return s.Store.Record(ctx, attempt, event, sideEffects)
}

func (s *coalesceFaultStore) count() int {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.records
}

func TestRuntimeCoalescingLatchesPersistenceErrorAndStopsWrites(t *testing.T) {
	t.Cleanup(hub.SetStreamCoalescingForTest(30*time.Millisecond, 10*time.Millisecond, 16<<10))
	base, session := identityStore(t)
	store := &coalesceFaultStore{Store: base}
	var late func(engine.Event) error
	var afterFailure []error
	err := coalesceRun(t, store, session.ID, func(_ context.Context, _ engine.Request, emit func(engine.Event) error) error {
		late = emit
		if err := emit(engine.Event{Kind: engine.SessionStarted, SessionID: "fault-native"}); err != nil {
			return err
		}
		for i := 0; i < 10; i++ {
			if err := emit(engine.Event{Kind: engine.MessageDelta, ID: "assistant-0", Text: "buffered"}); err != nil {
				return err
			}
		}
		// The tool start flushes the buffered delta first; that commit fails.
		afterFailure = append(afterFailure, emit(engine.Event{Kind: engine.ToolStart, ID: "call", Name: "read", Effect: engine.EffectRead}))
		afterFailure = append(afterFailure, emit(engine.Event{Kind: engine.MessageDelta, ID: "assistant-1", Text: "after failure"}))
		afterFailure = append(afterFailure, emit(engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded}))
		return nil
	})
	if err == nil {
		t.Fatal("latched persistence failure became success")
	}
	for _, callback := range afterFailure {
		if callback == nil {
			t.Fatal("callback after the persistence failure was admitted")
		}
	}
	settled := store.count()
	if late(engine.Event{Kind: engine.MessageDelta, ID: "assistant-2", Text: "late"}) == nil {
		t.Fatal("callback after Execute returned was admitted")
	}
	time.Sleep(100 * time.Millisecond)
	if store.count() != settled {
		t.Fatal("a write happened after Execute returned", settled, store.count())
	}
	for _, entry := range coalesceHistory(t, base, session.ID) {
		if entry.Event.Kind == engine.ToolStart || entry.Event.Text == "after failure" {
			t.Fatal("event after the latched failure reached durable history", entry.Event)
		}
	}
}
