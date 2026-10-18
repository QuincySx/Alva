// INPUT: context, encoding/json, strings, testing, kernel/abi, llm/wire
// OUTPUT: Exact working projection summary and critical protocol-group retention regressions
// POS: [L3 capability library] Prevents compression from summarizing unrelated store indices or orphaning protected tool evidence.
package agentcontext

import (
	"context"
	"encoding/json"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
	"strings"
	"testing"
)

func TestSummaryUsesExactDetachedWorkingRange(t *testing.T) {
	store := NewStore(1000, 1000, nil, SummarizerFunc(func(_ context.Context, entries []abi.ContextEntry, _ []string) (string, error) {
		if len(entries) != 1 || entries[0].Message.Text() != "projected" {
			t.Fatal("summarized wrong store entries", entries)
		}
		return "correct", nil
	}))
	unrelated := wire.TextMessage(wire.RoleUser, "unrelated store text")
	unrelated.ID = "stored"
	_ = store.InjectMessage("agent", abi.RuntimeInject, unrelated)
	projected := wire.TextMessage(wire.RoleUser, "projected")
	projected.ID = "projected"
	messages := []wire.Message{projected}
	err := (DefaultApply{Summarizer: SummarizerFunc(store.SummarizeEntries)}).ApplyCompressions(context.Background(), []abi.CompressAction{{Kind: abi.CompressSummary, Range: abi.MessageRange{From: abi.MessageSelector{Kind: abi.FromStart}, To: abi.MessageSelector{Kind: abi.ToEnd}}}}, &messages, store, "agent")
	if err != nil || len(messages) != 1 || !strings.Contains(messages[0].Text(), "correct") {
		t.Fatal(messages, err)
	}
}
func TestCriticalToolGroupSurvivesWindowAndPriorityCompression(t *testing.T) {
	call := wire.ToolCall{ID: "call", Name: "tool", Arguments: json.RawMessage(`{}`)}
	result := wire.ToolResult{CallID: "call", Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: "result"}}}
	messages := []wire.Message{{ID: "call-message", Role: wire.RoleAssistant, Content: []wire.ContentBlock{{Kind: wire.ContentToolCall, ToolCall: &call}}}, {ID: "result-message", Role: wire.RoleTool, Content: []wire.ContentBlock{{Kind: wire.ContentToolResult, ToolResult: &result}}}, wire.TextMessage(wire.RoleUser, "recent")}
	messages[2].ID = "recent"
	for _, action := range []abi.CompressAction{{Kind: abi.CompressWindow, KeepRecent: 1}, {Kind: abi.CompressPriority, Priority: abi.Low}} {
		store := NewStore(1000, 1000, nil, nil)
		for _, message := range messages {
			_ = store.InjectMessage("agent", abi.RuntimeInject, message)
			_ = store.TagPriority("agent", message.ID, abi.Low)
		}
		_ = store.TagPriority("agent", "call-message", abi.Critical)
		working := wire.CloneMessages(messages)
		if err := (DefaultApply{}).ApplyCompressions(context.Background(), []abi.CompressAction{action}, &working, store, "agent"); err != nil {
			t.Fatal(err)
		}
		if len(working) < 2 || working[0].Content[0].ToolCall == nil || working[1].Content[0].ToolResult == nil {
			t.Fatal("critical tool protocol orphaned", working)
		}
	}
}
