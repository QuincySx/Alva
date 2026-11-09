// INPUT: hubFixture real Alva assembly, fakeModel recorded requests, TurnService.Edit/Regenerate/SelectVersion and ConversationService.Versions/Fork
// OUTPUT: Scenarios for edit-and-resend, regenerate, switching versions across restarts, and forking into a new conversation with the model's real context
// POS: [scenario] Proves versions and forks through the product entries: what the model is sent, what the Message Stream shows and what survives a restart
package scenarios

import (
	"context"
	"errors"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/llm/provider"
	private "github.com/QuincySx/Alva/platform/storage/sqlite"
)

// conversationText renders the visible conversation as "U:..|A:.." in order.
func conversationText(blocks []product.Block) string {
	var parts []string
	for _, b := range blocks {
		switch b.Kind {
		case product.BlockKindUser:
			parts = append(parts, "U:"+b.Text)
		case product.BlockKindReply:
			parts = append(parts, "A:"+b.Text)
		}
	}
	return strings.Join(parts, "|")
}

func sendAndSettle(t *testing.T, h *hubFixture, id, prompt string) string {
	t.Helper()
	turn := h.send(id, prompt)
	h.expect(id, engine.Succeeded)
	return turn
}

func TestEditResendKeepsTheOldVersionAndTheModelSeesOnlyTheEditedHistory(t *testing.T) {
	for _, kind := range []provider.Kind{provider.OpenAIChatKind, provider.AnthropicKind} {
		t.Run(string(kind), func(t *testing.T) {
			model := newFakeModel(t, kind, text("reply-alpha"), text("reply-bravo"), text("reply-bravo-edited"), text("reply-charlie"))
			h := openHub(t, liveConfig(t, model))
			ctx := context.Background()
			s := h.create("edit", "")
			sendAndSettle(t, h, s.ID, "alpha")
			bravo := sendAndSettle(t, h, s.ID, "bravo")
			nativeBefore := h.branch(s.ID).NativeSessionID

			start, err := h.service().Turns.Edit(ctx, s.ID, bravo, "bravo-edited")
			if err != nil {
				t.Fatal(err)
			}
			h.expect(s.ID, engine.Succeeded)
			request := model.request(3)
			if !request.has("alpha") || !request.has("reply-alpha") || !request.has("bravo-edited") {
				t.Fatalf("the edited request lost the kept context: %s", request.Body)
			}
			if request.has("reply-bravo") || request.has(`"bravo"`) {
				t.Fatalf("the edited request still carries the replaced Turn: %s", request.Body)
			}
			if h.branch(s.ID).NativeSessionID == nativeBefore || h.branch(s.ID).NativeSessionID == "" {
				t.Fatal("the new version must run on its own native session")
			}
			check := func(when, want string) {
				t.Helper()
				if got := conversationText(h.blocks(s.ID)); got != want {
					t.Fatalf("%s: conversation %q, want %q", when, got, want)
				}
			}
			check("edited", "U:alpha|A:reply-alpha|U:bravo-edited|A:reply-bravo-edited")
			versions, err := h.service().Conversations.Versions(ctx, s.ID)
			if err != nil {
				t.Fatal(err)
			}
			switcher, ok := versions[start.TurnID]
			if !ok || switcher.Count != 2 || switcher.Index != 2 {
				t.Fatalf("the edited user block must show 2/2: %+v", versions)
			}
			original := switcher.Choices[0]
			if original.Active || original.TurnID != bravo {
				t.Fatalf("the first version is the original Turn: %+v", switcher.Choices)
			}
			// Switch back: the original conversation is whole again.
			if err = h.service().Turns.SelectVersion(ctx, s.ID, original.LineID); err != nil {
				t.Fatal(err)
			}
			check("switched back", "U:alpha|A:reply-alpha|U:bravo|A:reply-bravo")
			versions, _ = h.service().Conversations.Versions(ctx, s.ID)
			if sw := versions[bravo]; sw.Count != 2 || sw.Index != 1 {
				t.Fatalf("the original user block must show 1/2: %+v", versions)
			}
			// A new message continues the original version, not the edited one.
			sendAndSettle(t, h, s.ID, "charlie")
			next := model.request(4)
			if !next.has("reply-bravo") || !next.has("charlie") || next.has("bravo-edited") || next.has("reply-bravo-edited") {
				t.Fatalf("the original version saw the edited one: %s", next.Body)
			}
			check("continued", "U:alpha|A:reply-alpha|U:bravo|A:reply-bravo|U:charlie|A:reply-charlie")
			// Everything survives a restart, and the other version is still there.
			h.reopen()
			check("after restart", "U:alpha|A:reply-alpha|U:bravo|A:reply-bravo|U:charlie|A:reply-charlie")
			versions, _ = h.service().Conversations.Versions(ctx, s.ID)
			sw := versions[bravo]
			if sw.Count != 2 || sw.Index != 1 {
				t.Fatalf("versions after restart: %+v", versions)
			}
			if err = h.service().Turns.SelectVersion(ctx, s.ID, sw.Choices[1].LineID); err != nil {
				t.Fatal(err)
			}
			check("edited after restart", "U:alpha|A:reply-alpha|U:bravo-edited|A:reply-bravo-edited")
		})
	}
}

func TestRegenerateAsksAgainWithTheSamePromptAndKeepsBothReplies(t *testing.T) {
	model := newFakeModel(t, provider.OpenAIChatKind, text("first-answer"), text("second-answer"))
	h := openHub(t, liveConfig(t, model))
	ctx := context.Background()
	s := h.create("regenerate", "")
	turn := sendAndSettle(t, h, s.ID, "question")
	if _, err := h.service().Turns.Regenerate(ctx, s.ID, turn); err != nil {
		t.Fatal(err)
	}
	h.expect(s.ID, engine.Succeeded)
	request := model.request(2)
	if !request.has("question") || request.has("first-answer") {
		t.Fatalf("the regenerated request must repeat the prompt without the old answer: %s", request.Body)
	}
	if strings.Count(string(request.Body), "question") != 1 {
		t.Fatalf("the prompt must be sent once: %s", request.Body)
	}
	if got := conversationText(h.blocks(s.ID)); got != "U:question|A:second-answer" {
		t.Fatal("regenerated conversation", got)
	}
	versions, err := h.service().Conversations.Versions(ctx, s.ID)
	if err != nil || len(versions) != 1 {
		t.Fatal("one switcher expected", versions, err)
	}
	var shown product.VersionSwitch
	for _, v := range versions {
		shown = v
	}
	if shown.Count != 2 || shown.Index != 2 {
		t.Fatalf("two versions, the newest shown: %+v", shown)
	}
	if err = h.service().Turns.SelectVersion(ctx, s.ID, shown.Choices[0].LineID); err != nil {
		t.Fatal(err)
	}
	if got := conversationText(h.blocks(s.ID)); got != "U:question|A:first-answer" {
		t.Fatal("the first reply is still there", got)
	}
	// Only the last reply can be regenerated, and only while idle.
	if _, err = h.service().Turns.Regenerate(ctx, s.ID, "turn-unknown"); !errors.Is(err, hub.ErrNotFound) {
		t.Fatal("unknown Turn", err)
	}
}

func TestVersionsAndForksAreRefusedWhileRunning(t *testing.T) {
	hold, reached := make(chan struct{}), make(chan struct{})
	defer func() {
		select {
		case <-hold:
		default:
			close(hold)
		}
	}()
	model := newFakeModel(t, provider.OpenAIChatKind, text("a1"), reply{Text: []string{"slow"}, Hold: hold, Reached: reached})
	h := openHub(t, liveConfig(t, model))
	ctx := context.Background()
	s := h.create("busy", "")
	first := sendAndSettle(t, h, s.ID, "one")
	h.send(s.ID, "two")
	received(t, reached, "the running reply")
	if _, err := h.service().Turns.Edit(ctx, s.ID, first, "x"); !errors.Is(err, hub.ErrVersionBusy) {
		t.Fatal("editing while running must be refused", err)
	}
	if _, err := h.service().Turns.Regenerate(ctx, s.ID, first); !errors.Is(err, hub.ErrVersionBusy) {
		t.Fatal("regenerating while running must be refused", err)
	}
	if err := h.service().Turns.SelectVersion(ctx, s.ID, "line-x"); !errors.Is(err, hub.ErrVersionBusy) {
		t.Fatal("switching while running must be refused", err)
	}
	if _, err := h.service().Conversations.Fork(ctx, s.ID, first); !errors.Is(err, hub.ErrSessionRunning) {
		t.Fatal("forking a running conversation must be refused", err)
	}
	close(hold)
	h.expect(s.ID, engine.Succeeded)
	reason, err := h.service().Conversations.VersionPolicy(ctx, s.ID, first)
	if err != nil || reason != "" {
		t.Fatal("an ordinary Turn is versionable", reason, err)
	}
}

func TestForkContinuesFromTheKeptTurnInANewConversationWithTheModelsRealContext(t *testing.T) {
	model := newFakeModel(t, provider.OpenAIChatKind, text("reply-alpha"), text("reply-bravo"), text("reply-charlie"), text("reply-foxtrot"), text("reply-golf"), text("reply-delta"), text("reply-echo"))
	h := openHub(t, liveConfig(t, model))
	ctx := context.Background()
	s := h.create("fork source", "")
	sendAndSettle(t, h, s.ID, "alpha")
	bravo := sendAndSettle(t, h, s.ID, "bravo")
	sendAndSettle(t, h, s.ID, "charlie")
	sourceNative := h.branch(s.ID).NativeSessionID

	forked, err := h.service().Conversations.Fork(ctx, s.ID, bravo)
	if err != nil {
		t.Fatal(err)
	}
	if forked.Degraded {
		t.Fatal("Alva can fork natively; the fork must not be degraded")
	}
	child := forked.Session
	if child.ID == s.ID || child.ForkedFrom == nil || child.ForkedFrom.SessionID != s.ID || child.ForkedFrom.TurnID != bravo || child.ForkedFrom.Title != "fork source" {
		t.Fatalf("the fork must record where it came from: %+v", child.ForkedFrom)
	}
	if blocks := h.blocks(child.ID); len(blocks) != 0 {
		t.Fatal("a fork copies no history", conversationText(blocks))
	}
	// The origin points at a real place in the source: reading around it shows
	// the kept Turn's last event.
	around, err := h.service().Conversations.History(ctx, s.ID, child.ForkedFrom.EventSequence-1, 1)
	if err != nil || len(around) != 1 || around[0].TurnID != bravo {
		t.Fatal("the fork link must lead to the kept Turn", around, err)
	}
	// The source conversation goes on by two Turns before the fork's first
	// message: the cut was fixed when the fork was made, so they stay out.
	sendAndSettle(t, h, s.ID, "foxtrot")
	sendAndSettle(t, h, s.ID, "golf")
	sendAndSettle(t, h, child.ID, "delta")
	request := model.request(6)
	for _, want := range []string{"alpha", "reply-alpha", "bravo", "reply-bravo", "delta"} {
		if !request.has(want) {
			t.Fatalf("the forked conversation lost %q: %s", want, request.Body)
		}
	}
	for _, unwanted := range []string{"charlie", "reply-charlie", "foxtrot", "reply-foxtrot", "golf", "reply-golf"} {
		if request.has(unwanted) {
			t.Fatalf("the forked conversation leaked %q: %s", unwanted, request.Body)
		}
	}
	if h.branch(child.ID).NativeSessionID == "" || h.branch(child.ID).NativeSessionID == sourceNative {
		t.Fatal("the fork runs on its own native session")
	}
	// The source is untouched and continues with its own full context.
	sendAndSettle(t, h, s.ID, "echo")
	back := model.request(7)
	if !back.has("charlie") || !back.has("reply-charlie") || !back.has("golf") || back.has("delta") || back.has("reply-delta") {
		t.Fatalf("the source conversation changed: %s", back.Body)
	}
	if h.branch(s.ID).NativeSessionID != sourceNative {
		t.Fatal("the source keeps its native session")
	}
	if got := conversationText(h.blocks(child.ID)); got != "U:delta|A:reply-delta" {
		t.Fatal("fork conversation", got)
	}
	// The link opens the source around the fork point: the kept Turn and what
	// came after it are both in the window.
	aroundChunk, err := h.service().Transcripts.Around(ctx, s.ID, child.ForkedFrom.EventSequence)
	if err != nil {
		t.Fatal(err)
	}
	window := product.NewTranscriptWindow(0)
	window.Reset(aroundChunk)
	if got := conversationText(window.Blocks()); !strings.Contains(got, "U:bravo|A:reply-bravo|U:charlie|A:reply-charlie") || window.MoreAfter() {
		t.Fatal("around the fork point", got, window.MoreAfter())
	}
	// The link survives a restart.
	h.reopen()
	detail, err := h.service().Conversations.Detail(ctx, child.ID)
	if err != nil || detail.Session.ForkedFrom == nil || detail.Session.ForkedFrom.SessionID != s.ID {
		t.Fatal("fork origin after restart", detail.Session.ForkedFrom, err)
	}
}

func TestForkOfAnEditedVersionKeepsOnlyThatVersionsContext(t *testing.T) {
	model := newFakeModel(t, provider.OpenAIChatKind, text("reply-alpha"), text("reply-bravo"), text("reply-bravo-edited"), text("reply-next"))
	h := openHub(t, liveConfig(t, model))
	ctx := context.Background()
	s := h.create("fork versions", "")
	sendAndSettle(t, h, s.ID, "alpha")
	bravo := sendAndSettle(t, h, s.ID, "bravo")
	start, err := h.service().Turns.Edit(ctx, s.ID, bravo, "bravo-edited")
	if err != nil {
		t.Fatal(err)
	}
	h.expect(s.ID, engine.Succeeded)
	forked, err := h.service().Conversations.Fork(ctx, s.ID, start.TurnID)
	if err != nil {
		t.Fatal(err)
	}
	sendAndSettle(t, h, forked.Session.ID, "next")
	request := model.request(4)
	if !request.has("bravo-edited") || !request.has("reply-bravo-edited") || request.has("reply-bravo\"") || !request.has("next") {
		t.Fatalf("fork of the edited version: %s", request.Body)
	}
	if _, err = h.service().Conversations.Fork(ctx, s.ID, bravo); !errors.Is(err, hub.ErrNotFound) {
		t.Fatal("a Turn of the hidden version cannot be forked", err)
	}
}

func TestEditedVersionRetriedAfterAFailureAndSwitchingKeepsExactlyItsOwnContext(t *testing.T) {
	model := newFakeModel(t, provider.OpenAIChatKind, text("reply-alpha"), text("reply-bravo"), reply{Status: 400}, text("reply-charlie"), text("reply-delta"))
	h := openHub(t, liveConfig(t, model))
	ctx := context.Background()
	s := h.create("retry", "")
	sendAndSettle(t, h, s.ID, "alpha")
	bravo := sendAndSettle(t, h, s.ID, "bravo")
	// The edited version fails at the model.
	edit, err := h.service().Turns.Edit(ctx, s.ID, bravo, "bravo-edited")
	if err != nil {
		t.Fatal(err)
	}
	h.expect(s.ID, engine.Failed)
	versions, err := h.service().Conversations.Versions(ctx, s.ID)
	if err != nil || versions[edit.TurnID].Count != 2 {
		t.Fatal("two versions expected", versions, err)
	}
	original, edited := versions[edit.TurnID].Choices[0].LineID, versions[edit.TurnID].Choices[1].LineID
	// The original version goes on, then the edited one is retried.
	if err = h.service().Turns.SelectVersion(ctx, s.ID, original); err != nil {
		t.Fatal(err)
	}
	sendAndSettle(t, h, s.ID, "charlie")
	if err = h.service().Turns.SelectVersion(ctx, s.ID, edited); err != nil {
		t.Fatal(err)
	}
	sendAndSettle(t, h, s.ID, "delta")
	retry := model.request(model.calls())
	for _, want := range []string{"alpha", "reply-alpha", "bravo-edited", "delta"} {
		if !retry.has(want) {
			t.Fatalf("the retried version lost %q: %s", want, retry.Body)
		}
	}
	for _, unwanted := range []string{"reply-bravo", "charlie", "reply-charlie", `"bravo"`} {
		if retry.has(unwanted) {
			t.Fatalf("the retried version carries %q from another version: %s", unwanted, retry.Body)
		}
	}
}

func TestAnUnreachableForkDegradesInPlaceThroughTheRealRuntimeAndLeavesNoEmptyNativeSession(t *testing.T) {
	model := newFakeModel(t, provider.OpenAIChatKind, text("reply-alpha"), text("reply-bravo"), text("reply-delta"))
	h := openHub(t, liveConfig(t, model))
	ctx := context.Background()
	s := h.create("unreachable", "")
	alpha := sendAndSettle(t, h, s.ID, "alpha")
	sendAndSettle(t, h, s.ID, "bravo")
	forked, err := h.service().Conversations.Fork(ctx, s.ID, alpha)
	if err != nil || forked.Degraded {
		t.Fatal("the preflight agrees at fork time", forked, err)
	}
	child := forked.Session.ID
	// Between the preflight and the first message the anchor stops matching the
	// source (as after a rollback): move the recorded cut to a place no prompt
	// lies below.
	h.close()
	raw, err := private.Open(ctx, filepath.Join(h.config.DataDirectory, "product"), "product.sqlite")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = raw.ExecContext(ctx, `UPDATE hub_branches SET data=json_set(cast(data AS text),'$.fork.through_sequence',0,'$.fork.before_sequence',1) WHERE session=?`, child); err != nil {
		t.Fatal(err)
	}
	raw.Close()
	h.open()
	sendAndSettle(t, h, child, "delta")
	if model.calls() != 3 {
		t.Fatal("the model is asked once for the rescued Turn", model.calls())
	}
	request := model.request(3)
	if !request.has("[Hub handoff") || !request.has("alpha") || request.has("bravo") {
		t.Fatalf("the rerun carries the index of the kept Turn only: %s", request.Body)
	}
	if strings.Count(string(request.Body), "delta") != 1 {
		t.Fatalf("the user's prompt must appear exactly once: %s", request.Body)
	}
	notice := false
	for _, b := range h.blocks(child) {
		notice = notice || b.Kind == product.BlockKindStatus && b.Name == "notice" && strings.Contains(b.Text, "此引擎不能原生分叉")
	}
	if !notice {
		t.Fatal("fork_degraded must be visible", h.dump(child))
	}
	if page, err := h.app.Repository.List(ctx, child, "", 10); err != nil || len(page.Sessions) != 1 {
		t.Fatal("an unreachable fork must leave no empty native session on disk", page, err)
	}
	if h.branch(child).NativeSessionID == "" {
		t.Fatal("the rescued Turn runs on its own native session")
	}
}
