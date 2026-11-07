// INPUT: Actual private product SQLite, committed engine events and directly inserted legacy rows
// OUTPUT: json_extract availability, same-transaction delta deletion, truncation retention and read-time Turn sequence regressions
// POS: [L7 Hub product] Final messages supersede derived deltas without rewriting older rows or the stored event body
package sqlite

import (
	"context"
	"encoding/json"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

func compactionHistory(t *testing.T, s *Store, session string) []hub.HistoryEntry {
	t.Helper()
	var all []hub.HistoryEntry
	var after uint64
	for {
		page, err := s.History(context.Background(), session, after, 128)
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

func compactionCount(entries []hub.HistoryEntry, kind engine.Kind, id string) int {
	count := 0
	for _, entry := range entries {
		if entry.Event.Kind == kind && entry.Event.ID == id {
			count++
		}
	}
	return count
}

func compactionRecord(t *testing.T, s *Store, attempt string, events ...engine.Event) {
	t.Helper()
	for _, event := range events {
		if err := s.Record(context.Background(), attempt, event, false); err != nil {
			t.Fatal(event.Kind, err)
		}
	}
}

func TestJSONExtractReadsStoredEventBodies(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	session, err := s.CreateSession(ctx, "json", t.TempDir(), target("a"))
	if err != nil {
		t.Fatal(err)
	}
	claim, err := s.BeginTurn(ctx, session.ID, "probe")
	if err != nil {
		t.Fatal(err)
	}
	compactionRecord(t, s, claim.Attempt.ID, engine.Event{Kind: engine.SessionStarted, SessionID: "native"}, engine.Event{Kind: engine.MessageDelta, SessionID: "native", ID: "assistant-0", Text: "probe"})
	var kind, id string
	if err = s.db.QueryRowContext(ctx, `SELECT json_extract(data,'$.event.kind'),json_extract(data,'$.event.id') FROM hub_events WHERE turn=? ORDER BY seq DESC LIMIT 1`, claim.Turn.ID).Scan(&kind, &id); err != nil {
		t.Fatal("json_extract is unavailable for stored event bodies", err)
	}
	if kind != "message_delta" || id != "assistant-0" {
		t.Fatal("json_extract read the wrong event fields", kind, id)
	}
	// The deletion must stay a bounded index range, not a scan of older rows.
	rows, err := s.db.QueryContext(ctx, `EXPLAIN QUERY PLAN DELETE FROM hub_events WHERE turn=? AND seq<? AND attempt=? AND json_extract(data,'$.event.kind')=? AND json_extract(data,'$.event.id')=?`, claim.Turn.ID, 1<<40, claim.Attempt.ID, "message_delta", "assistant-0")
	if err != nil {
		t.Fatal(err)
	}
	defer rows.Close()
	var plan []string
	for rows.Next() {
		var a, b, c int
		var detail string
		if err := rows.Scan(&a, &b, &c, &detail); err != nil {
			t.Fatal(err)
		}
		plan = append(plan, detail)
	}
	if len(plan) == 0 || !strings.Contains(strings.Join(plan, ";"), "hub_events_turn") {
		t.Fatal("delta deletion does not use the turn index", plan)
	}
}

func TestFinalEventsDeleteOnlyTheirMergedDeltas(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	session, err := s.CreateSession(ctx, "compaction", t.TempDir(), target("a"))
	if err != nil {
		t.Fatal(err)
	}
	// An earlier failed Turn reuses the same per-attempt delta identity.
	earlier, err := s.BeginTurn(ctx, session.ID, "earlier")
	if err != nil {
		t.Fatal(err)
	}
	compactionRecord(t, s, earlier.Attempt.ID,
		engine.Event{Kind: engine.SessionStarted, SessionID: "native"},
		engine.Event{Kind: engine.MessageDelta, SessionID: "native", ID: "assistant-0", Text: "earlier partial"},
		engine.Event{Kind: engine.Completed, SessionID: "native", Outcome: engine.Failed})
	if err = s.FinishTurn(ctx, earlier.Turn.ID, engine.Failed); err != nil {
		t.Fatal(err)
	}
	claim, err := s.BeginTurn(ctx, session.ID, "current")
	if err != nil {
		t.Fatal(err)
	}
	attempt := claim.Attempt.ID
	compactionRecord(t, s, attempt,
		engine.Event{Kind: engine.SessionStarted, SessionID: "native"},
		engine.Event{Kind: engine.ReasoningDelta, SessionID: "native", ID: "reasoning-0", Text: "think "},
		engine.Event{Kind: engine.ReasoningDelta, SessionID: "native", ID: "reasoning-0", Text: "more"},
		engine.Event{Kind: engine.MessageDelta, SessionID: "native", ID: "assistant-0", Text: "hel"},
		engine.Event{Kind: engine.MessageDelta, SessionID: "native", ID: "assistant-0", Text: "lo"},
		engine.Event{Kind: engine.MessageDelta, SessionID: "native", ID: "assistant-1", Text: "tool preface"},
		engine.Event{Kind: engine.ToolStart, SessionID: "native", ID: "call", Name: "read", Effect: engine.EffectRead},
		engine.Event{Kind: engine.ToolEnd, SessionID: "native", ID: "call", Name: "read", Text: "result"})
	before := compactionHistory(t, s, session.ID)
	if compactionCount(before, engine.MessageDelta, "assistant-0") != 3 || compactionCount(before, engine.ReasoningDelta, "reasoning-0") != 2 {
		t.Fatal("fixture deltas were not committed")
	}
	compactionRecord(t, s, attempt, engine.Event{Kind: engine.Message, SessionID: "native", ID: "assistant-0", Text: "hello"})
	after := compactionHistory(t, s, session.ID)
	current := 0
	for _, entry := range after {
		if entry.TurnID == claim.Turn.ID && entry.Event.Kind == engine.MessageDelta && entry.Event.ID == "assistant-0" {
			current++
		}
	}
	if current != 0 || compactionCount(after, engine.MessageDelta, "assistant-0") != 1 || compactionCount(after, engine.MessageDelta, "assistant-1") != 1 || compactionCount(after, engine.ReasoningDelta, "reasoning-0") != 2 || compactionCount(after, engine.ToolEnd, "call") != 1 {
		t.Fatal("final message deleted the wrong rows", current, len(before), len(after))
	}
	reply := ""
	for _, entry := range after {
		if entry.Event.Kind == engine.Message && entry.Event.ID == "assistant-0" {
			reply = entry.Event.Text
		}
	}
	if reply != "hello" {
		t.Fatal("complete reply is no longer readable", reply)
	}
	// Reasoning follows the same rule through its final content block.
	compactionRecord(t, s, attempt, engine.Event{Kind: engine.ContentBlock, SessionID: "native", ID: "reasoning-0", Name: "reasoning", Text: "think more"})
	if compactionCount(compactionHistory(t, s, session.ID), engine.ReasoningDelta, "reasoning-0") != 0 {
		t.Fatal("final reasoning block kept its merged deltas")
	}
	compactionRecord(t, s, attempt, engine.Event{Kind: engine.Completed, SessionID: "native", Outcome: engine.Succeeded})
	if err = s.FinishTurn(ctx, claim.Turn.ID, engine.Succeeded); err != nil {
		t.Fatal(err)
	}
}

func TestTruncatedFinalEventKeepsDeltasUnlessReferenced(t *testing.T) {
	for _, tc := range []struct {
		name       string
		referenced bool
		kept       int
	}{
		{"truncated-without-reference", false, 2},
		{"truncated-with-reference", true, 0},
	} {
		t.Run(tc.name, func(t *testing.T) {
			s := newStore(t)
			ctx := context.Background()
			session, err := s.CreateSession(ctx, "truncated", t.TempDir(), target("a"))
			if err != nil {
				t.Fatal(err)
			}
			claim, err := s.BeginTurn(ctx, session.ID, "long")
			if err != nil {
				t.Fatal(err)
			}
			var reference *engine.Reference
			if tc.referenced {
				reference = &engine.Reference{SessionID: "native", RecordID: "record"}
			}
			compactionRecord(t, s, claim.Attempt.ID,
				engine.Event{Kind: engine.SessionStarted, SessionID: "native"},
				engine.Event{Kind: engine.MessageDelta, SessionID: "native", ID: "assistant-0", Text: "full "},
				engine.Event{Kind: engine.MessageDelta, SessionID: "native", ID: "assistant-0", Text: "text"},
				engine.Event{Kind: engine.ReasoningDelta, SessionID: "native", ID: "reasoning-0", Text: "full "},
				engine.Event{Kind: engine.ReasoningDelta, SessionID: "native", ID: "reasoning-0", Text: "thought"},
				engine.Event{Kind: engine.Message, SessionID: "native", ID: "assistant-0", Text: "full", Truncated: true, Reference: reference},
				engine.Event{Kind: engine.ContentBlock, SessionID: "native", ID: "reasoning-0", Name: "reasoning", Text: "full", Truncated: true})
			entries := compactionHistory(t, s, session.ID)
			if compactionCount(entries, engine.MessageDelta, "assistant-0") != tc.kept {
				t.Fatal("truncated message delta retention is wrong", compactionCount(entries, engine.MessageDelta, "assistant-0"))
			}
			// The unreferenced truncated reasoning block always keeps its deltas.
			if compactionCount(entries, engine.ReasoningDelta, "reasoning-0") != 2 {
				t.Fatal("unreferenced truncated reasoning lost its only complete text")
			}
		})
	}
}

func TestHistoryReadsTurnSequenceAndLegacyRows(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	session, err := s.CreateSession(ctx, "legacy", t.TempDir(), target("a"))
	if err != nil {
		t.Fatal(err)
	}
	var turns []hub.Claim
	for _, prompt := range []string{"first", "second"} {
		claim, err := s.BeginTurn(ctx, session.ID, prompt)
		if err != nil {
			t.Fatal(err)
		}
		complete(t, s, claim, "native")
		turns = append(turns, claim)
	}
	// Legacy rows: deltas that were never merged or deleted after their message,
	// plus a turn-less notice. They are read as they are, without rewriting.
	legacy := []hub.HistoryEntry{
		{SessionID: session.ID, TurnID: turns[1].Turn.ID, AttemptID: turns[1].Attempt.ID, Event: engine.Event{Kind: engine.MessageDelta, ID: "assistant-legacy", Text: "old "}},
		{SessionID: session.ID, TurnID: turns[1].Turn.ID, AttemptID: turns[1].Attempt.ID, Event: engine.Event{Kind: engine.MessageDelta, ID: "assistant-legacy", Text: "reply"}},
		{SessionID: session.ID, TurnID: turns[1].Turn.ID, AttemptID: turns[1].Attempt.ID, Event: engine.Event{Kind: engine.Message, ID: "assistant-legacy", Text: "old reply"}},
	}
	for _, entry := range legacy {
		data, err := json.Marshal(entry)
		if err != nil {
			t.Fatal(err)
		}
		if _, err = s.db.ExecContext(ctx, `INSERT INTO hub_events(session,turn,attempt,data) VALUES(?,?,?,?)`, session.ID, entry.TurnID, entry.AttemptID, data); err != nil {
			t.Fatal(err)
		}
	}
	notice, _ := json.Marshal(hub.HistoryEntry{SessionID: session.ID, Timeline: &hub.TimelineEvent{Kind: "worker_outcome", Text: "turn-less notice"}})
	if _, err = s.db.ExecContext(ctx, `INSERT INTO hub_events(session,data) VALUES(?,?)`, session.ID, notice); err != nil {
		t.Fatal(err)
	}
	forward := compactionHistory(t, s, session.ID)
	backward, err := s.HistoryBefore(ctx, session.ID, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	if len(forward) != len(backward) {
		t.Fatal("forward and backward pages disagree", len(forward), len(backward))
	}
	for i, entry := range forward {
		want := uint64(0)
		for _, claim := range turns {
			if entry.TurnID == claim.Turn.ID {
				want = claim.Turn.Sequence
			}
		}
		if entry.TurnSequence != want || backward[i].TurnSequence != want || backward[i].Sequence != entry.Sequence {
			t.Fatal("history page lost the read-time Turn sequence", entry.Sequence, entry.TurnSequence, want)
		}
	}
	if compactionCount(forward, engine.MessageDelta, "assistant-legacy") != 2 || turns[0].Turn.Sequence != 1 || turns[1].Turn.Sequence != 2 {
		t.Fatal("legacy unmerged rows were not read unchanged")
	}
	var stored int
	if err = s.db.QueryRowContext(ctx, `SELECT count(*) FROM hub_events WHERE instr(CAST(data AS TEXT),'turn_sequence')>0`).Scan(&stored); err != nil || stored != 0 {
		t.Fatal("Turn sequence leaked into stored event bodies", stored, err)
	}
	last := forward[len(forward)-1]
	if last.Timeline == nil || !strings.Contains(last.Timeline.Text, "turn-less") || last.TurnSequence != 0 {
		t.Fatal("turn-less row did not keep a zero Turn sequence", last)
	}
}
