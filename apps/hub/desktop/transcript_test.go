// INPUT: Committed engine/product event pages and presentation projection
// OUTPUT: Stable message identity, distinct kinds and overlap paging regression
// POS: [L7 Hub product] Pure UI conversion tests independent of native windows and product mutation
package hubdesktop

import (
	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	"testing"
)

func TestTranscriptStableIDsPagingAndDistinctTypes(t *testing.T) {
	entries := []hub.HistoryEntry{{Sequence: 1, TurnID: "turn", Input: "user"}, {Sequence: 2, AttemptID: "attempt", Event: engine.Event{Kind: engine.MessageDelta, ID: "assistant", Text: "a"}}, {Sequence: 3, AttemptID: "attempt", Event: engine.Event{Kind: engine.MessageDelta, ID: "assistant", Text: "b"}}, {Sequence: 4, AttemptID: "attempt", Event: engine.Event{Kind: engine.ToolStart, ID: "tool", Name: "read"}}, {Sequence: 5, AttemptID: "attempt", Event: engine.Event{Kind: engine.ToolEnd, ID: "tool", Name: "read", Text: "small ref", Reference: &engine.Reference{SessionID: "native", RecordID: "record"}}}, {Sequence: 6, AttemptID: "attempt", Event: engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded}}}
	var transcript Transcript
	transcript.Replace(entries[:3])
	id := transcript.Rows[1].ID
	transcript.Append(entries[2:])
	if len(transcript.Rows) != 3 || transcript.Rows[1].ID != id || transcript.Rows[1].Text != "ab" || transcript.Rows[1].Status != "completed" || transcript.Rows[2].Text != "small ref" || transcript.tools["attempt/tool"].reference.RecordID != "record" {
		t.Fatalf("unstable/duplicate projection: %+v", transcript.Rows)
	}
	transcript.Replace(entries[3:])
	transcript.Prepend(entries[:4])
	if len(transcript.Entries) != 6 || transcript.First != 1 || transcript.Last != 6 {
		t.Fatal("page overlap duplicated committed events")
	}
}

func TestTranscriptKeepsOriginalResultAndCopiesTypedReference(t *testing.T) {
	reference := &engine.Reference{SessionID: "native", RecordID: "result", Sequence: 42}
	entries := []hub.HistoryEntry{
		{Sequence: 1, SessionID: "product", AttemptID: "attempt", Event: engine.Event{Kind: engine.ToolStart, ID: "call", Name: "read", Text: "original start"}},
		{Sequence: 2, SessionID: "product", AttemptID: "attempt", Event: engine.Event{Kind: engine.ToolEnd, ID: "call", Text: "original failure", IsError: true, Reference: reference}},
	}
	var transcript Transcript
	transcript.Replace(entries)
	reference.RecordID = "mutated source"
	row, tool := transcript.Rows[0], transcript.tools["attempt/call"]
	if row.Text != "original failure" || row.Status != "failed" || tool.name != "read" || tool.reference.RecordID != "result" || tool.sessionID != "product" {
		t.Fatal("result/reference presentation changed its source or lost identity", row, tool)
	}
	transcript.Replace(nil)
	if len(transcript.tools) != 0 {
		t.Fatal("replaced history retained stale tool references")
	}
}

func TestTranscriptToolEndKeepsNativeOwnerWithoutInventingResultReference(t *testing.T) {
	for _, source := range []string{"event", "start-reference"} {
		t.Run(source, func(t *testing.T) {
			start := engine.Event{Kind: engine.ToolStart, ID: "call", Name: "read"}
			if source == "event" {
				start.SessionID = "native-a"
			} else {
				start.Reference = &engine.Reference{SessionID: "native-a", RecordID: "tool-start", Sequence: 42}
			}
			var transcript Transcript
			transcript.Replace([]hub.HistoryEntry{
				{Sequence: 1, SessionID: "product", AttemptID: "attempt", Event: start},
				{Sequence: 2, SessionID: "product", AttemptID: "attempt", Event: engine.Event{Kind: engine.ToolEnd, ID: "call", Text: "tool cancelled before settlement", IsError: true}},
			})
			tool := transcript.tools["attempt/call"]
			if tool.nativeID != "native-a" || tool.name != "read" || tool.hasRef || tool.reference != (engine.Reference{}) || transcript.Rows[0].Status != "failed" {
				t.Fatal("synthetic ToolEnd lost its original owner or promoted a start reference to a result", tool, transcript.Rows)
			}
		})
	}
}

func TestTranscriptToolOwnerDoesNotCrossAttemptOrProductSession(t *testing.T) {
	for _, other := range []hub.HistoryEntry{
		{SessionID: "product", AttemptID: "different-attempt"},
		{SessionID: "different-product", AttemptID: "attempt"},
	} {
		var transcript Transcript
		other.Sequence = 2
		other.Event = engine.Event{Kind: engine.ToolEnd, ID: "call", IsError: true}
		transcript.Replace([]hub.HistoryEntry{
			{Sequence: 1, SessionID: "product", AttemptID: "attempt", Event: engine.Event{Kind: engine.ToolStart, ID: "call", SessionID: "native-a"}},
			other,
		})
		if tool := transcript.tools[other.AttemptID+"/call"]; tool.nativeID != "" || tool.hasRef {
			t.Fatal("unrelated tool inherited a native owner", other, tool)
		}
	}
}

func TestTranscriptNativeOriginUsesOnlyRetainedCommittedAttemptFacts(t *testing.T) {
	var transcript Transcript
	reply := hub.HistoryEntry{Sequence: 2, SessionID: "session", AttemptID: "attempt", Event: engine.Event{Kind: engine.Message, ID: "reply", Text: "retained response"}}
	transcript.Replace([]hub.HistoryEntry{reply})
	id := transcript.Rows[0].ID
	if len(transcript.origins) != 0 {
		t.Fatal("projection invented native identity without a retained event")
	}
	transcript.Prepend([]hub.HistoryEntry{{Sequence: 1, SessionID: "session", AttemptID: "attempt", Event: engine.Event{Kind: engine.SessionStarted, SessionID: "actual-native"}}})
	origin := transcript.origins["attempt"]
	if transcript.Rows[0].ID != id || origin.sessionID != "session" || origin.nativeID != "actual-native" || origin.ambiguous {
		t.Fatal("older committed origin changed row identity or lost its original scope", transcript.Rows, origin)
	}
	transcript.Append([]hub.HistoryEntry{{Sequence: 3, SessionID: "session", AttemptID: "attempt", Event: engine.Event{Kind: engine.SessionStarted, SessionID: "conflicting-native"}}})
	if !transcript.origins["attempt"].ambiguous {
		t.Fatal("conflicting native identities silently picked one model source")
	}
	transcript.Replace(nil)
	if len(transcript.origins) != 0 {
		t.Fatal("replaced history retained the former attempt's identity")
	}
}
