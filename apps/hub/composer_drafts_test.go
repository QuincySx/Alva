// INPUT: Scoped manual text, bounded selected-reference receipts and frozen preparation bytes.
// OUTPUT: Pure draft validation, copy isolation and logical-scope regressions.
// POS: [L7 Hub tests] Verifies durable draft policy without a UI, filesystem or execution runtime.
package hub

import (
	"encoding/json"
	"strings"
	"testing"
)

func TestComposerDraftScopeAndValueValidation(t *testing.T) {
	valid := ComposerDraft{Scope: ComposerDraftScope{Kind: "home", ID: "project:new-task"}, Text: strings.Repeat("<", 64<<10), References: []ComposerDraftReference{{Kind: "file", ID: "original", SHA256: strings.Repeat("a", 64)}}, PreparedCommands: json.RawMessage(`[]`)}
	if err := valid.Validate(); err != nil {
		t.Fatal(err)
	}
	for _, mutate := range []func(*ComposerDraft){
		func(d *ComposerDraft) { d.Scope.Kind = "window" },
		func(d *ComposerDraft) { d.Scope.ID = "" },
		func(d *ComposerDraft) { d.Text += "x" },
		func(d *ComposerDraft) { d.Text = "invalid\x00text" },
		func(d *ComposerDraft) { d.Text = string([]byte{0xff}) },
		func(d *ComposerDraft) { d.Revision = 1 << 63 },
		func(d *ComposerDraft) { d.References = append(d.References, d.References[0]) },
		func(d *ComposerDraft) { d.References[0].SHA256 = strings.Repeat("A", 64) },
		func(d *ComposerDraft) { d.PreparedCommands = json.RawMessage(`null`) },
		func(d *ComposerDraft) { d.PreparedCommands = json.RawMessage(`{}`) },
		func(d *ComposerDraft) { d.PreparedCommands = json.RawMessage(`[`) },
	} {
		bad := CloneComposerDraft(valid)
		mutate(&bad)
		if bad.Validate() == nil {
			t.Fatalf("invalid draft accepted: %+v", bad.Scope)
		}
	}
	copy := CloneComposerDraft(valid)
	copy.References[0].ID = "changed"
	copy.PreparedCommands[0] = ' '
	if valid.References[0].ID != "original" || string(valid.PreparedCommands) != `[]` {
		t.Fatal("caller mutation changed the selected source draft")
	}
}
