// INPUT: hubFixture real Alva assembly, fakeModel write/shell calls and the unified interaction read/answer
// OUTPUT: Scenarios for the settled approval line and the waiting card's tool display
// POS: [scenario] What the conversation keeps after the user decides an Alva approval.
package scenarios

import (
	"context"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/llm/provider"
)

// The waiting card names the call by its ToolDisplay, and after the decision a
// one-line "allowed/denied" record stays in the conversation, like for an
// external engine.
func TestAlvaApprovalLeavesAllowedAndDeniedLines(t *testing.T) {
	workspace := t.TempDir()
	model := newFakeModel(t, provider.OpenAIChatKind,
		call("c-edit", "write_file", `{"path":"a.go","content":"package a"}`),
		call("c-shell", "execute_shell", `{"command":"printf hi"}`),
		text("done"),
	)
	h := openHub(t, liveConfig(t, model))
	s := h.create("lines", workspace)
	enableShell(t, h, s.ID)
	h.send(s.ID, "write then run")
	ctx := context.Background()

	first := h.pending(s.ID)
	bound, _ := product.BindInteractions(h.blocks(s.ID), []product.Interaction{first})
	var waiting *product.InteractionView
	for _, b := range bound {
		if b.Kind == product.BlockKindInteraction && b.Interaction != nil && b.Interaction.State == product.InteractionWaiting {
			waiting = b.Interaction
		}
	}
	if waiting == nil || waiting.Tool == nil || waiting.Tool.Kind != engine.ToolEdit || waiting.Tool.Path != "a.go" {
		t.Fatalf("the waiting card does not carry the call's ToolDisplay: %+v", waiting)
	}
	if err := h.service().RespondInteraction(ctx, s.ID, first, product.InteractionResponse{Allow: true}); err != nil {
		t.Fatal(err)
	}
	second := h.pending(s.ID)
	if err := h.service().RespondInteraction(ctx, s.ID, second, product.InteractionResponse{Allow: false}); err != nil {
		t.Fatal(err)
	}
	h.settle(s.ID)

	var lines []*product.InteractionView
	for _, b := range blocksOf(h.blocks(s.ID), product.BlockKindInteraction) {
		lines = append(lines, b.Interaction)
	}
	if len(lines) != 2 {
		t.Fatalf("want two settled lines, got %d: %s", len(lines), h.dump(s.ID))
	}
	allowed, denied := lines[0], lines[1]
	if allowed.State != product.InteractionAllowed || allowed.Tool == nil || allowed.Tool.Kind != engine.ToolEdit || allowed.Tool.Path != "a.go" {
		t.Fatalf("allowed line %+v", allowed)
	}
	if denied.State != product.InteractionDenied || denied.Tool == nil || denied.Tool.Kind != engine.ToolCommand || !strings.Contains(denied.Tool.Command, "printf hi") {
		t.Fatalf("denied line %+v", denied)
	}
	if allowed.Name != "" || denied.Name != "" {
		t.Fatal("a settled line must not carry the raw tool name")
	}
	// The line is part of stored history, so it survives a restart.
	h.reopen()
	if again := blocksOf(h.blocks(s.ID), product.BlockKindInteraction); len(again) != 2 || again[0].Interaction.State != product.InteractionAllowed {
		t.Fatalf("settled lines lost after restart: %+v", again)
	}
}
