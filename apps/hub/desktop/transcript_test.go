// INPUT: Committed engine/product event pages and presentation projection
// OUTPUT: Stable message identity, distinct kinds and overlap paging regression
// POS: [L7 Hub product] Pure UI conversion tests independent of native windows and product mutation
package hubdesktop

import (
	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	"strings"
	"testing"
)

func TestTranscriptStableIDsPagingAndDistinctTypes(t *testing.T) {
	entries := []hub.HistoryEntry{{Sequence: 1, TurnID: "turn", Input: "user"}, {Sequence: 2, AttemptID: "attempt", Event: engine.Event{Kind: engine.MessageDelta, ID: "assistant", Text: "a"}}, {Sequence: 3, AttemptID: "attempt", Event: engine.Event{Kind: engine.MessageDelta, ID: "assistant", Text: "b"}}, {Sequence: 4, AttemptID: "attempt", Event: engine.Event{Kind: engine.ToolStart, ID: "tool", Name: "read"}}, {Sequence: 5, AttemptID: "attempt", Event: engine.Event{Kind: engine.ToolEnd, ID: "tool", Name: "read", Text: "small ref", Reference: &engine.Reference{SessionID: "native", RecordID: "record"}}}, {Sequence: 6, AttemptID: "attempt", Event: engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded}}}
	var transcript Transcript
	transcript.Replace(entries[:3])
	id := transcript.Rows[1].ID
	transcript.Append(entries[2:])
	if len(transcript.Rows) != 3 || transcript.Rows[1].ID != id || transcript.Rows[1].Text != "ab" || transcript.Rows[1].Status != "completed" || !strings.Contains(transcript.Rows[2].Text, "record") {
		t.Fatalf("unstable/duplicate projection: %+v", transcript.Rows)
	}
	transcript.Replace(entries[3:])
	transcript.Prepend(entries[:4])
	if len(transcript.Entries) != 6 || transcript.First != 1 || transcript.Last != 6 {
		t.Fatal("page overlap duplicated committed events")
	}
}
