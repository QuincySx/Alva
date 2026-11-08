// INPUT: testing and the engine context report contract
// OUTPUT: ContextReport validation and stream placement regressions
// POS: [L4.5 engine contract testing] Reports stay measured, bounded and attached only to context updates.
package engine

import (
	"strings"
	"testing"
)

func TestContextReportValidation(t *testing.T) {
	n := func(v uint64) *uint64 { return &v }
	valid := []ContextReport{
		{Usage: &ContextUsage{InputTokens: 900, OutputTokens: 100, CachedInputTokens: n(300), WindowTokens: 128_000, Parts: &ContextParts{System: 100, Tools: 200, Summary: 300, Recent: 400}}},
		{Usage: &ContextUsage{InputTokens: 10}},
		{Compaction: &ContextCompaction{Status: ContextCompacted, Trigger: "auto", FromSequence: 1, ThroughSequence: 40, Messages: 30, Reference: &Reference{SessionID: "native", RecordID: "native:41", Sequence: 41}, Usage: &Usage{InputTokens: 50, OutputTokens: 9}}},
		{Compaction: &ContextCompaction{Status: ContextCompactionFailed, Trigger: "overflow", Error: "provider down"}},
		{Compaction: &ContextCompaction{Status: ContextSummaryCorrupt, Trigger: "auto"}},
		{Compaction: &ContextCompaction{Status: ContextCompactionSkipped, Trigger: "manual"}},
	}
	for i, report := range valid {
		if err := report.Validate(); err != nil {
			t.Fatal(i, err)
		}
	}
	invalid := []ContextReport{
		{},
		{Usage: &ContextUsage{}},
		{Usage: &ContextUsage{InputTokens: 10, CachedInputTokens: n(11)}},
		{Usage: &ContextUsage{InputTokens: 900, OutputTokens: 100, Parts: &ContextParts{System: 1}}},
		{Compaction: &ContextCompaction{Status: ContextCompacted, FromSequence: 5, ThroughSequence: 4, Messages: 1, Reference: &Reference{}}},
		{Compaction: &ContextCompaction{Status: ContextCompacted, FromSequence: 1, ThroughSequence: 4, Messages: 1}},
		{Compaction: &ContextCompaction{Status: "invented"}},
		{Compaction: &ContextCompaction{Status: ContextCompactionFailed, Error: strings.Repeat("e", 2<<10)}},
		{Compaction: &ContextCompaction{Status: ContextCompactionFailed, Usage: &Usage{InputTokens: 1, CachedInputTokens: n(2)}}},
	}
	for i, report := range invalid {
		if report.Validate() == nil {
			t.Fatal("accepted invalid report", i)
		}
	}
}

func TestContextReportBelongsOnlyToContextUpdates(t *testing.T) {
	report := &ContextReport{Usage: &ContextUsage{InputTokens: 10}}
	contract := StreamContract{}
	if err := contract.Accept(Event{Kind: SessionStarted, SessionID: "native"}); err != nil {
		t.Fatal(err)
	}
	if err := contract.Accept(Event{Kind: MessageDelta, ID: "m", Text: "x", Context: report}); err == nil {
		t.Fatal("context report attached to a message")
	}
	if err := contract.Accept(Event{Kind: ContextUpdated}); err == nil {
		t.Fatal("context update without a report")
	}
	if err := contract.Accept(Event{Kind: ContextUpdated, Context: &ContextReport{}}); err == nil {
		t.Fatal("empty context report accepted")
	}
	if err := contract.Accept(Event{Kind: ContextUpdated, SessionID: "native", Context: report}); err != nil {
		t.Fatal(err)
	}
	if err := contract.Accept(Event{Kind: Completed, Outcome: Succeeded}); err != nil {
		t.Fatal(err)
	}
	if (ContextUsage{InputTokens: 7, OutputTokens: 3}).UsedTokens() != 10 {
		t.Fatal("used tokens is input plus output")
	}
}
