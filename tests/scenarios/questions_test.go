// INPUT: hubFixture real Alva assembly with the interaction tool, durable waits, the unified interaction answer and wait review commands
// OUTPUT: Scenarios for answering ask_human inline, across a restart while waiting, and abandoning a wait
// POS: [scenario] Questions the model asks the user, from the card to the model's next request.
package scenarios

import (
	"context"
	"encoding/json"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/llm/provider"
)

const askArgs = `{"question":"Which colour?","options":[{"label":"Red"},{"label":"Second choice green"}]}`

func enableQuestions(t *testing.T, h *hubFixture, id string, durable bool) {
	t.Helper()
	if err := h.service().SetToolEnabled(context.Background(), id, "interaction", true, false); err != nil {
		t.Fatal(err)
	}
	if durable {
		if err := h.service().EnableDurableWaits(context.Background(), id, true); err != nil {
			t.Fatal(err)
		}
	}
}

func TestAskHumanAnsweredInlineReachesTheModel(t *testing.T) {
	model := newFakeModel(t, provider.OpenAIChatKind, call("c-ask", "ask_human", askArgs), text("green it is"))
	h := openHub(t, liveConfig(t, model))
	s := h.create("ask", "")
	enableQuestions(t, h, s.ID, false)
	h.send(s.ID, "pick a colour with me")
	item := h.pending(s.ID)
	if item.View.Kind != product.InteractionKindQuestion || len(item.View.Questions) != 1 || len(item.View.Questions[0].Options) != 2 {
		t.Fatalf("question card %+v", item.View)
	}
	q := item.View.Questions[0]
	if err := h.service().RespondInteraction(context.Background(), s.ID, item, product.InteractionResponse{Answers: map[string]product.InteractionAnswer{q.ID: {QuestionID: q.ID, Options: []int{1}}}}); err != nil {
		t.Fatal(err)
	}
	h.expect(s.ID, engine.Succeeded)
	// The model gets the chosen option index (1-based) and a reference to
	// the complete answer text.
	if r := model.request(2).toolResults()["c-ask"]; !contains(r, `\"selected\":[2]`) || !contains(r, `\"answer\":{\"uri\"`) {
		t.Fatalf("the answer did not reach the model: %s", r)
	}
	if err := h.service().RespondInteraction(context.Background(), s.ID, item, product.InteractionResponse{Answers: map[string]product.InteractionAnswer{q.ID: {QuestionID: q.ID, Options: []int{0}}}}); err == nil {
		t.Fatal("a second answer to the settled question was accepted")
	}
}

func TestAskHumanWaitSurvivesRestartAndTheAnswerStillCounts(t *testing.T) {
	model := newFakeModel(t, provider.OpenAIChatKind, call("c-ask", "ask_human", askArgs), text("green after restart"))
	h := openHub(t, liveConfig(t, model))
	s := h.create("durable ask", "")
	enableQuestions(t, h, s.ID, true)
	turn := h.send(s.ID, "ask me and wait")
	h.expect(s.ID, engine.Waiting)
	if b := toolBlock(t, h.blocks(s.ID), "c-ask"); b.Status != product.BlockStatusWaiting {
		t.Fatalf("waiting question block %+v", b)
	}
	h.reopen()
	view, err := h.service().Waits(context.Background(), s.ID)
	if err != nil {
		t.Fatal(err)
	}
	if view.Status != engine.Waiting || view.TurnID != turn || len(view.Snapshot.State.Requests) != 1 || view.Snapshot.State.Requests[0].Kind != "question" || model.calls() != 1 {
		t.Fatalf("wait after restart %+v (model calls %d)", view, model.calls())
	}
	request := view.Snapshot.State.Requests[0]
	target := product.WaitTarget{BranchID: view.BranchID, TurnID: view.TurnID, Revision: view.Snapshot.Revision}
	if err := h.service().ResolveWait(context.Background(), s.ID, product.WaitResolution{Target: target, RequestID: request.ID, Binding: request.Binding, Result: json.RawMessage(`{"selected":[2]}`)}); err != nil {
		t.Fatal(err)
	}
	view, err = h.service().Waits(context.Background(), s.ID)
	if err != nil || !view.CanResume {
		t.Fatalf("resolved wait cannot continue: %+v %v", view, err)
	}
	resumed, err := h.service().ResumeWait(context.Background(), s.ID, product.WaitTarget{BranchID: view.BranchID, TurnID: view.TurnID, Revision: view.Snapshot.Revision})
	if err != nil || resumed != turn {
		t.Fatalf("resume %q %v", resumed, err)
	}
	h.expect(s.ID, engine.Succeeded)
	// The yielded call keeps its placeholder; the answer arrives as an
	// explicit durable result naming the call and the chosen option.
	if r := model.request(2); !r.has(`\"call_id\":\"c-ask\"`) || !r.has(`\"result\":{\"selected\":[2]}`) {
		t.Fatalf("the answer given after restart did not reach the model: %s", r.Body)
	}
	if b := toolBlock(t, h.blocks(s.ID), "c-ask"); b.Status != product.BlockStatusCompleted {
		t.Fatalf("question block after continuing %+v", b)
	}
}

// A new session needs no setting: the question waits, survives the app
// closing and still reaches the model once answered.
func TestDurableWaitsAreOnByDefaultAndSurviveRestart(t *testing.T) {
	model := newFakeModel(t, provider.OpenAIChatKind, call("c-ask", "ask_human", askArgs), text("green by default"))
	h := openHub(t, liveConfig(t, model))
	fresh := h.createDefault("untouched", "")
	view, err := h.service().Waits(context.Background(), fresh.ID)
	if err != nil || !view.Enabled {
		t.Fatalf("a new session did not start with waits on: %+v %v", view, err)
	}
	if err := h.service().SetToolEnabled(context.Background(), fresh.ID, "interaction", true, false); err != nil {
		t.Fatal(err)
	}
	turn := h.send(fresh.ID, "ask me and wait")
	h.expect(fresh.ID, engine.Waiting)
	h.reopen()
	view, err = h.service().Waits(context.Background(), fresh.ID)
	if err != nil || view.Status != engine.Waiting || view.TurnID != turn || len(view.Snapshot.State.Requests) != 1 {
		t.Fatalf("the default wait did not survive the restart: %+v %v", view, err)
	}
}

func TestAbandoningAWaitingQuestionLetsTheUserMoveOn(t *testing.T) {
	model := newFakeModel(t, provider.OpenAIChatKind, call("c-ask", "ask_human", askArgs), text("new topic"))
	h := openHub(t, liveConfig(t, model))
	s := h.create("abandon", "")
	enableQuestions(t, h, s.ID, true)
	h.send(s.ID, "ask me")
	h.expect(s.ID, engine.Waiting)
	if _, err := h.service().Submit(context.Background(), s.ID, "something else", nil); err == nil {
		t.Fatal("a new message bypassed the open question")
	}
	view, err := h.service().Waits(context.Background(), s.ID)
	if err != nil {
		t.Fatal(err)
	}
	if err := h.service().AbandonWait(context.Background(), s.ID, product.WaitTarget{BranchID: view.BranchID, TurnID: view.TurnID, Revision: view.Snapshot.Revision}); err != nil {
		t.Fatal(err)
	}
	if status := h.detail(s.ID).Session.Status; status == engine.Waiting {
		t.Fatalf("session still waiting after abandon: %s", status)
	}
	if b := toolBlock(t, h.blocks(s.ID), "c-ask"); b.Status == product.BlockStatusWaiting {
		t.Fatalf("abandoned question still shows waiting: %+v", b)
	}
	h.send(s.ID, "something else")
	h.expect(s.ID, engine.Succeeded)
	if model.calls() != 2 {
		t.Fatalf("model requests %d", model.calls())
	}
}

// contains is strings.Contains for scenario assertions.
func contains(haystack, needle string) bool { return strings.Contains(haystack, needle) }

// With durable waits turned off, closing the application while a
// question waits stops that Turn; the question is gone after reopening and the
// user simply continues.
func TestQuestionWithoutDurableWaitsEndsWhenTheAppCloses(t *testing.T) {
	model := newFakeModel(t, provider.OpenAIChatKind, call("c-ask", "ask_human", askArgs), text("fresh start"))
	h := openHub(t, liveConfig(t, model))
	s := h.create("ephemeral ask", "")
	enableQuestions(t, h, s.ID, false)
	h.send(s.ID, "ask me")
	h.pending(s.ID)
	h.reopen()
	status := h.detail(s.ID).Session.Status
	if status != engine.Cancelled && status != engine.Interrupted {
		t.Fatalf("question Turn after close/reopen: %s: %s", status, h.dump(s.ID))
	}
	if items, err := h.service().PendingInteractions(context.Background(), s.ID); err != nil || len(items) != 0 {
		t.Fatalf("question survived without durable waits: %+v %v", items, err)
	}
	if b := toolBlock(t, h.blocks(s.ID), "c-ask"); b.Status == product.BlockStatusRunning || b.Status == product.BlockStatusWaiting {
		t.Fatalf("question block after reopen %+v", b)
	}
	h.send(s.ID, "let us start over")
	h.expect(s.ID, engine.Succeeded)
	t.Logf("closing the app with an open question ended the Turn as %s", status)
}
