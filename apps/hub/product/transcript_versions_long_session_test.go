// INPUT: Actual private product SQLite, optional synthetic GiB history (ALVA_LONG_SESSION_GIB) and a version line cut near the end of it
// OUTPUT: Long-session gate with versions: the segmented History read opens the Tail as fast as an unversioned session, hides the replaced Turns and keeps Older paging inside its byte limit
// POS: [L7 Hub product] The version line's History/HistoryBefore path must not slow long sessions; the unversioned path is gated by TestLongSessionTranscriptGate
package product

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	hubsqlite "github.com/QuincySx/Alva/apps/hub/sqlite"
	"github.com/QuincySx/Alva/integration/engine"
)

func TestLongSessionTranscriptGateWithVersionLines(t *testing.T) {
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
	session, err := store.CreateSession(ctx, "long versioned session", t.TempDir(), longSessionTarget)
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
	record := func(attempt string, event engine.Event) {
		t.Helper()
		if err := store.Record(ctx, attempt, event, false); err != nil {
			t.Fatal(event.Kind, err)
		}
	}
	full := strings.Repeat(longSessionDelta, 300)
	var turns []string
	for turn := 0; turn < 20; turn++ {
		claim, err := store.BeginTurn(ctx, session.ID, fmt.Sprintf("real question %d", turn))
		if err != nil {
			t.Fatal(err)
		}
		turns = append(turns, claim.Turn.ID)
		record(claim.Attempt.ID, engine.Event{Kind: engine.SessionStarted, SessionID: native})
		record(claim.Attempt.ID, engine.Event{Kind: engine.Message, SessionID: native, ID: "assistant-0", Text: full})
		record(claim.Attempt.ID, engine.Event{Kind: engine.Completed, SessionID: native, Outcome: engine.Succeeded})
		if err = store.FinishTurn(ctx, claim.Turn.ID, engine.Succeeded); err != nil {
			t.Fatal(err)
		}
	}
	// Edit the fifth-from-last Turn: a new version line cut before it, then one
	// reply in the new version.
	anchorStart := time.Now()
	if _, _, err = store.ForkAnchor(ctx, session.ID, turns[len(turns)-5], false); err != nil {
		t.Fatal(err)
	}
	anchorTook := time.Since(anchorStart)
	start := time.Now()
	claim, err := store.BeginComposerTurn(ctx, session.ID, "", hub.ComposerTurnInput{Prompt: "edited question", UserText: "edited question", Line: &hub.LineFork{TurnID: turns[len(turns)-5], NativeFork: true}})
	if err != nil {
		t.Fatal(err)
	}
	forked := time.Since(start)
	record(claim.Attempt.ID, engine.Event{Kind: engine.SessionStarted, SessionID: "native-forked"})
	record(claim.Attempt.ID, engine.Event{Kind: engine.Message, SessionID: "native-forked", ID: "assistant-0", Text: full})
	record(claim.Attempt.ID, engine.Event{Kind: engine.Completed, SessionID: "native-forked", Outcome: engine.Succeeded})
	if err = store.FinishTurn(ctx, claim.Turn.ID, engine.Succeeded); err != nil {
		t.Fatal(err)
	}

	service := &TranscriptService{Store: store}
	opening := time.Now()
	chunk, err := service.Tail(ctx, session.ID)
	if err != nil {
		t.Fatal(err)
	}
	window := NewTranscriptWindow(0)
	window.Reset(chunk)
	window.Trim(true)
	opened := time.Since(opening)
	var shown []string
	for _, block := range window.Blocks() {
		if block.Kind == BlockKindUser {
			shown = append(shown, block.Text)
		}
	}
	tailUsers := strings.Join(shown, "|")
	if !strings.HasSuffix(tailUsers, "real question 14|edited question") || strings.Contains(tailUsers, "real question 15") || strings.Contains(tailUsers, "real question 19") {
		t.Fatal("the edited version must end with the edit and hide the replaced Turns", tailUsers)
	}
	// The other version is whole and one switch away.
	lines, err := store.Lines(ctx, session.ID)
	if err != nil || len(lines) != 2 {
		t.Fatal(err, len(lines))
	}
	// Scrolling back fifty times stays within the byte limit across the segment edge.
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
	t.Logf("%.1f GiB with a version line: edit admission transaction %s (anchor lookup alone %s); Tail+Reset %s with %d visible messages", gib, forked, anchorTook, opened, visibleMessages(window.Blocks()))
	if gib > 0 && forked > 50*time.Millisecond {
		t.Fatal("the edit admission transaction holds the write connection too long", forked)
	}
	if gib > 0 && opened > 100*time.Millisecond {
		t.Fatal("opening a long versioned session exceeded 100 ms", opened)
	}
}
