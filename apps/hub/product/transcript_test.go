// INPUT: Synthetic committed history pages, an in-memory TranscriptStore and the product projection
// OUTPUT: Migrated desktop identity/tool/origin cases plus streaming, final replacement, boundary merge, trimming, legacy and paging regressions
// POS: [L7 Hub product] Pure Message Stream tests independent of MyGo, native windows and product mutation
package product

import (
	"context"
	"fmt"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

func chunkOf(entries ...hub.HistoryEntry) TranscriptChunk {
	chunk := TranscriptChunk{entries: append([]hub.HistoryEntry(nil), entries...)}
	chunk.bounds()
	return chunk
}

func blockByID(t *testing.T, w *TranscriptWindow, id string) Block {
	t.Helper()
	for _, block := range w.Blocks() {
		if block.ID == id {
			return block
		}
	}
	t.Fatalf("block %q missing from %+v", id, w.Blocks())
	return Block{}
}

func uniqueIDs(t *testing.T, w *TranscriptWindow) {
	t.Helper()
	seen := map[string]bool{}
	for _, block := range w.Blocks() {
		if seen[block.ID] {
			t.Fatal("duplicate block identity", block.ID)
		}
		seen[block.ID] = true
	}
}

// memoryHistory serves committed pages exactly as the SQLite store does.
type memoryHistory struct {
	entries []hub.HistoryEntry
	calls   int
}

func (m *memoryHistory) History(_ context.Context, _ string, after uint64, limit int) ([]hub.HistoryEntry, error) {
	m.calls++
	var page []hub.HistoryEntry
	for _, entry := range m.entries {
		if entry.Sequence > after && len(page) < limit {
			page = append(page, entry)
		}
	}
	return page, nil
}

func (m *memoryHistory) HistoryBefore(_ context.Context, _ string, before uint64, limit int) ([]hub.HistoryEntry, error) {
	m.calls++
	var page []hub.HistoryEntry
	for i := len(m.entries) - 1; i >= 0 && len(page) < limit; i-- {
		if before == 0 || m.entries[i].Sequence < before {
			page = append([]hub.HistoryEntry{m.entries[i]}, page...)
		}
	}
	return page, nil
}

func TestTranscriptStableIDsPagingAndDistinctTypes(t *testing.T) {
	entries := []hub.HistoryEntry{{Sequence: 1, TurnID: "turn", Input: "user"}, {Sequence: 2, AttemptID: "attempt", Event: engine.Event{Kind: engine.MessageDelta, ID: "assistant", Text: "a"}}, {Sequence: 3, AttemptID: "attempt", Event: engine.Event{Kind: engine.MessageDelta, ID: "assistant", Text: "b"}}, {Sequence: 4, AttemptID: "attempt", Event: engine.Event{Kind: engine.ToolStart, ID: "tool", Name: "read"}}, {Sequence: 5, AttemptID: "attempt", Event: engine.Event{Kind: engine.ToolEnd, ID: "tool", Name: "read", Text: "small ref", Reference: &engine.Reference{SessionID: "native", RecordID: "record"}}}, {Sequence: 6, AttemptID: "attempt", Event: engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded}}}
	w := NewTranscriptWindow(0)
	w.Reset(chunkOf(entries[:3]...))
	id := w.Blocks()[1].ID
	w.ApplyNewer(chunkOf(entries[2:]...))
	blocks := w.Blocks()
	if len(blocks) != 3 || blocks[1].ID != id || blocks[1].Kind != BlockKindReply || blocks[1].Text != "ab" || blocks[1].Status != BlockStatusCompleted || blocks[2].Kind != BlockKindTool || blocks[2].Text != "small ref" || blocks[2].Ref == nil || blocks[2].Ref.RecordID != "record" || blocks[0].Kind != BlockKindUser {
		t.Fatalf("unstable/duplicate projection: %+v", blocks)
	}
	w.Reset(chunkOf(entries[3:]...))
	w.ApplyOlder(chunkOf(entries[:4]...))
	uniqueIDs(t, w)
	if len(w.Blocks()) != 3 || w.First() != 1 || w.Last() != 6 || blockByID(t, w, "attempt/assistant").Status != BlockStatusCompleted {
		t.Fatal("page overlap duplicated committed events or lost the settled status", w.Blocks(), w.First(), w.Last())
	}
}

func TestTranscriptKeepsOriginalResultAndCopiesTypedReference(t *testing.T) {
	reference := &engine.Reference{SessionID: "native", RecordID: "result", Sequence: 42}
	entries := []hub.HistoryEntry{
		{Sequence: 1, SessionID: "product", AttemptID: "attempt", Event: engine.Event{Kind: engine.ToolStart, ID: "call", Name: "read", Text: "original start"}},
		{Sequence: 2, SessionID: "product", AttemptID: "attempt", Event: engine.Event{Kind: engine.ToolEnd, ID: "call", Text: "original failure", IsError: true, Reference: reference}},
	}
	w := NewTranscriptWindow(0)
	w.Reset(chunkOf(entries...))
	reference.RecordID = "mutated source"
	tool := w.Blocks()[0]
	if tool.Text != "original failure" || tool.Status != BlockStatusFailed || tool.Name != "read" || tool.Ref.RecordID != "result" || tool.SessionID != "product" {
		t.Fatal("result/reference presentation changed its source or lost identity", tool)
	}
	tool.Ref.RecordID = "caller mutation"
	if w.Blocks()[0].Ref.RecordID != "result" {
		t.Fatal("returned blocks alias the window's references")
	}
	w.Reset(TranscriptChunk{})
	if len(w.Blocks()) != 0 || w.First() != 0 || w.Last() != 0 {
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
			w := NewTranscriptWindow(0)
			w.Reset(chunkOf(
				hub.HistoryEntry{Sequence: 1, SessionID: "product", AttemptID: "attempt", Event: start},
				hub.HistoryEntry{Sequence: 2, SessionID: "product", AttemptID: "attempt", Event: engine.Event{Kind: engine.ToolEnd, ID: "call", Text: "tool cancelled before settlement", IsError: true}},
			))
			tool := w.Blocks()[0]
			if tool.NativeSessionID != "native-a" || tool.Name != "read" || tool.Ref != nil || tool.Status != BlockStatusFailed {
				t.Fatal("synthetic ToolEnd lost its original owner or promoted a start reference to a result", tool)
			}
		})
	}
}

func TestTranscriptToolOwnerDoesNotCrossAttemptOrProductSession(t *testing.T) {
	for _, other := range []hub.HistoryEntry{
		{SessionID: "product", AttemptID: "different-attempt"},
		{SessionID: "different-product", AttemptID: "attempt"},
	} {
		other.Sequence = 2
		other.Event = engine.Event{Kind: engine.ToolEnd, ID: "call", IsError: true}
		w := NewTranscriptWindow(0)
		w.Reset(chunkOf(hub.HistoryEntry{Sequence: 1, SessionID: "product", AttemptID: "attempt", Event: engine.Event{Kind: engine.ToolStart, ID: "call", SessionID: "native-a"}}, other))
		if tool := blockByID(t, w, other.AttemptID+"/call"); tool.NativeSessionID != "" || tool.Ref != nil {
			t.Fatal("unrelated tool inherited a native owner", other, tool)
		}
	}
}

func TestTranscriptNativeOriginUsesOnlyRetainedCommittedAttemptFacts(t *testing.T) {
	w := NewTranscriptWindow(0)
	w.Reset(chunkOf(hub.HistoryEntry{Sequence: 2, SessionID: "session", AttemptID: "attempt", Event: engine.Event{Kind: engine.Message, ID: "reply", Text: "retained response"}}))
	id := w.Blocks()[0].ID
	if w.Blocks()[0].NativeSessionID != "" || len(w.view.origins) != 0 {
		t.Fatal("projection invented native identity without a retained event")
	}
	w.ApplyOlder(chunkOf(hub.HistoryEntry{Sequence: 1, SessionID: "session", AttemptID: "attempt", Event: engine.Event{Kind: engine.SessionStarted, SessionID: "actual-native"}}))
	reply := w.Blocks()[0]
	if reply.ID != id || reply.SessionID != "session" || reply.NativeSessionID != "actual-native" || w.view.origins["attempt"].ambiguous {
		t.Fatal("older committed origin changed block identity or lost its original scope", w.Blocks())
	}
	w.ApplyNewer(chunkOf(hub.HistoryEntry{Sequence: 3, SessionID: "session", AttemptID: "attempt", Event: engine.Event{Kind: engine.SessionStarted, SessionID: "conflicting-native"}}))
	if !w.view.origins["attempt"].ambiguous || w.Blocks()[0].NativeSessionID != "" {
		t.Fatal("conflicting native identities silently picked one model source")
	}
	w.Reset(TranscriptChunk{})
	if len(w.view.origins) != 0 {
		t.Fatal("replaced history retained the former attempt's identity")
	}
	// A conflict that arrives from an older page is also ambiguous.
	w.Reset(chunkOf(hub.HistoryEntry{Sequence: 5, SessionID: "session", AttemptID: "a2", Event: engine.Event{Kind: engine.MessageDelta, SessionID: "native-b", ID: "reply", Text: "x"}}))
	w.ApplyOlder(chunkOf(hub.HistoryEntry{Sequence: 4, SessionID: "session", AttemptID: "a2", Event: engine.Event{Kind: engine.SessionStarted, SessionID: "native-a"}}))
	if w.Blocks()[0].NativeSessionID != "" {
		t.Fatal("older conflicting origin was ignored")
	}
}

func TestTranscriptStreamingKeepsUserBlocksAndCompleteText(t *testing.T) {
	base := []hub.HistoryEntry{
		{Sequence: 1, TurnID: "t0", AttemptID: "a0", TurnSequence: 1, Input: "第一条问题"},
		{Sequence: 2, TurnID: "t0", AttemptID: "a0", TurnSequence: 1, Event: engine.Event{Kind: engine.Message, ID: "assistant-0", Text: "第一条回答"}},
		{Sequence: 3, TurnID: "t0", AttemptID: "a0", TurnSequence: 1, Event: engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded}},
		{Sequence: 4, TurnID: "t1", AttemptID: "a1", TurnSequence: 2, Input: "第二条问题"},
	}
	w := NewTranscriptWindow(0)
	w.Reset(chunkOf(base...))
	seq := uint64(4)
	var expected strings.Builder
	for sent := 0; sent < 3000; {
		var batch []hub.HistoryEntry
		for i := 0; i < 128 && sent < 3000; i++ {
			seq++
			sent++
			text := fmt.Sprintf("%d,", sent)
			expected.WriteString(text)
			batch = append(batch, hub.HistoryEntry{Sequence: seq, TurnID: "t1", AttemptID: "a1", TurnSequence: 2, Event: engine.Event{Kind: engine.MessageDelta, ID: "assistant-0", Text: text}})
		}
		w.ApplyNewer(chunkOf(batch...))
		w.Trim(true)
	}
	blocks := w.Blocks()
	if len(blocks) != 4 || blocks[0].Text != "第一条问题" || blocks[1].Text != "第一条回答" || blocks[2].Text != "第二条问题" || blocks[3].Text != expected.String() || blocks[3].Status != BlockStatusRunning || blocks[3].Turn != 2 {
		t.Fatalf("streaming evicted the conversation or lost reply text: %d blocks, reply %d/%d bytes", len(blocks), len(blocks[len(blocks)-1].Text), expected.Len())
	}
	seq++
	w.ApplyNewer(chunkOf(hub.HistoryEntry{Sequence: seq, TurnID: "t1", AttemptID: "a1", TurnSequence: 2, Event: engine.Event{Kind: engine.Message, ID: "assistant-0", Text: "complete final", Truncated: true, Reference: &engine.Reference{SessionID: "native", RecordID: "final"}}}))
	reply := w.Blocks()[3]
	if reply.Text != "complete final" || reply.Status != BlockStatusCompleted || !reply.Truncated || reply.Ref == nil || reply.Ref.RecordID != "final" {
		t.Fatal("final message did not replace the streamed draft", reply)
	}
}

func TestTranscriptApplyOlderMergesBoundaryBlock(t *testing.T) {
	part := func(seq uint64, kind engine.Kind, text string) hub.HistoryEntry {
		return hub.HistoryEntry{Sequence: seq, TurnID: "t", AttemptID: "a", Event: engine.Event{Kind: kind, ID: "assistant-0", Text: text}}
	}
	w := NewTranscriptWindow(0)
	w.Reset(chunkOf(part(4, engine.MessageDelta, "C"), part(5, engine.MessageDelta, "D")))
	w.ApplyOlder(chunkOf(hub.HistoryEntry{Sequence: 1, TurnID: "t", AttemptID: "a", Input: "question"}, part(2, engine.MessageDelta, "A"), part(3, engine.MessageDelta, "B")))
	blocks := w.Blocks()
	if len(blocks) != 2 || blocks[0].Kind != BlockKindUser || blocks[1].ID != "a/assistant-0" || blocks[1].Text != "ABCD" || w.First() != 1 {
		t.Fatal("older draft text was not joined in front of the current block", blocks)
	}
	w.Reset(chunkOf(part(4, engine.MessageDelta, "C"), part(5, engine.Message, "final answer")))
	w.ApplyOlder(chunkOf(part(2, engine.MessageDelta, "A"), part(3, engine.MessageDelta, "B")))
	if blocks := w.Blocks(); len(blocks) != 1 || blocks[0].Text != "final answer" || blocks[0].Status != BlockStatusCompleted {
		t.Fatal("older drafts overwrote the current final text", blocks)
	}
}

func TestTranscriptLegacyUnmergedRowsStillProject(t *testing.T) {
	// Legacy history kept every delta beside the final message.
	legacy := []hub.HistoryEntry{{Sequence: 1, TurnID: "t", AttemptID: "a", Input: "old question"}}
	for i := 0; i < 300; i++ {
		legacy = append(legacy, hub.HistoryEntry{Sequence: uint64(2 + i), TurnID: "t", AttemptID: "a", Event: engine.Event{Kind: engine.MessageDelta, ID: "assistant-0", Text: "x"}})
	}
	legacy = append(legacy,
		hub.HistoryEntry{Sequence: 302, TurnID: "t", AttemptID: "a", Event: engine.Event{Kind: engine.Message, ID: "assistant-0", Text: strings.Repeat("x", 300)}},
		hub.HistoryEntry{Sequence: 303, TurnID: "t", AttemptID: "a", Event: engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded}})
	store := &memoryHistory{entries: legacy}
	service := &TranscriptService{Store: store}
	chunk, err := service.Tail(context.Background(), "session")
	if err != nil {
		t.Fatal(err)
	}
	w := NewTranscriptWindow(0)
	w.Reset(chunk)
	for w.MoreBefore() {
		older, err := service.Older(context.Background(), "session", w.First())
		if err != nil {
			t.Fatal(err)
		}
		w.ApplyOlder(older)
	}
	uniqueIDs(t, w)
	blocks := w.Blocks()
	if len(blocks) != 2 || blocks[0].Text != "old question" || blocks[1].Text != strings.Repeat("x", 300) || blocks[1].Status != BlockStatusCompleted {
		t.Fatal("legacy unmerged history did not project one complete reply", len(blocks))
	}
}

func TestTranscriptBlockCarriesTurnSequence(t *testing.T) {
	w := NewTranscriptWindow(0)
	w.Reset(chunkOf(
		hub.HistoryEntry{Sequence: 1, TurnID: "t7", AttemptID: "a", TurnSequence: 7, Input: "prompt"},
		hub.HistoryEntry{Sequence: 2, TurnID: "t7", AttemptID: "a", TurnSequence: 7, Event: engine.Event{Kind: engine.Message, ID: "m", Text: "answer"}},
		hub.HistoryEntry{Sequence: 3, Timeline: &hub.TimelineEvent{Kind: "worker_outcome", Text: "turn-less", HandoffID: "handoff-1"}},
	))
	blocks := w.Blocks()
	if blocks[0].Turn != 7 || blocks[1].Turn != 7 || blocks[2].Turn != 0 || blocks[2].Kind != BlockKindTimeline || blocks[2].HandoffID != "handoff-1" || blocks[2].Name != "worker_outcome" {
		t.Fatal("Turn sequence or timeline identity was not carried into blocks", blocks)
	}
}

func TestTranscriptStatusActivityAndSettlementBlocks(t *testing.T) {
	w := NewTranscriptWindow(0)
	w.Reset(chunkOf(
		hub.HistoryEntry{Sequence: 1, SessionID: "s", AttemptID: "a", Event: engine.Event{Kind: engine.ExecutionActivity, SessionID: "native", Activity: &engine.Activity{ID: "build", Name: "prepare", Effect: engine.EffectRead}}},
		hub.HistoryEntry{Sequence: 2, SessionID: "s", AttemptID: "a", Event: engine.Event{Kind: engine.ReasoningDelta, ID: "reasoning-0", Text: "think"}},
		hub.HistoryEntry{Sequence: 3, SessionID: "s", AttemptID: "a", Event: engine.Event{Kind: engine.ContentBlock, ID: "block-1", Name: "refusal", Text: "no"}},
		hub.HistoryEntry{Sequence: 4, SessionID: "s", AttemptID: "a", Event: engine.Event{Kind: engine.Error, Text: "retrying", Recoverable: true}},
		hub.HistoryEntry{Sequence: 5, SessionID: "s", AttemptID: "a", Event: engine.Event{Kind: engine.Error, Text: "fatal"}},
		hub.HistoryEntry{Sequence: 6, SessionID: "s", AttemptID: "a", Event: engine.Event{Kind: engine.Completed, Outcome: engine.Cancelled, Text: "settled by owner"}},
	))
	blocks := w.Blocks()
	if len(blocks) != 6 || blocks[0].Kind != BlockKindActivity || blocks[0].Name != "prepare" || !strings.Contains(blocks[0].Text, "effect=read") || blocks[0].NativeSessionID != "native" {
		t.Fatal("activity block lost its type or committed identity", blocks)
	}
	if blocks[1].Kind != BlockKindReasoning || blocks[1].Status != BlockStatusCancelled || blocks[2].Kind != BlockKindContent || blocks[2].Name != "refusal" || blocks[2].Status != BlockStatusCompleted {
		t.Fatal("reasoning/content blocks or Completed status propagation changed", blocks[1], blocks[2])
	}
	if blocks[3].Kind != BlockKindStatus || blocks[3].Name != "retry" || blocks[4].Name != "error" || blocks[4].Status != BlockStatusFailed || blocks[5].Name != "settlement" || blocks[5].Status != BlockStatusCancelled || blocks[5].Text != "settled by owner" {
		t.Fatal("error, retry warning or settlement status changed", blocks[3:])
	}
}

func turnEntries(seq *uint64, turn int, reply string) []hub.HistoryEntry {
	id, attempt := fmt.Sprintf("t%d", turn), fmt.Sprintf("a%d", turn)
	var out []hub.HistoryEntry
	for _, entry := range []hub.HistoryEntry{
		{TurnID: id, AttemptID: attempt, Input: fmt.Sprintf("question %d", turn)},
		{TurnID: id, AttemptID: attempt, Event: engine.Event{Kind: engine.Message, ID: "assistant-0", Text: reply}},
		{TurnID: id, AttemptID: attempt, Event: engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded}},
	} {
		*seq++
		entry.Sequence, entry.TurnSequence = *seq, uint64(turn)
		out = append(out, entry)
	}
	return out
}

func TestTranscriptTrimBothEndsWithinByteLimit(t *testing.T) {
	var seq uint64
	var history []hub.HistoryEntry
	for turn := 1; turn <= 100; turn++ {
		history = append(history, turnEntries(&seq, turn, strings.Repeat("r", 1024))...)
	}
	limit := 20 << 10
	w := NewTranscriptWindow(limit)
	w.Reset(chunkOf(history...))
	w.Trim(true)
	if w.bytes() > limit || !w.MoreBefore() || w.MoreAfter() || w.Last() != seq || w.First() <= 1 {
		t.Fatal("trimming the old end did not respect the limit", w.bytes(), w.First(), w.Last())
	}
	last := w.Blocks()[len(w.Blocks())-1]
	if last.ID != "a100/assistant-0" {
		t.Fatal("trimming at the end dropped the newest blocks", last.ID)
	}
	// Paging back across the cut rebuilds dropped blocks whole.
	service := &TranscriptService{Store: &memoryHistory{entries: history}}
	older, err := service.Older(context.Background(), "s", w.First())
	if err != nil {
		t.Fatal(err)
	}
	w.ApplyOlder(older)
	uniqueIDs(t, w)
	for _, block := range w.Blocks() {
		if block.Kind == BlockKindReply && len(block.Text) != 1024 {
			t.Fatal("rebuilt block is partial", block.ID, len(block.Text))
		}
	}
	w.Trim(false)
	if w.bytes() > limit || !w.MoreAfter() || w.Last() >= seq {
		t.Fatal("trimming the new end did not respect the limit", w.bytes(), w.Last())
	}
	newer, err := service.Newer(context.Background(), "s", w.Last())
	if err != nil {
		t.Fatal(err)
	}
	w.ApplyNewer(newer)
	uniqueIDs(t, w)
	if w.Last() != seq || w.MoreAfter() {
		t.Fatal("reading after the trimmed end did not reach the latest history", w.Last(), seq)
	}
}

func TestTranscriptTrimNeverDropsRunningBlocksOrLiveTurn(t *testing.T) {
	var seq uint64
	var history []hub.HistoryEntry
	for turn := 1; turn <= 40; turn++ {
		history = append(history, turnEntries(&seq, turn, strings.Repeat("r", 1024))...)
	}
	seq++
	history = append(history, hub.HistoryEntry{Sequence: seq, TurnID: "live", AttemptID: "live-attempt", Input: "streaming question"})
	for i := 0; i < 64; i++ {
		seq++
		history = append(history, hub.HistoryEntry{Sequence: seq, TurnID: "live", AttemptID: "live-attempt", Event: engine.Event{Kind: engine.MessageDelta, ID: "assistant-0", Text: strings.Repeat("s", 512)}})
	}
	w := NewTranscriptWindow(8 << 10)
	w.Reset(chunkOf(history...))
	w.Trim(false)
	if w.MoreAfter() || blockByID(t, w, "live-attempt/assistant-0").Status != BlockStatusRunning {
		t.Fatal("trimming the new end removed the running block")
	}
	w.Trim(true)
	blocks := w.Blocks()
	if len(blocks) != 2 || blocks[0].ID != "live/input" || len(blocks[1].Text) != 64*512 {
		t.Fatal("trimming dropped the live Turn's user block or streamed text", len(blocks))
	}
}

func TestTranscriptTrimCutsOnlyBetweenWholeBlocks(t *testing.T) {
	// The reply spans seq 1..6 while a tool block sits inside it at 3..4: no cut
	// may fall between them, or paging back would rebuild a partial reply.
	delta := func(seq uint64, text string) hub.HistoryEntry {
		return hub.HistoryEntry{Sequence: seq, TurnID: "t", AttemptID: "a", Event: engine.Event{Kind: engine.MessageDelta, ID: "reply", Text: text}}
	}
	entries := []hub.HistoryEntry{
		delta(1, strings.Repeat("a", 4096)), delta(2, strings.Repeat("b", 4096)),
		{Sequence: 3, TurnID: "t", AttemptID: "a", Event: engine.Event{Kind: engine.ToolStart, ID: "tool", Name: "read"}},
		{Sequence: 4, TurnID: "t", AttemptID: "a", Event: engine.Event{Kind: engine.ToolEnd, ID: "tool", Name: "read", Text: "r"}},
		delta(5, strings.Repeat("c", 4096)),
		{Sequence: 6, TurnID: "t", AttemptID: "a", Event: engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded}},
		{Sequence: 7, TurnID: "t2", AttemptID: "b", Input: "next"},
		{Sequence: 8, TurnID: "t2", AttemptID: "b", Event: engine.Event{Kind: engine.Message, ID: "reply", Text: "short"}},
		{Sequence: 9, TurnID: "t2", AttemptID: "b", Event: engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded}},
	}
	w := NewTranscriptWindow(1024)
	w.Reset(chunkOf(entries...))
	w.Trim(true)
	blocks := w.Blocks()
	if blocks[0].ID == "a/tool" || w.First() != 7 {
		t.Fatal("trim split an interleaved reply from its tool", blocks, w.First())
	}
}

func TestTranscriptServicePagesUntilEnoughBlocks(t *testing.T) {
	var seq uint64
	var history []hub.HistoryEntry
	for turn := 1; turn <= 40; turn++ {
		history = append(history, turnEntries(&seq, turn, "r")...)
		for i := 0; i < 200; i++ {
			seq++
			history = append(history, hub.HistoryEntry{Sequence: seq, TurnID: fmt.Sprintf("t%d", turn), AttemptID: fmt.Sprintf("a%d", turn), Event: engine.Event{Kind: engine.ExecutionActivity, Activity: &engine.Activity{ID: fmt.Sprint(i), Name: "probe", Effect: engine.EffectNone}}})
		}
	}
	store := &memoryHistory{entries: history}
	service := &TranscriptService{Store: store}
	chunk, err := service.Tail(context.Background(), "s")
	if err != nil {
		t.Fatal(err)
	}
	if store.calls != 16 || !chunk.More || len(chunk.entries) != 16*128 || chunk.Last != seq || chunk.First != chunk.entries[0].Sequence {
		t.Fatal("Tail did not stop at sixteen pages", store.calls, chunk.More, len(chunk.entries))
	}
	store.calls = 0
	sparse := &memoryHistory{}
	var sparseSeq uint64
	for turn := 1; turn <= 60; turn++ {
		sparse.entries = append(sparse.entries, turnEntries(&sparseSeq, turn, "r")...)
	}
	chunk, err = (&TranscriptService{Store: sparse}).Tail(context.Background(), "s")
	if err != nil || sparse.calls != 1 || !chunk.More {
		t.Fatal("Tail read past a page that already held enough blocks", sparse.calls, err)
	}
	older, err := (&TranscriptService{Store: sparse}).Older(context.Background(), "s", chunk.First)
	if err != nil || older.More || older.Last >= chunk.First || len(older.entries) != len(sparse.entries)-len(chunk.entries) {
		t.Fatal("Older did not read the remaining earlier history", older.More, len(older.entries), err)
	}
	empty, err := service.Older(context.Background(), "s", 0)
	if err != nil || len(empty.entries) != 0 || empty.More {
		t.Fatal("Older with an empty window read the newest history")
	}
	newer, err := service.Newer(context.Background(), "s", 0)
	if err != nil || len(newer.entries) != 8*128 || !newer.More {
		t.Fatal("Newer did not stop at eight pages", len(newer.entries), newer.More, err)
	}
	if _, err := (&TranscriptService{}).Tail(context.Background(), "s"); err == nil {
		t.Fatal("missing history store was not reported")
	}
}
