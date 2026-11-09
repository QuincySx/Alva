// INPUT: hubFixture real Alva assembly, fakeModel streams held at a model boundary, TurnService.Queue/Interject/Queued and restarts
// OUTPUT: Scenarios for a message interjected into a running Alva Turn, a queued message started after the Turn, and a queue that survives a restart without resending
// POS: [scenario] Proves what the model is really sent when the user types while it works, and that the Message Stream shows it where it arrived
package scenarios

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/llm/provider"
)

func TestInterjectionIsSeenByTheModelAfterTheToolResultAndShownInTheStream(t *testing.T) {
	for _, kind := range []provider.Kind{provider.OpenAIChatKind, provider.AnthropicKind} {
		t.Run(string(kind), func(t *testing.T) {
			workspace := t.TempDir()
			if err := os.WriteFile(filepath.Join(workspace, "notes.txt"), []byte("project notes\n"), 0o600); err != nil {
				t.Fatal(err)
			}
			hold, reached := make(chan struct{}), make(chan struct{})
			released := false
			defer func() {
				if !released {
					close(hold)
				}
			}()
			model := newFakeModel(t, kind,
				reply{Text: []string{"let me look"}, Hold: hold, Reached: reached, Calls: []toolCall{{ID: "c-read", Name: "read_file", Args: `{"path":"notes.txt"}`}}},
				text("noted, I also considered it"),
			)
			h := openHub(t, liveConfig(t, model))
			ctx := context.Background()
			s := h.create("interject", workspace)
			if err := h.service().SetPermissionPolicy(ctx, s.ID, hub.PermissionFull, true); err != nil {
				t.Fatal(err)
			}
			if capabilities, err := h.service().Turns.Capabilities(ctx, s.ID); err != nil || !capabilities.Interject || !capabilities.Fork {
				t.Fatal("Alva takes interjections and forks natively", capabilities, err)
			}
			start, err := h.service().Turns.Send(ctx, s.ID, "read the notes", nil)
			if err != nil {
				t.Fatal(err)
			}
			received(t, reached, "the first model request")
			result, err := h.service().Turns.Interject(ctx, s.ID, start.ExecutionID, "INTERJECT-NOTE also compare with last year")
			if err != nil || result.Mode != product.InterjectSent {
				t.Fatal("interjection must reach the running Alva Turn", result, err)
			}
			released = true
			close(hold)
			h.expect(s.ID, engine.Succeeded)

			first, second := model.request(1), model.request(2)
			if first.has("INTERJECT-NOTE") {
				t.Fatal("the request already in flight cannot contain the interjection")
			}
			body := string(second.Body)
			result1, note := max(strings.Index(body, `tool_call_id":"c-read"`), strings.Index(body, `tool_use_id":"c-read"`)), strings.Index(body, "INTERJECT-NOTE")
			if result1 < 0 || note < 0 || note < result1 {
				t.Fatalf("the interjection must follow the tool result in the next request (result at %d, note at %d): %s", result1, note, body)
			}
			if role, content := second.last(); role != "user" || !strings.Contains(content, "INTERJECT-NOTE") {
				t.Fatalf("the interjection must be the last, user, message: %s %s", role, content)
			}
			check := func(when string) {
				t.Helper()
				var shown []string
				for _, b := range blocksOf(h.blocks(s.ID), product.BlockKindUser) {
					shown = append(shown, b.Name+":"+b.Text)
				}
				if got := strings.Join(shown, "|"); got != ":read the notes|interjection:INTERJECT-NOTE also compare with last year" {
					t.Fatalf("%s: user blocks %q: %s", when, got, h.dump(s.ID))
				}
				if r := lastReply(t, h.blocks(s.ID)); r.Text != "noted, I also considered it" {
					t.Fatalf("%s: reply %q", when, r.Text)
				}
				if queue, _ := h.service().Turns.Queued(ctx, s.ID); len(queue) != 0 {
					t.Fatalf("%s: the delivered interjection must leave the queue: %+v", when, queue)
				}
			}
			check("live")
			h.reopen()
			check("after restart")
			if model.calls() != 2 {
				t.Fatal("an interjection is not a new Turn; model calls", model.calls())
			}
		})
	}
}

func TestQueuedMessageStartsAfterTheTurnAndIsSeenWithTheFullContext(t *testing.T) {
	hold, reached := make(chan struct{}), make(chan struct{})
	released := false
	defer func() {
		if !released {
			close(hold)
		}
	}()
	model := newFakeModel(t, provider.OpenAIChatKind,
		reply{Text: []string{"working "}, Hold: hold, Reached: reached, After: []string{"done"}},
		text("second answer"),
	)
	h := openHub(t, liveConfig(t, model))
	ctx := context.Background()
	s := h.create("queue", "")
	h.send(s.ID, "first request")
	received(t, reached, "the running reply")
	item, err := h.service().Turns.Queue(ctx, s.ID, "SECOND-REQUEST")
	if err != nil {
		t.Fatal(err)
	}
	edited, err := h.service().Turns.EditQueued(ctx, s.ID, item.ID, "SECOND-REQUEST edited")
	if err != nil || edited.Text != "SECOND-REQUEST edited" {
		t.Fatal(edited, err)
	}
	// The queue survives a restart without starting anything by itself.
	released = true
	close(hold)
	h.expect(s.ID, engine.Succeeded)
	waitFor(t, "the queued message to become a Turn", func() bool { return model.calls() == 2 })
	h.expect(s.ID, engine.Succeeded)
	request := model.request(2)
	if !request.has("first request") || !request.has("working done") || !request.typed("SECOND-REQUEST edited") {
		t.Fatalf("the queued message must be sent with the conversation before it: %s", request.Body)
	}
	if queue, _ := h.service().Turns.Queued(ctx, s.ID); len(queue) != 0 {
		t.Fatal("a sent message leaves the queue", queue)
	}
	if got := conversationText(h.blocks(s.ID)); got != "U:first request|A:working done|U:SECOND-REQUEST edited|A:second answer" {
		t.Fatal("conversation", got)
	}
}

func TestQueueSurvivesARestartWithoutResending(t *testing.T) {
	hold, reached := make(chan struct{}), make(chan struct{})
	defer close(hold)
	model := newFakeModel(t, provider.OpenAIChatKind, reply{Text: []string{"partial"}, Hold: hold, Reached: reached}, text("later"))
	h := openHub(t, liveConfig(t, model))
	ctx := context.Background()
	s := h.create("crash", "")
	h.send(s.ID, "long task")
	received(t, reached, "the running reply")
	if _, err := h.service().Turns.Queue(ctx, s.ID, "QUEUED-ACROSS-RESTART"); err != nil {
		t.Fatal(err)
	}
	// The application exits mid-Turn: closing cancels the run, the queue stays,
	// and nothing is sent by itself after the restart (a cancelled Turn never
	// starts the queue).
	h.reopen()
	if got := h.settle(s.ID); got != engine.Cancelled && got != engine.Interrupted {
		t.Fatal("the Turn that was running ends cancelled or interrupted after a restart", got)
	}
	queue, err := h.service().Turns.Queued(ctx, s.ID)
	if err != nil || len(queue) != 1 || queue[0].Text != "QUEUED-ACROSS-RESTART" || queue[0].State != hub.PendingQueued {
		t.Fatal("the queue must survive the restart", queue, err)
	}
	if model.calls() != 1 {
		t.Fatal("a restart must not send queued messages by itself", model.calls())
	}
	// The user resumes it explicitly.
	if _, err = h.service().Turns.SendQueued(ctx, s.ID, ""); err != nil {
		t.Fatal(err)
	}
	h.expect(s.ID, engine.Succeeded)
	if !model.request(2).typed("QUEUED-ACROSS-RESTART") {
		t.Fatal("the resumed message was not sent", model.request(2).Body)
	}
}
