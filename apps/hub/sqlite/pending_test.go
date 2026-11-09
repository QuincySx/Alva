// INPUT: Real private SQLite product stores, queued and interjected inputs, Turn admission and delivery receipts
// OUTPUT: Queue bounds and order, atomic dispatch, delivery receipts, requeue at the Turn end and crash recovery regressions
// POS: [L7 Hub product] Proves a queued or interjected message is neither lost nor sent twice: dispatch deletes it in the admission transaction, a receipt deletes it, an unreceipted one returns to the head
package sqlite

import (
	"context"
	"errors"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

func queueTexts(t *testing.T, s *Store, id string) string {
	t.Helper()
	items, err := s.PendingInputs(context.Background(), id)
	if err != nil {
		t.Fatal(err)
	}
	var parts []string
	for _, item := range items {
		parts = append(parts, string(item.State)[:1]+":"+item.Text)
	}
	return strings.Join(parts, "|")
}

func TestQueueIsBoundedOrderedAndEditableOnlyWhileQueued(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	session, err := s.CreateSession(ctx, "queue", t.TempDir(), target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	var first hub.PendingInput
	for i := 0; i < hub.MaxPendingInputs; i++ {
		item, e := s.QueueInput(ctx, session.ID, string(rune('a'+i)), hub.PendingQueued)
		if e != nil {
			t.Fatal(e)
		}
		if i == 0 {
			first = item
		}
	}
	if _, err = s.QueueInput(ctx, session.ID, "ninth", hub.PendingQueued); !errors.Is(err, hub.ErrPendingFull) {
		t.Fatal("the ninth input must be refused", err)
	}
	if got := queueTexts(t, s, session.ID); got != "q:a|q:b|q:c|q:d|q:e|q:f|q:g|q:h" {
		t.Fatal("dispatch order", got)
	}
	if _, err = s.QueueInput(ctx, session.ID, strings.Repeat("x", hub.MaxPendingInputText+1), hub.PendingQueued); err == nil {
		t.Fatal("16 KiB + 1 must be refused")
	}
	if _, err = s.QueueInput(ctx, session.ID, "", hub.PendingQueued); err == nil {
		t.Fatal("empty input must be refused")
	}
	if _, err = s.QueueInput(ctx, session.ID, "bad\x00", hub.PendingQueued); err == nil {
		t.Fatal("NUL must be refused")
	}
	edited, err := s.EditPendingInput(ctx, session.ID, first.ID, "a-edited")
	if err != nil || edited.Text != "a-edited" {
		t.Fatal(err)
	}
	if err = s.CancelPendingInput(ctx, session.ID, first.ID); err != nil {
		t.Fatal(err)
	}
	if _, err = s.EditPendingInput(ctx, session.ID, first.ID, "again"); !errors.Is(err, hub.ErrPendingGone) {
		t.Fatal("a cancelled input cannot be edited", err)
	}
	if got := queueTexts(t, s, session.ID); !strings.HasPrefix(got, "q:b|") {
		t.Fatal("cancel removes only that input", got)
	}
	if _, err = s.PendingInputs(ctx, "hub-missing"); !errors.Is(err, hub.ErrNotFound) {
		t.Fatal("unknown session", err)
	}
}

func TestDispatchDeletesTheQueuedInputInTheAdmissionTransaction(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	session, err := s.CreateSession(ctx, "dispatch", t.TempDir(), target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	item, err := s.QueueInput(ctx, session.ID, "queued prompt", hub.PendingQueued)
	if err != nil {
		t.Fatal(err)
	}
	claim, err := s.BeginComposerTurn(ctx, session.ID, "", hub.ComposerTurnInput{Prompt: "queued prompt", UserText: "queued prompt", QueueID: item.ID})
	if err != nil {
		t.Fatal(err)
	}
	if got := queueTexts(t, s, session.ID); got != "" {
		t.Fatal("admission must consume the queued input", got)
	}
	if err = s.Record(ctx, claim.Attempt.ID, engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded}, false); err != nil {
		t.Fatal(err)
	}
	if err = s.FinishTurn(ctx, claim.Turn.ID, engine.Succeeded); err != nil {
		t.Fatal(err)
	}
	// A second admission naming the same input fails whole: no second Turn.
	before := rowCount(t, s, `SELECT count(*) FROM hub_turns`)
	if _, err = s.BeginComposerTurn(ctx, session.ID, "", hub.ComposerTurnInput{Prompt: "queued prompt", UserText: "queued prompt", QueueID: item.ID}); !errors.Is(err, hub.ErrPendingGone) {
		t.Fatal("a dispatched input cannot be dispatched twice", err)
	}
	if rowCount(t, s, `SELECT count(*) FROM hub_turns`) != before {
		t.Fatal("a failed dispatch must not leave a Turn behind")
	}
}

func TestDeliveryReceiptDeletesAndTurnEndRequeuesAtTheHead(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	session, err := s.CreateSession(ctx, "interject", t.TempDir(), target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.QueueInput(ctx, session.ID, "later", hub.PendingQueued); err != nil {
		t.Fatal(err)
	}
	claim, err := s.BeginTurn(ctx, session.ID, "go")
	if err != nil {
		t.Fatal(err)
	}
	delivered, err := s.QueueInput(ctx, session.ID, "now-1", hub.PendingInterjecting)
	if err != nil {
		t.Fatal(err)
	}
	lost, err := s.QueueInput(ctx, session.ID, "now-2", hub.PendingInterjecting)
	if err != nil {
		t.Fatal(err)
	}
	if err = s.Record(ctx, claim.Attempt.ID, engine.Event{Kind: engine.SessionStarted, SessionID: "native-1"}, false); err != nil {
		t.Fatal(err)
	}
	// An interjecting input cannot be cancelled or edited: it is with the engine.
	if err = s.CancelPendingInput(ctx, session.ID, delivered.ID); !errors.Is(err, hub.ErrPendingGone) {
		t.Fatal("an interjecting input cannot be retracted", err)
	}
	if _, err = s.EditPendingInput(ctx, session.ID, delivered.ID, "x"); !errors.Is(err, hub.ErrPendingGone) {
		t.Fatal("an interjecting input cannot be edited", err)
	}
	if err = s.Record(ctx, claim.Attempt.ID, engine.Event{Kind: engine.InputDelivered, SessionID: "native-1", ID: delivered.ID, Text: "now-1"}, false); err != nil {
		t.Fatal(err)
	}
	if got := queueTexts(t, s, session.ID); got != "q:later|i:now-2" {
		t.Fatal("the receipt deletes exactly the delivered input", got)
	}
	if err = s.Record(ctx, claim.Attempt.ID, engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded}, false); err != nil {
		t.Fatal(err)
	}
	if err = s.FinishTurn(ctx, claim.Turn.ID, engine.Succeeded); err != nil {
		t.Fatal(err)
	}
	if got := queueTexts(t, s, session.ID); got != "q:now-2|q:later" {
		t.Fatal("an interjection without a receipt returns to the queue head", got)
	}
	items, _ := s.PendingInputs(ctx, session.ID)
	if items[0].ID != lost.ID || items[0].Error == "" {
		t.Fatal("the requeued input says why", items[0])
	}
	// The receipt event is part of the visible history.
	entries, err := s.History(ctx, session.ID, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	seen := false
	for _, entry := range entries {
		seen = seen || entry.Event.Kind == engine.InputDelivered && entry.Event.Text == "now-1"
	}
	if !seen {
		t.Fatal("the delivery receipt must be in the history")
	}
}

func TestCrashLeavesInterjectedInputUncertainAndNeverResends(t *testing.T) {
	ctx := context.Background()
	dir := filepath.Join(t.TempDir(), "private")
	s, err := Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	session, err := s.CreateSession(ctx, "crash", t.TempDir(), target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.BeginTurn(ctx, session.ID, "go"); err != nil {
		t.Fatal(err)
	}
	if _, err = s.QueueInput(ctx, session.ID, "with engine", hub.PendingInterjecting); err != nil {
		t.Fatal(err)
	}
	if _, err = s.QueueInput(ctx, session.ID, "waiting", hub.PendingQueued); err != nil {
		t.Fatal(err)
	}
	if err = s.Close(); err != nil {
		t.Fatal(err)
	}
	reopened, err := Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	defer reopened.Close()
	if got := queueTexts(t, reopened, session.ID); got != "u:with engine|q:waiting" {
		t.Fatal("after a crash the interjected input is uncertain and the queue is kept", got)
	}
	items, _ := reopened.PendingInputs(ctx, session.ID)
	if items[0].Error == "" {
		t.Fatal("an uncertain input must explain itself")
	}
	if err = reopened.CancelPendingInput(ctx, session.ID, items[0].ID); err != nil {
		t.Fatal("an uncertain input can be discarded by the user", err)
	}
}

func TestRequeueKeepsOriginalOrderOfSeveralInterjections(t *testing.T) {
	s := newStore(t)
	ctx := context.Background()
	session, _ := s.CreateSession(ctx, "order", t.TempDir(), target("alva"))
	claim, err := s.BeginTurn(ctx, session.ID, "go")
	if err != nil {
		t.Fatal(err)
	}
	for _, text := range []string{"i1", "i2", "i3"} {
		if _, err = s.QueueInput(ctx, session.ID, text, hub.PendingInterjecting); err != nil {
			t.Fatal(err)
		}
	}
	if _, err = s.QueueInput(ctx, session.ID, "q1", hub.PendingQueued); err != nil {
		t.Fatal(err)
	}
	if err = s.Record(ctx, claim.Attempt.ID, engine.Event{Kind: engine.Completed, Outcome: engine.Failed}, false); err != nil {
		t.Fatal(err)
	}
	if err = s.FinishTurn(ctx, claim.Turn.ID, engine.Failed); err != nil {
		t.Fatal(err)
	}
	if got := queueTexts(t, s, session.ID); got != "q:i1|q:i2|q:i3|q:q1" {
		t.Fatal("interjections return in their original order, before older queued input", got)
	}
}

func TestUncertainInputIsDispatchedAtomicallyAndKeepsItsMarkWhenAdmissionFails(t *testing.T) {
	ctx := context.Background()
	dir := filepath.Join(t.TempDir(), "private")
	s, err := Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	session, err := s.CreateSession(ctx, "uncertain", t.TempDir(), target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	running, err := s.BeginTurn(ctx, session.ID, "go")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.QueueInput(ctx, session.ID, "maybe sent", hub.PendingInterjecting); err != nil {
		t.Fatal(err)
	}
	if _, err = s.QueueInput(ctx, session.ID, "after it", hub.PendingQueued); err != nil {
		t.Fatal(err)
	}
	if err = s.Close(); err != nil {
		t.Fatal(err)
	}
	if s, err = Open(ctx, dir); err != nil {
		t.Fatal(err)
	}
	defer s.Close()
	items, _ := s.PendingInputs(ctx, session.ID)
	if len(items) != 2 || items[0].State != hub.PendingUncertain {
		t.Fatal("fixture: the interjected input is uncertain after the crash", items)
	}
	uncertain := items[0]
	_ = running
	// The crash interrupted the Turn (recovery), the session is free again.
	// An admission that fails keeps the row exactly as it was: same state, same
	// text, still at the head.
	if _, err = s.BeginComposerTurn(ctx, session.ID, "", hub.ComposerTurnInput{Prompt: "   ", UserText: "   ", QueueID: uncertain.ID}); err == nil {
		t.Fatal("a blank prompt cannot be admitted")
	}
	after, _ := s.PendingInputs(ctx, session.ID)
	if len(after) != 2 || after[0].ID != uncertain.ID || after[0].State != hub.PendingUncertain || after[0].Error == "" || after[0].Position != uncertain.Position {
		t.Fatal("a failed admission must not move or unmark the uncertain input", after)
	}
	// An admission that succeeds consumes it in the same transaction.
	claim, err := s.BeginComposerTurn(ctx, session.ID, "", hub.ComposerTurnInput{Prompt: uncertain.Text, UserText: uncertain.Text, QueueID: uncertain.ID})
	if err != nil {
		t.Fatal(err)
	}
	if got := queueTexts(t, s, session.ID); got != "q:after it" {
		t.Fatal("dispatch consumes exactly the uncertain input", got)
	}
	if claim.Turn.Prompt != "maybe sent" {
		t.Fatal(claim.Turn.Prompt)
	}
}

func TestBlankQueuedTextIsRefused(t *testing.T) {
	s := newStore(t)
	session, _ := s.CreateSession(context.Background(), "blank", t.TempDir(), target("alva"))
	for _, blank := range []string{"", " ", "\n\t"} {
		if _, err := s.QueueInput(context.Background(), session.ID, blank, hub.PendingQueued); err == nil {
			t.Fatalf("%q was queued", blank)
		}
	}
	if _, err := s.EditPendingInput(context.Background(), session.ID, "input-x", "  "); err == nil {
		t.Fatal("a blank edit was accepted")
	}
}
