// INPUT: hubFixture real Alva assembly, fakeModel write/shell calls, the unified interaction read/answer and the three permission policies
// OUTPUT: Scenarios for Ask/Auto/Full on writes and shell, once-only consent, denial, duplicate and late clicks
// POS: [scenario] Approval semantics seen from the user's clicks and the model's next request.
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

func TestShellAndWritesFollowAskAutoAndFullPolicies(t *testing.T) {
	for _, policy := range []hub.PermissionPolicy{hub.PermissionAsk, hub.PermissionAuto, hub.PermissionFull} {
		t.Run(string(policy), func(t *testing.T) {
			workspace := t.TempDir()
			model := newFakeModel(t, provider.OpenAIChatKind,
				call("c-write", "write_file", `{"path":"w.txt","content":"written"}`),
				call("c-shell", "execute_shell", `{"command":"printf ran > s.txt"}`),
				text("finished"),
			)
			h := openHub(t, liveConfig(t, model))
			s := h.create("policy", workspace)
			if err := h.service().SetPermissionPolicy(context.Background(), s.ID, policy, policy == hub.PermissionFull); err != nil {
				t.Fatal(err)
			}
			enableShell(t, h, s.ID)
			h.send(s.ID, "write then run")
			// The write: Ask asks, Auto and Full do not.
			if policy == hub.PermissionAsk {
				item := h.pending(s.ID)
				if item.View.Kind != product.InteractionKindPermission || item.CallID != "c-write" {
					t.Fatalf("write approval %+v", item)
				}
				if _, err := os.Stat(filepath.Join(workspace, "w.txt")); !os.IsNotExist(err) {
					t.Fatal("Ask wrote before consent")
				}
				if err := h.service().RespondInteraction(context.Background(), s.ID, item, product.InteractionResponse{Allow: true}); err != nil {
					t.Fatal(err)
				}
			}
			// The shell: Ask and Auto ask, only Full runs unasked.
			if policy != hub.PermissionFull {
				item := h.pending(s.ID)
				if item.CallID != "c-shell" || item.View.Kind != product.InteractionKindPermission {
					t.Fatalf("shell approval %+v", item)
				}
				if _, err := os.Stat(filepath.Join(workspace, "s.txt")); !os.IsNotExist(err) {
					t.Fatal("shell ran before consent")
				}
				if err := h.service().RespondInteraction(context.Background(), s.ID, item, product.InteractionResponse{Allow: true}); err != nil {
					t.Fatal(err)
				}
			}
			h.expect(s.ID, engine.Succeeded)
			for name, want := range map[string]string{"w.txt": "written", "s.txt": "ran"} {
				if data, err := os.ReadFile(filepath.Join(workspace, name)); err != nil || string(data) != want {
					t.Fatalf("%s = %q %v", name, data, err)
				}
			}
		})
	}
}

// Consent is for one call: a second write in the same Turn asks again, and a
// click on an already answered or finished request fails instead of acting.
func TestApprovalIsOnceOnlyAndDuplicateOrLateClicksFail(t *testing.T) {
	workspace := t.TempDir()
	model := newFakeModel(t, provider.OpenAIChatKind,
		call("c-first", "write_file", `{"path":"first.txt","content":"one"}`),
		call("c-second", "write_file", `{"path":"second.txt","content":"two"}`),
		text("both written"),
	)
	h := openHub(t, liveConfig(t, model))
	s := h.create("once", workspace)
	h.send(s.ID, "write two files")
	first := h.pending(s.ID)
	if first.CallID != "c-first" {
		t.Fatalf("first approval %+v", first)
	}
	ctx := context.Background()
	if err := h.service().RespondInteraction(ctx, s.ID, first, product.InteractionResponse{Allow: true}); err != nil {
		t.Fatal(err)
	}
	second := h.pending(s.ID)
	if second.CallID != "c-second" {
		t.Fatalf("consent for the first call spread to the second: %+v", second)
	}
	if _, err := os.Stat(filepath.Join(workspace, "second.txt")); !os.IsNotExist(err) {
		t.Fatal("second write ran on the first consent")
	}
	if err := h.service().RespondInteraction(ctx, s.ID, first, product.InteractionResponse{Allow: true}); err == nil {
		t.Fatal("a duplicate click on the answered first request succeeded")
	}
	if err := h.service().RespondInteraction(ctx, s.ID, second, product.InteractionResponse{Allow: true}); err != nil {
		t.Fatal(err)
	}
	h.expect(s.ID, engine.Succeeded)
	if err := h.service().RespondInteraction(ctx, s.ID, second, product.InteractionResponse{Allow: false}); err == nil {
		t.Fatal("a late click after the Turn ended was accepted")
	}
	for name, want := range map[string]string{"first.txt": "one", "second.txt": "two"} {
		if data, err := os.ReadFile(filepath.Join(workspace, name)); err != nil || string(data) != want {
			t.Fatalf("%s = %q %v", name, data, err)
		}
	}
	// Each answered approval stays in the Message Stream as one settled line
	// (approval_lines_test.go checks its content).
	if cards := blocksOf(h.blocks(s.ID), product.BlockKindInteraction); len(cards) != 2 {
		t.Fatalf("answered Alva approvals left %d history cards, want 2", len(cards))
	}
}

// Denying a write leaves the file untouched and the model is told: the denial
// arrives as a tool error result and the Turn continues. The decline is one-time:
// when the model asks for the same write again the user is asked again.
func TestDeniedWriteReachesTheModelAndAnAskedAgainWriteIsAskedAgain(t *testing.T) {
	workspace := t.TempDir()
	args := `{"path":"denied.txt","content":"now allowed"}`
	model := newFakeModel(t, provider.OpenAIChatKind,
		call("c-deny", "write_file", args),
		call("c-retry", "write_file", args),
		text("done"),
	)
	h := openHub(t, liveConfig(t, model))
	s := h.create("deny", workspace)
	h.send(s.ID, "write a file")
	first := h.pending(s.ID)
	if err := h.service().RespondInteraction(context.Background(), s.ID, first, product.InteractionResponse{Allow: false}); err != nil {
		t.Fatal(err)
	}
	// The decline reaches the model as an error result carrying guidance.
	second := h.pending(s.ID)
	if second.CallID != "c-retry" {
		t.Fatalf("expected a fresh approval for the retried call, got %+v", second)
	}
	if r := model.request(2).toolResults()["c-deny"]; !strings.Contains(r, "The user declined this tool call") || !strings.Contains(r, "Do not retry") {
		t.Fatalf("the decline did not reach the model: %s", r)
	}
	if _, err := os.Stat(filepath.Join(workspace, "denied.txt")); !os.IsNotExist(err) {
		t.Fatal("denied write took effect", err)
	}
	if err := h.service().RespondInteraction(context.Background(), s.ID, second, product.InteractionResponse{Allow: true}); err != nil {
		t.Fatal(err)
	}
	h.expect(s.ID, engine.Succeeded)
	if data, err := os.ReadFile(filepath.Join(workspace, "denied.txt")); err != nil || string(data) != "now allowed" {
		t.Fatalf("approved retry %q %v", data, err)
	}
	if model.calls() != 3 {
		t.Fatalf("model requests %d, want 3", model.calls())
	}
	blocks := h.blocks(s.ID)
	if b := toolBlock(t, blocks, "c-deny"); b.Status != product.BlockStatusFailed || !strings.Contains(b.Text, "declined") {
		t.Fatalf("declined call block %+v", b)
	}
	if errs := errorEvents(h.history(s.ID)); len(errs) != 0 {
		t.Fatalf("a decline must not fail the Turn: %+v", errs)
	}
}

// A path outside the workspace is refused without asking anyone; the model is
// told why and the Turn continues.
func TestOutsideWorkspaceWriteIsRefusedToTheModelWithoutACard(t *testing.T) {
	workspace := t.TempDir()
	model := newFakeModel(t, provider.OpenAIChatKind,
		call("c-out", "write_file", `{"path":"../escape.txt","content":"x"}`),
		text("understood"),
	)
	h := openHub(t, liveConfig(t, model))
	s := h.create("outside", workspace)
	h.send(s.ID, "write outside")
	h.expect(s.ID, engine.Succeeded)
	if r := model.request(2).toolResults()["c-out"]; !strings.Contains(r, "outside the workspace") {
		t.Fatalf("the refusal did not reach the model: %s", r)
	}
	if _, err := os.Stat(filepath.Join(filepath.Dir(workspace), "escape.txt")); !os.IsNotExist(err) {
		t.Fatal("outside write took effect", err)
	}
}

// Stop while a card waits: the Turn ends cancelled, nothing is written, the
// card disappears and a click on it afterwards fails.
func TestStopWhileAnApprovalWaitsCancelsAndExpiresTheCard(t *testing.T) {
	workspace := t.TempDir()
	model := newFakeModel(t, provider.OpenAIChatKind, call("c-wait", "write_file", `{"path":"never.txt","content":"x"}`), text("after stop"))
	h := openHub(t, liveConfig(t, model))
	s := h.create("stop approval", workspace)
	h.send(s.ID, "write something")
	item := h.pending(s.ID)
	execution, active := h.app.Runtime.ActiveExecution(s.ID)
	if !active {
		t.Fatal("no active execution while the approval waits")
	}
	if err := h.app.Runtime.CancelExecution(context.Background(), s.ID, execution); err != nil {
		t.Fatal(err)
	}
	h.expect(s.ID, engine.Cancelled)
	if items, err := h.service().PendingInteractions(context.Background(), s.ID); err != nil || len(items) != 0 {
		t.Fatalf("pending after Stop %+v %v", items, err)
	}
	if err := h.service().RespondInteraction(context.Background(), s.ID, item, product.InteractionResponse{Allow: true}); err == nil {
		t.Fatal("a click after Stop was accepted")
	}
	if _, err := os.Stat(filepath.Join(workspace, "never.txt")); !os.IsNotExist(err) {
		t.Fatal("write happened after Stop", err)
	}
	if b := toolBlock(t, h.blocks(s.ID), "c-wait"); b.Status == product.BlockStatusRunning || b.Status == product.BlockStatusCompleted {
		t.Fatalf("stopped call block %+v", b)
	}
	h.send(s.ID, "carry on")
	h.expect(s.ID, engine.Succeeded)
}

// A tool name the model invents cannot execute anything. The model gets the
// kernel's unknown-tool feedback and continues; the user is never asked to
// approve it (a denial would otherwise abort the whole Turn).
func TestUnknownToolCallGetsFeedbackWithoutAnApprovalCard(t *testing.T) {
	for _, durable := range []bool{false, true} {
		name := "broker"
		if durable {
			name = "durable-wait"
		}
		t.Run(name, func(t *testing.T) {
			model := newFakeModel(t, provider.OpenAIChatKind, call("c-ghost", "ghost_tool", `{"anything":1}`), text("sorry, using real tools"))
			h := openHub(t, liveConfig(t, model))
			s := h.create("ghost", "")
			if durable {
				if err := h.service().EnableDurableWaits(context.Background(), s.ID, true); err != nil {
					t.Fatal(err)
				}
			}
			h.send(s.ID, "call a tool that does not exist")
			waitFor(t, "the unknown tool call to settle", func() bool {
				items, err := h.service().PendingInteractions(context.Background(), s.ID)
				if err != nil {
					t.Fatal(err)
				}
				if len(items) > 0 {
					t.Fatalf("the user was asked to approve a tool that does not exist: %+v", items[0].View)
				}
				return h.detail(s.ID).Session.Status != hub.Running
			})
			outcome := h.settle(s.ID)
			if outcome != engine.Succeeded || model.calls() != 2 {
				t.Fatalf("unknown tool: turn %s, model requests %d: %s", outcome, model.calls(), h.dump(s.ID))
			}
			if r := model.request(2).toolResults()["c-ghost"]; !strings.Contains(r, `unknown tool`) || !strings.Contains(r, "ghost_tool") {
				t.Fatalf("unknown-tool feedback %s", r)
			}
			if b := toolBlock(t, h.blocks(s.ID), "c-ghost"); b.Status != product.BlockStatusFailed {
				t.Fatalf("unknown tool block %+v", b)
			}
		})
	}
}
