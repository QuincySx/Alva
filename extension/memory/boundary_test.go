// INPUT: Actual configured memory service/reference capabilities and direct standalone Tool calls.
// OUTPUT: Typed-nil, actual Tool and ambiguous argument rejection before memory writes or approvals.
// POS: [L4 extension] Memory service replacement preserves exact invocation and reference capability boundaries.
package memoryplugin

import (
	"context"
	"testing"

	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type absentReferences struct{ agentcontext.ReferenceBackend }

func TestMemoryRejectsNilReferencesAndBorrowedInvocation(t *testing.T) {
	f := newFixture(t, true)
	config := f.plugin.config
	config.References = (*absentReferences)(nil)
	if _, err := New(config); err == nil {
		t.Error("typed-nil memory reference backend accepted")
	}
	tool := f.plugin.Tools()[1]
	for _, mode := range []string{"no_actual", "duplicate", "invalid_unicode"} {
		t.Run(mode, func(t *testing.T) {
			raw := `{"key":"fixture","content":"unapproved"}`
			e := abi.ToolExecutionContext{SessionID: "actual", RunID: "run", CallID: "call", Workspace: f.cwd, ActualTool: tool}
			if mode == "no_actual" {
				e.ActualTool = nil
			}
			if mode == "duplicate" {
				raw = `{"key":"fixture","content":"one","content":"two"}`
			}
			if mode == "invalid_unicode" {
				raw = `{"key":"fixture","content":"\ud800"}`
			}
			if _, err := tool.Execute(context.Background(), e, wire.ToolCall{ID: "call", Name: tool.Name(), Arguments: []byte(raw)}); err == nil {
				t.Error("invalid memory invocation accepted")
			}
		})
	}
}
