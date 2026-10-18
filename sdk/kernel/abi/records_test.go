// INPUT: encoding/json, testing, llm/wire
// OUTPUT: Prepared intent identity, cache-reporting presence and deep-copy regressions
// POS: [L1 kernel contracts] Ensures call/effective input and usage evidence cannot be rewritten through shared bytes/pointers.
package abi

import (
	"encoding/json"
	"testing"

	"github.com/QuincySx/Alva/sdk/llm/wire"
)

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
