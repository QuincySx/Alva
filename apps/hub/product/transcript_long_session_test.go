// INPUT: Actual private product SQLite through BeginTurn/Record/FinishTurn, directly inserted legacy rows and optional synthetic GiB history
// OUTPUT: Long-session gate: superseded deltas deleted, Tail shows the conversation, Older stays byte-bounded and streaming keeps its user block
// POS: [L7 Hub product] CI gate for the Message Stream data path; ALVA_LONG_SESSION_GIB adds the full-size run outside CI
package product

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	hubsqlite "github.com/QuincySx/Alva/apps/hub/sqlite"
	"github.com/QuincySx/Alva/integration/engine"
	private "github.com/QuincySx/Alva/platform/storage/sqlite"
)

const longSessionDelta = "这是一段模拟模型输出的增量文本abc "

var longSessionTarget = hub.Target{EngineID: "alva", ModelID: "long-session", ProviderID: "local", AccountSource: "test-only", BillingRoute: "no-billing"}

// seedLongSession writes synthetic committed turns directly, as the format now
// stores them: input, tools, one final message and completion, deltas deleted.
func seedLongSession(t *testing.T, dir, session string, gib float64) {
	t.Helper()
	ctx := context.Background()
	db, err := private.Open(ctx, dir, "product.sqlite")
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	db.SetMaxOpenConns(1)
	if _, err = db.ExecContext(ctx, `PRAGMA synchronous=OFF`); err != nil {
		t.Fatal(err)
	}
	var branch hub.Branch
	var data []byte
	if err = db.QueryRowContext(ctx, `SELECT data FROM hub_branches WHERE session=?`, session).Scan(&data); err != nil || json.Unmarshal(data, &branch) != nil {
		t.Fatal("seed branch unavailable", err)
	}
	full := strings.Repeat(longSessionDelta, 400)
	path := filepath.Join(dir, "product.sqlite")
	size := func() float64 {
		info, err := os.Stat(path)
		if err != nil {
			return 0
		}
		return float64(info.Size()) / (1 << 30)
	}
	start, turns := time.Now(), uint64(0)
	for size() < gib {
		tx, err := db.BeginTx(ctx, nil)
		if err != nil {
			t.Fatal(err)
		}
		for i := 0; i < 256; i++ {
			turns++
			turnID, attemptID := "seed-turn-"+strconv.FormatUint(turns, 10), "seed-attempt-"+strconv.FormatUint(turns, 10)
			turn, _ := json.Marshal(hub.Turn{ID: turnID, SessionID: session, Sequence: turns, Prompt: "seeded question", Status: engine.Succeeded, ActiveAttemptID: attemptID, CreatedAt: 1, EndedAt: 2})
			attempt, _ := json.Marshal(hub.Attempt{ID: attemptID, TurnID: turnID, SessionID: session, BranchID: branch.ID, Target: longSessionTarget, NativeSessionID: "native-seed", Status: engine.Succeeded, CreatedAt: 1, EndedAt: 2})
			if _, err = tx.ExecContext(ctx, `INSERT INTO hub_turns(id,session,seq,status,data) VALUES(?,?,?,?,?)`, turnID, session, turns, engine.Succeeded, turn); err != nil {
				t.Fatal(err)
			}
			if _, err = tx.ExecContext(ctx, `INSERT INTO hub_attempts(id,session,turn,status,data) VALUES(?,?,?,?,?)`, attemptID, session, turnID, engine.Succeeded, attempt); err != nil {
				t.Fatal(err)
			}
			events := []hub.HistoryEntry{{Input: "第 " + strconv.FormatUint(turns, 10) + " 轮问题"}, {Event: engine.Event{Kind: engine.SessionStarted, SessionID: "native-seed"}}}
			for k := 0; k < 3; k++ {
				id := fmt.Sprintf("call-%d", k)
				events = append(events, hub.HistoryEntry{Event: engine.Event{Kind: engine.ToolStart, SessionID: "native-seed", ID: id, Name: "read_file", Effect: engine.EffectRead}}, hub.HistoryEntry{Event: engine.Event{Kind: engine.ToolEnd, SessionID: "native-seed", ID: id, Name: "read_file", Text: strings.Repeat("r", 500)}})
			}
			events = append(events, hub.HistoryEntry{Event: engine.Event{Kind: engine.Message, SessionID: "native-seed", ID: "assistant-0", Text: full}}, hub.HistoryEntry{Event: engine.Event{Kind: engine.Completed, SessionID: "native-seed", Outcome: engine.Succeeded}})
			for _, entry := range events {
				entry.SessionID, entry.TurnID, entry.AttemptID, entry.Time = session, turnID, attemptID, 1
				body, _ := json.Marshal(entry)
				if _, err = tx.ExecContext(ctx, `INSERT INTO hub_events(session,turn,attempt,data) VALUES(?,?,?,?)`, session, turnID, attemptID, body); err != nil {
					t.Fatal(err)
				}
			}
		}
		if err = tx.Commit(); err != nil {
			t.Fatal(err)
		}
	}
	// The seeded turns succeeded on this branch, as Record would have marked.
	branch.Watermark = turns
	branch.NativeSessionID = "native-seed"
	if data, err = json.Marshal(branch); err != nil {
		t.Fatal(err)
	}
	if _, err = db.ExecContext(ctx, `UPDATE hub_branches SET data=? WHERE id=?`, data, branch.ID); err != nil {
		t.Fatal(err)
	}
	t.Logf("seeded %.2f GiB: %d turns in %s", size(), turns, time.Since(start).Round(time.Second))
}

func visibleMessages(blocks []Block) int {
	count := 0
	for _, block := range blocks {
		if block.Kind == BlockKindUser || block.Kind == BlockKindReply {
			count++
		}
	}
	return count
}

func TestLongSessionTranscriptGate(t *testing.T) {
	ctx := context.Background()
	gib := 0.0
	if value := os.Getenv("ALVA_LONG_SESSION_GIB"); value != "" {
		parsed, err := strconv.ParseFloat(value, 64)
		if err != nil || parsed <= 0 {
			t.Fatal("ALVA_LONG_SESSION_GIB must be a positive number of GiB", value)
		}
		gib = parsed
	}
	dir := filepath.Join(t.TempDir(), "private")
	store, err := hubsqlite.Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	session, err := store.CreateSession(ctx, "long session", t.TempDir(), longSessionTarget)
	if err != nil {
		t.Fatal(err)
	}
	native := "native-long"
	if gib > 0 {
		if err = store.Close(); err != nil {
			t.Fatal(err)
		}
		seedLongSession(t, dir, session.ID, gib)
		native = "native-seed"
		if store, err = hubsqlite.Open(ctx, dir); err != nil {
			t.Fatal(err)
		}
	}
	t.Cleanup(func() { _ = store.Close() })
	record := func(attempt string, event engine.Event) time.Duration {
		t.Helper()
		start := time.Now()
		if err := store.Record(ctx, attempt, event, false); err != nil {
			t.Fatal(event.Kind, err)
		}
		return time.Since(start)
	}
	full := strings.Repeat(longSessionDelta, 300)
	var deltaLatency []time.Duration
	var realTurns []string
	for turn := 0; turn < 20; turn++ {
		claim, err := store.BeginTurn(ctx, session.ID, fmt.Sprintf("real question %d", turn))
		if err != nil {
			t.Fatal(err)
		}
		realTurns = append(realTurns, claim.Turn.ID)
		record(claim.Attempt.ID, engine.Event{Kind: engine.SessionStarted, SessionID: native})
		for i := 0; i < 300; i++ {
			deltaLatency = append(deltaLatency, record(claim.Attempt.ID, engine.Event{Kind: engine.MessageDelta, SessionID: native, ID: "assistant-0", Text: longSessionDelta}))
		}
		record(claim.Attempt.ID, engine.Event{Kind: engine.Message, SessionID: native, ID: "assistant-0", Text: full})
		record(claim.Attempt.ID, engine.Event{Kind: engine.Completed, SessionID: native, Outcome: engine.Succeeded})
		if err = store.FinishTurn(ctx, claim.Turn.ID, engine.Succeeded); err != nil {
			t.Fatal(err)
		}
	}
	// Legacy rows: committed before deltas were merged and deleted.
	raw, err := private.Open(ctx, dir, "product.sqlite")
	if err != nil {
		t.Fatal(err)
	}
	defer raw.Close()
	var legacyTurns []string
	for turn := 0; turn < 3; turn++ {
		claim, err := store.BeginTurn(ctx, session.ID, fmt.Sprintf("legacy question %d", turn))
		if err != nil {
			t.Fatal(err)
		}
		legacyTurns = append(legacyTurns, claim.Turn.ID)
		record(claim.Attempt.ID, engine.Event{Kind: engine.SessionStarted, SessionID: native})
		insert := func(event engine.Event) {
			body, _ := json.Marshal(hub.HistoryEntry{SessionID: session.ID, TurnID: claim.Turn.ID, AttemptID: claim.Attempt.ID, Event: event, Time: 1})
			if _, err := raw.ExecContext(ctx, `INSERT INTO hub_events(session,turn,attempt,data) VALUES(?,?,?,?)`, session.ID, claim.Turn.ID, claim.Attempt.ID, body); err != nil {
				t.Fatal(err)
			}
		}
		for i := 0; i < 100; i++ {
			insert(engine.Event{Kind: engine.MessageDelta, SessionID: native, ID: "assistant-0", Text: "legacy "})
		}
		insert(engine.Event{Kind: engine.Message, SessionID: native, ID: "assistant-0", Text: strings.Repeat("legacy ", 100)})
		record(claim.Attempt.ID, engine.Event{Kind: engine.Completed, SessionID: native, Outcome: engine.Succeeded})
		if err = store.FinishTurn(ctx, claim.Turn.ID, engine.Succeeded); err != nil {
			t.Fatal(err)
		}
	}
	deltas := map[string]int{}
	for _, entry := range longSessionTail(t, store, session.ID) {
		if entry.Event.Kind == engine.MessageDelta {
			deltas[entry.TurnID]++
		}
	}
	for _, turn := range realTurns {
		if deltas[turn] != 0 {
			t.Fatal("final message left merged deltas behind", turn, deltas[turn])
		}
	}
	for _, turn := range legacyTurns {
		if deltas[turn] != 100 {
			t.Fatal("legacy rows were rewritten", turn, deltas[turn])
		}
	}

	service := &TranscriptService{Store: store}
	start := time.Now()
	chunk, err := service.Tail(ctx, session.ID)
	if err != nil {
		t.Fatal(err)
	}
	window := NewTranscriptWindow(0)
	window.Reset(chunk)
	window.Trim(true)
	opened := time.Since(start)
	blocks := window.Blocks()
	if visibleMessages(blocks) < 20 {
		t.Fatal("Tail shows too little conversation", visibleMessages(blocks))
	}
	for _, block := range blocks {
		if block.Kind == BlockKindReply && block.Text != full && block.Text != strings.Repeat("legacy ", 100) {
			t.Fatal("reply text is partial after opening", block.ID, len(block.Text))
		}
	}

	// Streaming: the live Turn keeps its user block and complete draft text.
	claim, err := store.BeginTurn(ctx, session.ID, "streaming question")
	if err != nil {
		t.Fatal(err)
	}
	record(claim.Attempt.ID, engine.Event{Kind: engine.SessionStarted, SessionID: native})
	var streamed strings.Builder
	for i := 0; i < 300; i++ {
		text := fmt.Sprintf("s%d ", i)
		streamed.WriteString(text)
		record(claim.Attempt.ID, engine.Event{Kind: engine.MessageDelta, SessionID: native, ID: "assistant-0", Text: text})
		if i%75 == 74 {
			newer, err := service.Newer(ctx, session.ID, window.Last())
			if err != nil {
				t.Fatal(err)
			}
			window.ApplyNewer(newer)
			window.Trim(true)
		}
	}
	live := false
	for _, block := range window.Blocks() {
		live = live || block.ID == claim.Turn.ID+"/input"
		if block.ID == claim.Attempt.ID+"/assistant-0" && (block.Text != streamed.String() || block.Status != BlockStatusRunning) {
			t.Fatal("streamed draft is incomplete", len(block.Text), streamed.Len())
		}
	}
	if !live {
		t.Fatal("streaming evicted its own user block")
	}
	record(claim.Attempt.ID, engine.Event{Kind: engine.Message, SessionID: native, ID: "assistant-0", Text: streamed.String()})
	record(claim.Attempt.ID, engine.Event{Kind: engine.Completed, SessionID: native, Outcome: engine.Succeeded})
	if err = store.FinishTurn(ctx, claim.Turn.ID, engine.Succeeded); err != nil {
		t.Fatal(err)
	}

	// Scrolling back fifty times stays within the byte limit, both default and tight.
	for _, limit := range []int{0, 48 << 10} {
		w := NewTranscriptWindow(limit)
		if limit == 0 {
			limit = transcriptDefaultBytes
		}
		tail, err := service.Tail(ctx, session.ID)
		if err != nil {
			t.Fatal(err)
		}
		w.Reset(tail)
		w.Trim(true)
		for i := 0; i < 50; i++ {
			older, err := service.Older(ctx, session.ID, w.First())
			if err != nil {
				t.Fatal(err)
			}
			w.ApplyOlder(older)
			w.Trim(false)
			if w.bytes() > limit {
				t.Fatal("window exceeded its byte limit while scrolling back", i, w.bytes(), limit)
			}
		}
	}

	if gib > 0 {
		sort.Slice(deltaLatency, func(i, j int) bool { return deltaLatency[i] < deltaLatency[j] })
		p95 := deltaLatency[len(deltaLatency)*95/100]
		t.Logf("%.1f GiB: Tail+Reset %s with %d visible messages; Record(MessageDelta) p50=%s p95=%s n=%d", gib, opened, visibleMessages(blocks), deltaLatency[len(deltaLatency)/2], p95, len(deltaLatency))
		if opened > 100*time.Millisecond {
			t.Fatal("opening a long session exceeded 100 ms", opened)
		}
	}
}

// longSessionTail reads only the recent part of history the gate wrote, so the
// GiB run does not scan the whole synthetic prefix.
func longSessionTail(t *testing.T, store *hubsqlite.Store, session string) []hub.HistoryEntry {
	t.Helper()
	var all []hub.HistoryEntry
	var before uint64
	for page := 0; page < 64; page++ {
		entries, err := store.HistoryBefore(context.Background(), session, before, 128)
		if err != nil {
			t.Fatal(err)
		}
		all = append(entries, all...)
		if len(entries) < 128 || strings.HasPrefix(entries[0].TurnID, "seed-") {
			break
		}
		before = entries[0].Sequence
	}
	return all
}
