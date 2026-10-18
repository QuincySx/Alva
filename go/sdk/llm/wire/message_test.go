// INPUT: encoding/json, testing
// OUTPUT: Typed wire validation and deep-copy ownership regressions
// POS: [L0 foundation] Prevents ambiguous content unions and reference auto-expansion.
package wire

import (
	"encoding/json"
	"testing"
)

func TestWireContentValidation(t *testing.T) {
	fixtures := []struct {
		name    string
		message Message
		valid   bool
	}{
		{"text", TextMessage(RoleUser, "hello"), true},
		{"reference", Message{Role: RoleUser, Content: []ContentBlock{{Kind: ContentReference, Reference: &Reference{URI: "session://file", Bytes: 20, Processing: "original bytes; not delivered"}}}}, true},
		{"unknown-role", Message{Role: "unknown"}, false},
		{"mixed", Message{Role: RoleAssistant, Content: []ContentBlock{{Kind: ContentText, ToolCall: &ToolCall{ID: "a", Name: "tool", Arguments: json.RawMessage(`{}`)}}}}, false},
		{"invalid-json", Message{Role: RoleAssistant, Content: []ContentBlock{{Kind: ContentToolCall, ToolCall: &ToolCall{ID: "a", Name: "tool", Arguments: json.RawMessage(`{`)}}}}, false},
		{"nested-protocol", Message{Role: RoleTool, Content: []ContentBlock{{Kind: ContentToolResult, ToolResult: &ToolResult{CallID: "a", Content: []ContentBlock{{Kind: ContentToolResult, ToolResult: &ToolResult{CallID: "b"}}}}}}}, false},
	}
	for _, fixture := range fixtures {
		t.Run(fixture.name, func(t *testing.T) {
			if err := fixture.message.Validate(); (err == nil) != fixture.valid {
				t.Fatalf("err=%v valid=%v", err, fixture.valid)
			}
		})
	}
}

func TestWireCloneOwnsNestedContentAndArguments(t *testing.T) {
	messages := []Message{{Role: RoleAssistant, Content: []ContentBlock{{Kind: ContentToolCall, ToolCall: &ToolCall{ID: "a", Name: "tool", Arguments: json.RawMessage(`{}`)}}}}, {Role: RoleTool, Content: []ContentBlock{{Kind: ContentToolResult, ToolResult: &ToolResult{CallID: "a", Content: []ContentBlock{{Kind: ContentReference, Reference: &Reference{URI: "original"}}}}}}}}
	copy := CloneMessages(messages)
	copy[0].Content[0].ToolCall.Arguments[0] = '['
	copy[1].Content[0].ToolResult.Content[0].Reference.URI = "changed"
	if string(messages[0].Content[0].ToolCall.Arguments) != "{}" || messages[1].Content[0].ToolResult.Content[0].Reference.URI != "original" {
		t.Fatal("wire clone shared nested storage")
	}
}
