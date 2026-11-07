// INPUT: encoding/json, strings, testing, time, llm/wire
// OUTPUT: Prepared intent identity, cache-reporting presence, tool execution time and deep-copy regressions
// POS: [L1 kernel contracts] Ensures call/effective input, usage and execution-time evidence cannot be rewritten or reversed.
package abi

import (
	"encoding/json"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestToolOutcomeExecutionTimesRoundTripAndOrder(t *testing.T) {
	started := time.Date(2026, 11, 7, 8, 0, 0, 123456789, time.UTC)
	ended := started.Add(1500 * time.Millisecond)
	outcome := func(start, end time.Time) SessionRecord {
		return BuiltinRecord(SessionRecord{Kind: RecordToolOutcome, ToolOutcome: &ToolOutcome{RunID: "run", CallID: "call", Status: OutcomeSucceeded, Result: wire.ToolResult{CallID: "call"}, StartedAt: start, EndedAt: end}})
	}
	record := outcome(started, ended)
	if err := record.Validate(); err != nil {
		t.Fatal(err)
	}
	data, err := json.Marshal(record)
	if err != nil {
		t.Fatal(err)
	}
	var decoded SessionRecord
	if err = json.Unmarshal(data, &decoded); err != nil {
		t.Fatal(err)
	}
	if !decoded.ToolOutcome.StartedAt.Equal(started) || !decoded.ToolOutcome.EndedAt.Equal(ended) {
		t.Fatal("execution times lost in JSON", string(data))
	}
	if cloned := CloneRecords([]SessionRecord{record})[0]; !cloned.ToolOutcome.StartedAt.Equal(started) || !cloned.ToolOutcome.EndedAt.Equal(ended) {
		t.Fatal("deep clone dropped execution times")
	}
	// Records that never executed carry no invented instants and keep the
	// pre-existing JSON shape.
	unexecuted, err := json.Marshal(outcome(time.Time{}, time.Time{}))
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(unexecuted), "started_at") || strings.Contains(string(unexecuted), "ended_at") {
		t.Fatal("zero execution times serialized", string(unexecuted))
	}
	var legacy SessionRecord
	if err = json.Unmarshal(unexecuted, &legacy); err != nil || legacy.Validate() != nil || !legacy.ToolOutcome.StartedAt.IsZero() || !legacy.ToolOutcome.EndedAt.IsZero() {
		t.Fatal("outcome without times no longer reads", err)
	}
	if err := outcome(ended, started).Validate(); err == nil {
		t.Fatal("outcome ending before it started accepted")
	}
	for _, partial := range []SessionRecord{outcome(started, time.Time{}), outcome(time.Time{}, ended), outcome(started, started)} {
		if err := partial.Validate(); err != nil {
			t.Fatal("non-reversed execution time rejected", err)
		}
	}
}

func TestPreparedIntentIdentityAndDeepClone(t *testing.T) {
	declared := wire.ToolCall{ID: "call", Name: "tool", Arguments: json.RawMessage(`{"original":true}`), ProviderState: []wire.ProviderState{{Provider: "p", Data: json.RawMessage(`{"signature":1}`)}}}
	effective := wire.CloneToolCall(declared)
	effective.Arguments = json.RawMessage(`{"effective":true}`)
	record := BuiltinRecord(SessionRecord{Kind: RecordToolIntent, ToolIntent: &ToolIntent{RunID: "run", Call: effective, DeclaredCall: &declared, NoSideEffect: true}})
	if err := record.Validate(); err != nil {
		t.Fatal(err)
	}
	cloned := CloneRecords([]SessionRecord{record})
	cloned[0].ToolIntent.DeclaredCall.Arguments[0] = 'x'
	cloned[0].ToolIntent.DeclaredCall.ProviderState[0].Data[0] = 'x'
	cloned[0].ToolIntent.Call.Arguments[0] = 'x'
	if !wire.ValidJSON(record.ToolIntent.DeclaredCall.Arguments) || !wire.ValidJSON(record.ToolIntent.DeclaredCall.ProviderState[0].Data) || !wire.ValidJSON(record.ToolIntent.Call.Arguments) {
		t.Fatal("clone rewrote original intent evidence")
	}
	record.ToolIntent.DeclaredCall.ID = "different"
	if err := record.Validate(); err == nil {
		t.Fatal("preparation changed call identity")
	}
}

func TestCacheUsageCloneAcrossEventsAndModelFacts(t *testing.T) {
	cached, written := uint64(0), uint64(2)
	usage := &wire.Usage{CachedInputTokens: &cached, CacheWriteInputTokens: &written}
	clonedEvent := CloneEvent(Event{Kind: EventUsage, Usage: usage})
	clonedRecord := CloneRecords([]SessionRecord{BuiltinRecord(SessionRecord{Kind: RecordModelOutcome, ModelOutcome: &ModelOutcome{RequestID: "request", Status: OutcomeSucceeded, Usage: usage}})})[0]
	cached, written = 100, 200
	for _, detached := range []*wire.Usage{clonedEvent.Usage, clonedRecord.ModelOutcome.Usage} {
		if detached.CachedInputTokens == nil || *detached.CachedInputTokens != 0 || detached.CacheWriteInputTokens == nil || *detached.CacheWriteInputTokens != 2 {
			t.Fatal("committed/queued cache evidence shares caller memory", detached)
		}
	}
}
