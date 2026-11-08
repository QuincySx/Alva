// INPUT: hubFixture real Alva assembly with opt-in native SubAgents, Full policy and a fakeModel that answers root and child by content
// OUTPUT: Scenario where the model delegates to a child Agent and the child's result, tool block and trace link come back to the parent
// POS: [scenario] Complements bootstrap's root/child HITL test with result return and trace navigation.
package scenarios

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/llm/provider"
)

func TestDelegatedChildResultReturnsToTheParentAndTheTraceLinksIt(t *testing.T) {
	model := newFakeModel(t, provider.OpenAIChatKind)
	model.next = func(r modelRequest) reply {
		switch {
		case r.answersTool():
			return text("parent saw the child")
		case r.typed("CHILD-TASK: count the files"):
			return text("CHILD-RESULT-42")
		default:
			return call("c-spawn", "agent", `{"task":"CHILD-TASK: count the files","tools":["list_directory"],"timeout_ms":30000,"max_iterations":4}`)
		}
	}
	h := openHub(t, liveConfig(t, model))
	s := h.create("delegate", "")
	ctx := context.Background()
	if err := h.app.Store.SetSubagentsEnabled(ctx, s.ID, true); err != nil {
		t.Fatal(err)
	}
	if err := h.service().SetPermissionPolicy(ctx, s.ID, hub.PermissionFull, true); err != nil {
		t.Fatal(err)
	}
	h.send(s.ID, "delegate the counting")
	h.expect(s.ID, engine.Succeeded)
	if model.calls() != 3 {
		t.Fatalf("model requests %d, want root, child, root", model.calls())
	}
	// The parent model receives the child's outcome and a reference to its
	// selected final answer (references first), readable in the parent session.
	raw := model.request(3).toolResults()["c-spawn"]
	var content string
	var result struct {
		Outcome  string `json:"outcome"`
		Selected struct {
			URI string `json:"uri"`
		} `json:"selected_output"`
		Committed bool `json:"selected_output_committed"`
	}
	if json.Unmarshal([]byte(raw), &content) != nil || json.Unmarshal([]byte(content), &result) != nil || result.Outcome != "completed" || !result.Committed || result.Selected.URI == "" {
		t.Fatalf("child result returned to the parent model: %s", raw)
	}
	parentRefs, err := h.app.Repository.BoundReferences(ctx, s.ID, h.branch(s.ID).NativeSessionID)
	if err != nil {
		t.Fatal(err)
	}
	page, err := parentRefs.ReadText(ctx, result.Selected.URI, 0, 1024)
	if err != nil || page.Text != "CHILD-RESULT-42" {
		t.Fatalf("selected child answer %q %v", page.Text, err)
	}
	b := toolBlock(t, h.blocks(s.ID), "c-spawn")
	if b.Tool == nil || b.Tool.Kind != engine.ToolSubagent || b.Tool.ChildSessionID == "" || b.Tool.Jump != product.ToolJumpChild || b.Status != product.BlockStatusCompleted {
		t.Fatalf("sub-Agent block %+v / %+v", b, b.Tool)
	}
	var child *product.TraceStep
	for _, step := range h.trace(s.ID) {
		if step.Kind == product.TraceStepChild {
			step := step
			child = &step
		}
	}
	if child == nil || child.ChildLink == 0 {
		t.Fatal("trace has no enterable child step")
	}
	target, err := product.TraceTarget{SessionID: s.ID, BranchID: h.branch(s.ID).ID}.Child(*child)
	if err != nil {
		t.Fatal(err)
	}
	chunk, err := h.service().Traces.Tail(ctx, target)
	if err != nil {
		t.Fatal(err)
	}
	w := product.NewTraceWindow(0)
	w.Reset(chunk)
	if w.NativeID() != b.Tool.ChildSessionID || len(w.Steps()) == 0 {
		t.Fatalf("child trace %q with %d steps, want child %q", w.NativeID(), len(w.Steps()), b.Tool.ChildSessionID)
	}
	// The child's own messages never flatten into the parent Message Stream.
	for _, block := range h.blocks(s.ID) {
		if block.Kind == product.BlockKindReply && contains(block.Text, "CHILD-RESULT-42") {
			t.Fatal("child reply flattened into the parent stream")
		}
	}
}
