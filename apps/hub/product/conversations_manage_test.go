// INPUT: Real product service fixture with the local Alva engine
// OUTPUT: Title-rule table, rename/pin/delete service rules and the stopped-reply Continue action
// POS: [L7 Hub product] Management and Continue go through the narrow Conversations/Turns services; no windows or external engines
package product

import (
	"errors"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

func TestSessionTitleIsOneRule(t *testing.T) {
	long := strings.Repeat("字", 40)
	for _, c := range []struct{ explicit, prompt, want string }{
		{"", "  first line  \nsecond", "first line"},
		{"", "\n\n  after blanks\nx", "after blanks"},
		{"", long, strings.Repeat("字", 32) + "…"},
		{"", strings.Repeat("字", 32), strings.Repeat("字", 32)},
		{"explicit title\nmore", "ignored prompt", "explicit title"},
		{"  ", "", DefaultSessionTitle},
		{"", "   \n ", DefaultSessionTitle},
		{"", "first\rsecond", "first"},
		{"", "a\x00b\u202ec", "abc"},
		{"", "\u200b\u2067", DefaultSessionTitle},
	} {
		if got := SessionTitle(c.explicit, c.prompt); got != c.want {
			t.Fatalf("SessionTitle(%q,%q)=%q want %q", c.explicit, c.prompt, got, c.want)
		}
	}
	if _, err := RenameTitle("   "); err == nil {
		t.Fatal("blank rename accepted")
	}
	if _, err := RenameTitle(strings.Repeat("a", 65)); err == nil {
		t.Fatal("over-long rename accepted")
	}
	for _, bad := range []string{"a\x00b", "x\u202ey", "a\u200bb", "tab\there"} {
		if _, err := RenameTitle(bad); err == nil {
			t.Fatalf("rename accepted control characters in %q", bad)
		}
	}
	if got, err := RenameTitle("lone\rcr"); err != nil || got != "lone" {
		t.Fatal("a lone CR must end the title line", got, err)
	}
	if got, err := RenameTitle("  新标题 \n 第二行"); err != nil || got != "新标题" {
		t.Fatal("rename not normalized", got, err)
	}
}

func TestConversationManagementRules(t *testing.T) {
	s, _ := fixture(t)
	ctx := t.Context()
	a, err := s.Create(ctx, "alpha", "")
	if err != nil {
		t.Fatal(err)
	}
	b, err := s.Create(ctx, "beta", "")
	if err != nil {
		t.Fatal(err)
	}
	c := s.Conversations
	if err = c.Rename(ctx, a.ID, "  Alpha 2 "); err != nil {
		t.Fatal(err)
	}
	if err = c.Rename(ctx, a.ID, ""); err == nil {
		t.Fatal("empty rename accepted")
	}
	if err = c.SetPinned(ctx, b.ID, true); err != nil {
		t.Fatal(err)
	}
	pinned, err := c.Pinned(ctx)
	if err != nil || len(pinned) != 1 || pinned[0].ID != b.ID {
		t.Fatal("pinned wrong", pinned, err)
	}
	// A running session refuses deletion; after it ends the delete succeeds.
	start, err := s.Turns.Send(ctx, a.ID, "do work", nil)
	if err != nil {
		t.Fatal(err)
	}
	if err = c.Delete(ctx, a.ID); !errors.Is(err, hub.ErrSessionRunning) {
		<-start.Done
		t.Fatal("running session was deletable", err)
	}
	<-start.Done
	waitIdle(t, s, a.ID)
	if err = c.Delete(ctx, a.ID); err != nil {
		t.Fatal(err)
	}
	if _, err = c.Detail(ctx, a.ID); !errors.Is(err, hub.ErrNotFound) {
		t.Fatal("deleted session still addressable", err)
	}
	if _, err = s.Turns.Send(ctx, a.ID, "late", nil); !errors.Is(err, hub.ErrNotFound) {
		t.Fatal("deleted session accepted a Turn", err)
	}
	page, err := c.Page(ctx, "", hub.SessionCursor{}, 10)
	if err != nil || len(page.Sessions) != 1 || page.Sessions[0].ID != b.ID {
		t.Fatal("deleted session in page", page, err)
	}
	// The data is still on disk behind the product store.
	if raw, err := s.Store.DetailIncludingDeleted(ctx, a.ID); err != nil || raw.Session.DeletedAt == 0 || raw.Session.Title != "Alpha 2" {
		t.Fatal("soft delete must keep the session record", raw, err)
	}
}

func TestContinueOnlyAfterAStoppedReply(t *testing.T) {
	s, requests := fixture(t)
	ctx := t.Context()
	p, err := s.Create(ctx, "continue", "")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.Turns.Continue(ctx, p.ID); !errors.Is(err, ErrNothingToContinue) {
		t.Fatal("continue without any turn must be refused", err)
	}
	first, err := s.Turns.Send(ctx, p.ID, "write something", nil)
	if err != nil {
		t.Fatal(err)
	}
	if err = <-first.Done; err != nil {
		t.Fatal(err)
	}
	waitIdle(t, s, p.ID)
	if _, err = s.Turns.Continue(ctx, p.ID); !errors.Is(err, ErrNothingToContinue) {
		t.Fatal("a completed reply must not offer continue", err)
	}
	for len(requests) > 0 {
		<-requests
	}
	// A Turn admitted and then stopped, the state a Stop leaves behind.
	stopped, err := s.Store.BeginTurn(ctx, p.ID, "stopped midway")
	if err != nil {
		t.Fatal(err)
	}
	if err = s.Store.FinishTurn(ctx, stopped.Turn.ID, engine.Cancelled); err != nil {
		t.Fatal(err)
	}
	second, err := s.Turns.Continue(ctx, p.ID)
	if err != nil {
		t.Fatal(err)
	}
	if second.ExecutionID == "" {
		t.Fatal("continue must return the execution identity Stop needs")
	}
	if err = <-second.Done; err != nil {
		t.Fatal(err)
	}
	request := <-requests
	if !strings.Contains(request.Prompt, ContinuePrompt) {
		t.Fatal("the fixed continue instruction did not reach the engine", request.Prompt)
	}
	history, err := s.Conversations.History(ctx, p.ID, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	var continued, userEchoes int
	for _, entry := range history {
		if entry.TurnID == second.TurnID && entry.Origin == hub.TurnOriginContinue {
			continued++
		}
		if entry.TurnID == second.TurnID && entry.Input != "" {
			userEchoes++
		}
	}
	if continued != 1 || userEchoes != 0 {
		t.Fatal("continue turn must be marked and carry no user text", continued, userEchoes)
	}
	// Transcript: a status block, no user message for the continue turn.
	chunk, err := (&TranscriptService{Store: s.Store}).Tail(ctx, p.ID)
	if err != nil {
		t.Fatal(err)
	}
	var window TranscriptWindow
	window.Reset(chunk)
	var status, user int
	for _, block := range window.Blocks() {
		if block.TurnID != second.TurnID {
			continue
		}
		if block.Kind == BlockKindStatus && block.Text == "继续生成" {
			status++
		}
		if block.Kind == BlockKindUser {
			user++
		}
	}
	if status != 1 || user != 0 {
		t.Fatal("continue turn must show one 继续生成 status and no user block", status, user)
	}
}
