// INPUT: hubFixture real Alva assembly, held model streams and the Conversations/Turns services
// OUTPUT: Scenarios for rename/pin surviving a restart, soft delete (hidden everywhere, data kept, running refused) and continuing a stopped reply
// POS: [scenario] Session management and Continue as the user sees them across an application restart.
package scenarios

import (
	"context"
	"errors"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/llm/provider"
)

func TestRenamedAndPinnedSessionSurvivesRestart(t *testing.T) {
	model := newFakeModel(t, provider.OpenAIChatKind)
	model.next = func(modelRequest) reply { return text("an answer") }
	h := openHub(t, liveConfig(t, model))
	ctx := context.Background()
	first := h.create("first", "")
	second := h.create("second", "")
	h.send(first.ID, "question")
	h.expect(first.ID, engine.Succeeded)
	if err := h.service().Conversations.Rename(ctx, first.ID, "  更好的名字 "); err != nil {
		t.Fatal(err)
	}
	if err := h.service().Conversations.SetPinned(ctx, second.ID, true); err != nil {
		t.Fatal(err)
	}
	h.reopen()
	d, err := h.service().Conversations.Detail(ctx, first.ID)
	if err != nil || d.Session.Title != "更好的名字" {
		t.Fatal("the new title did not survive a restart", d.Session.Title, err)
	}
	pinned, err := h.service().Conversations.Pinned(ctx)
	if err != nil || len(pinned) != 1 || pinned[0].ID != second.ID {
		t.Fatal("the pin did not survive a restart", pinned, err)
	}
	page, err := h.service().Conversations.Page(ctx, "更好", hub.SessionCursor{}, 10)
	if err != nil || len(page.Sessions) != 1 || page.Sessions[0].ID != first.ID {
		t.Fatal("search does not find the renamed session", page.Sessions, err)
	}
	if blocks := h.blocks(first.ID); len(blocks) < 2 {
		t.Fatal("rename damaged the conversation", blocks)
	}
}

func TestDeletedSessionVanishesFromListsButItsDataStays(t *testing.T) {
	hold, reached := make(chan struct{}), make(chan struct{})
	model := newFakeModel(t, provider.OpenAIChatKind)
	model.next = func(r modelRequest) reply {
		if r.typed("hold this one") {
			return reply{Text: []string{"working"}, Hold: hold, Reached: reached}
		}
		return text("an answer")
	}
	h := openHub(t, liveConfig(t, model))
	ctx := context.Background()
	keep := h.create("keep me", "")
	gone := h.create("delete me", "")
	h.send(keep.ID, "hello")
	h.send(gone.ID, "hello")
	h.expect(keep.ID, engine.Succeeded)
	h.expect(gone.ID, engine.Succeeded)
	if err := h.service().Conversations.SetPinned(ctx, gone.ID, true); err != nil {
		t.Fatal(err)
	}

	// A running session refuses deletion and stays fully usable.
	busy := h.create("busy", "")
	h.send(busy.ID, "hold this one")
	received(t, reached, "the held reply")
	if err := h.service().Conversations.Delete(ctx, busy.ID); !errors.Is(err, hub.ErrSessionRunning) {
		t.Fatal("a running session was deletable", err)
	}
	execution, _ := h.app.Runtime.ActiveExecution(busy.ID)
	if err := h.app.Runtime.CancelExecution(ctx, busy.ID, execution); err != nil {
		t.Fatal(err)
	}
	close(hold)
	h.expect(busy.ID, engine.Cancelled)

	if err := h.service().Conversations.Delete(ctx, gone.ID); err != nil {
		t.Fatal(err)
	}
	check := func(when string) {
		t.Helper()
		page, err := h.service().Conversations.Page(ctx, "", hub.SessionCursor{}, 10)
		if err != nil {
			t.Fatal(err)
		}
		for _, s := range page.Sessions {
			if s.ID == gone.ID {
				t.Fatalf("%s: the deleted session is still listed", when)
			}
		}
		if found, _ := h.service().Conversations.Page(ctx, "delete me", hub.SessionCursor{}, 10); len(found.Sessions) != 0 {
			t.Fatalf("%s: search still finds the deleted session", when)
		}
		if pinned, _ := h.service().Conversations.Pinned(ctx); len(pinned) != 0 {
			t.Fatalf("%s: the deleted session is still pinned", when)
		}
		if _, err := h.service().Conversations.Detail(ctx, gone.ID); !errors.Is(err, hub.ErrNotFound) {
			t.Fatalf("%s: the deleted session is still addressable: %v", when, err)
		}
		if _, err := h.service().Turns.Send(ctx, gone.ID, "late", nil); !errors.Is(err, hub.ErrNotFound) {
			t.Fatalf("%s: the deleted session accepted a Turn: %v", when, err)
		}
		// The data is still on disk behind the product store.
		raw, err := h.app.Store.DetailIncludingDeleted(ctx, gone.ID)
		if err != nil {
			t.Fatal(err)
		}
		if raw.Session.DeletedAt == 0 || raw.Branches[0].NativeSessionID == "" {
			t.Fatalf("%s: soft delete lost data: %+v", when, raw.Session)
		}
	}
	check("before restart")
	h.reopen()
	check("after restart")
}

func TestStoppedReplyContinuesAsAStatusBlockNotAUserMessage(t *testing.T) {
	hold, reached := make(chan struct{}), make(chan struct{})
	defer close(hold)
	model := newFakeModel(t, provider.OpenAIChatKind,
		reply{Text: []string{"partial ", "answer"}, Hold: hold, Reached: reached},
		text("the rest of the answer"),
	)
	h := openHub(t, liveConfig(t, model))
	ctx := context.Background()
	s := h.create("continue", "")
	if _, err := h.service().Turns.Continue(ctx, s.ID); !errors.Is(err, product.ErrNothingToContinue) {
		t.Fatal("a fresh session offered Continue", err)
	}
	h.send(s.ID, "long question")
	received(t, reached, "the partial reply")
	waitFor(t, "partial reply", func() bool {
		r := blocksOf(h.blocks(s.ID), product.BlockKindReply)
		return len(r) == 1 && r[0].Text == "partial answer"
	})
	if _, err := h.service().Turns.Continue(ctx, s.ID); err == nil {
		t.Fatal("Continue was accepted while the reply was still running")
	}
	execution, _ := h.app.Runtime.ActiveExecution(s.ID)
	if err := h.app.Runtime.CancelExecution(ctx, s.ID, execution); err != nil {
		t.Fatal(err)
	}
	h.expect(s.ID, engine.Cancelled)

	start, err := h.service().Turns.Continue(ctx, s.ID)
	if err != nil {
		t.Fatal(err)
	}
	if start.ExecutionID == "" {
		t.Fatal("Continue returned no execution identity to stop")
	}
	h.expect(s.ID, engine.Succeeded)
	request := model.request(2)
	if !request.has("long question") || !request.has(product.ContinuePrompt) {
		t.Fatalf("the continue request lacks the conversation or the fixed instruction: %s", request.Body)
	}
	check := func(when string) {
		t.Helper()
		var statuses, users int
		for _, block := range h.blocks(s.ID) {
			if block.TurnID != start.TurnID {
				continue
			}
			if block.Kind == product.BlockKindStatus && block.Text == "继续生成" {
				statuses++
			}
			if block.Kind == product.BlockKindUser {
				users++
			}
		}
		if statuses != 1 || users != 0 {
			t.Fatalf("%s: continue turn shows %d status and %d user blocks: %s", when, statuses, users, h.dump(s.ID))
		}
		if r := lastReply(t, h.blocks(s.ID)); !strings.Contains(r.Text, "the rest of the answer") {
			t.Fatalf("%s: continued reply %+v", when, r)
		}
	}
	check("live")
	h.reopen()
	check("after restart")
	// A finished reply offers no Continue.
	if _, err := h.service().Turns.Continue(ctx, s.ID); !errors.Is(err, product.ErrNothingToContinue) {
		t.Fatal("a completed reply offered Continue", err)
	}
}
