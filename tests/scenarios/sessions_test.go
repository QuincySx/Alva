// INPUT: hubFixture real Alva assembly and SQLite reopened on one data directory, fakeModel replies keyed by prompt markers
// OUTPUT: Scenarios for creating, listing/paging, reopening and resuming sessions, and two sessions running at once
// POS: [scenario] Session lifecycle as the user sees it across an application restart.
package scenarios

import (
	"context"
	"fmt"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/llm/provider"
)

type sessionSnapshot struct {
	title, workspace, branch, native string
	status                           engine.Outcome
	blocks                           []string
}

func (h *hubFixture) snapshot(id string) sessionSnapshot {
	h.t.Helper()
	d := h.detail(id)
	b := h.branch(id)
	s := sessionSnapshot{title: d.Session.Title, workspace: d.Session.Workspace, status: d.Session.Status, branch: b.ID, native: b.NativeSessionID}
	for _, block := range h.blocks(id) {
		s.blocks = append(s.blocks, fmt.Sprintf("%s|%s|%s|%s", block.ID, block.Kind, block.Status, block.Text))
	}
	return s
}

func TestSessionsListPageAndRestoreAfterRestart(t *testing.T) {
	model := newFakeModel(t, provider.OpenAIChatKind)
	model.next = func(r modelRequest) reply {
		if r.typed("please fail") {
			return reply{Status: 400}
		}
		return text("answer to ", "your question")
	}
	h := openHub(t, liveConfig(t, model))
	var ids []string
	for i := 0; i < 5; i++ {
		ids = append(ids, h.create(fmt.Sprintf("conversation %d", i), "").ID)
	}
	h.send(ids[0], "first question")
	h.expect(ids[0], engine.Succeeded)
	h.send(ids[1], "please fail")
	h.expect(ids[1], engine.Failed)
	// Paging returns every session exactly once, most recently updated first.
	seen := map[string]bool{}
	var order []string
	cursor := hub.SessionCursor{}
	for pages := 0; ; pages++ {
		if pages > 5 {
			t.Fatal("session paging did not end")
		}
		page, err := h.app.Store.PageSessions(context.Background(), "", cursor, 2)
		if err != nil {
			t.Fatal(err)
		}
		for _, s := range page.Sessions {
			if seen[s.ID] {
				t.Fatalf("session %s listed twice", s.ID)
			}
			seen[s.ID] = true
			order = append(order, s.ID)
		}
		if !page.HasMore {
			break
		}
		cursor = page.Next
	}
	if len(order) != 5 || order[0] != ids[1] || order[1] != ids[0] {
		t.Fatalf("paged order %v, want the failed then the answered session first (ids %v)", order, ids)
	}
	if page, err := h.app.Store.PageSessions(context.Background(), "conversation 3", hub.SessionCursor{}, 10); err != nil || len(page.Sessions) != 1 || page.Sessions[0].ID != ids[3] {
		t.Fatalf("title search %+v %v", page.Sessions, err)
	}
	before := map[string]sessionSnapshot{}
	for _, id := range ids {
		before[id] = h.snapshot(id)
	}
	h.reopen()
	if h.app.Runtime.HasActive() {
		t.Fatal("reopen started execution")
	}
	for _, id := range ids {
		after := h.snapshot(id)
		if fmt.Sprint(after) != fmt.Sprint(before[id]) {
			t.Fatalf("session %s changed across restart:\nbefore %+v\nafter  %+v", id, before[id], after)
		}
	}
	if s := before[ids[0]]; s.status != engine.Succeeded || len(s.blocks) < 2 || !strings.Contains(strings.Join(s.blocks, "\n"), "answer to your question") {
		t.Fatalf("answered session snapshot %+v", s)
	}
	// Reopening continues the same native conversation with its history.
	calls := model.calls()
	h.send(ids[0], "follow-up question")
	h.expect(ids[0], engine.Succeeded)
	if b := h.branch(ids[0]); b.NativeSessionID != before[ids[0]].native {
		t.Fatal("reopen started a new native conversation")
	}
	followUp := model.request(calls + 1)
	if !followUp.has("first question") || !followUp.has("answer to your question") || !followUp.has("follow-up question") {
		t.Fatalf("resumed request lacks the earlier conversation: %s", followUp.Body)
	}
	// The failed session can be retried by sending again.
	h.send(ids[1], "now succeed")
	h.expect(ids[1], engine.Succeeded)
}

func TestTwoSessionsRunAtTheSameTimeWithoutCrossTalk(t *testing.T) {
	var mu sync.Mutex
	arrived := map[string]bool{}
	both := make(chan struct{})
	model := newFakeModel(t, provider.OpenAIChatKind)
	model.next = func(r modelRequest) reply {
		_, content := r.last()
		marker := "alpha"
		if strings.Contains(content, "beta") {
			marker = "beta"
		}
		if strings.Contains(content, "round one") {
			mu.Lock()
			arrived[marker] = true
			if len(arrived) == 2 {
				close(both)
			}
			mu.Unlock()
			// Both Turns are in flight before either answers.
			select {
			case <-both:
			case <-time.After(10 * time.Second):
				return reply{Status: 500}
			}
		}
		return text("reply-to-", marker)
	}
	h := openHub(t, liveConfig(t, model))
	a := h.create("alpha session", "")
	b := h.create("beta session", "")
	h.send(a.ID, "alpha round one")
	h.send(b.ID, "beta round one")
	h.expect(a.ID, engine.Succeeded)
	h.expect(b.ID, engine.Succeeded)
	h.send(a.ID, "alpha round two")
	h.send(b.ID, "beta round two")
	h.expect(a.ID, engine.Succeeded)
	h.expect(b.ID, engine.Succeeded)
	for _, r := range model.all() {
		if r.has("alpha") && r.has("beta") {
			t.Fatalf("request %d mixes both conversations: %s", r.N, r.Body)
		}
	}
	for id, mine := range map[string]string{a.ID: "alpha", b.ID: "beta"} {
		for _, block := range h.blocks(id) {
			other := "beta"
			if mine == "beta" {
				other = "alpha"
			}
			if strings.Contains(block.Text, other) {
				t.Fatalf("%s session shows %q", mine, block.Text)
			}
		}
		replies := blocksOf(h.blocks(id), product.BlockKindReply)
		if len(replies) != 2 || replies[1].Text != "reply-to-"+mine {
			t.Fatalf("%s replies %+v", mine, replies)
		}
	}
}

// A double click on Send admits one Turn; the second submission is refused
// while the first runs, and nothing is sent twice.
func TestDoubleSendAdmitsExactlyOneTurn(t *testing.T) {
	hold, reached := make(chan struct{}), make(chan struct{})
	model := newFakeModel(t, provider.OpenAIChatKind, reply{Text: []string{"only once"}, Hold: hold, Reached: reached})
	h := openHub(t, liveConfig(t, model))
	s := h.create("double send", "")
	var wg sync.WaitGroup
	errs := make([]error, 2)
	for i := range errs {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			_, errs[i] = h.service().Submit(context.Background(), s.ID, "same message", nil)
		}(i)
	}
	wg.Wait()
	received(t, reached, "the admitted Turn")
	if (errs[0] == nil) == (errs[1] == nil) {
		t.Fatalf("double send results %v / %v, want exactly one admitted", errs[0], errs[1])
	}
	close(hold)
	h.expect(s.ID, engine.Succeeded)
	inputs := 0
	for _, e := range h.history(s.ID) {
		if e.Input != "" {
			inputs++
		}
	}
	if inputs != 1 || model.calls() != 1 {
		t.Fatalf("inputs %d, model requests %d", inputs, model.calls())
	}
}
