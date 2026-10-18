// INPUT: context, encoding/json, testing, kernel/abi, llm/wire
// OUTPUT: Atomic append, causal identity, typed outcome and extension preservation tests
// POS: [L2.5 SDK assembly] In-memory reference backend contract regressions independent of host persistence.
package core_test

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func conversation(text string) abi.SessionRecord {
	message := wire.TextMessage(wire.RoleUser, text)
	return abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: &message})
}

func TestMemorySessionAppendIsAllOrNone(t *testing.T) {
	session := core.NewMemorySession("atomic")
	bad := conversation("bad")
	bad.Message.Content[0].Kind = "unknown"
	if _, err := session.AppendRecords(context.Background(), []abi.SessionRecord{conversation("first"), bad}); err == nil {
		t.Fatal("invalid batch accepted")
	}
	if len(records(t, session)) != 0 {
		t.Fatal("partial records committed")
	}
	messages, _ := session.Messages(context.Background())
	if len(messages) != 0 {
		t.Fatal("partial conversation committed")
	}
	committed, err := session.AppendRecords(context.Background(), []abi.SessionRecord{conversation("one"), conversation("two")})
	if err != nil {
		t.Fatal(err)
	}
	if committed[0].Sequence != 1 || committed[1].Sequence != 2 || committed[1].Parent.RecordID != committed[0].ID || committed[0].Time.IsZero() {
		t.Fatalf("bad identities: %#v", committed)
	}
}

func TestMemorySessionCopiesEveryOwnershipBoundary(t *testing.T) {
	session := core.NewMemorySession("copies")
	input := conversation("original")
	committed, err := session.AppendRecords(context.Background(), []abi.SessionRecord{input})
	if err != nil {
		t.Fatal(err)
	}
	input.Message.Content[0].Text = "input mutation"
	committed[0].Message.Content[0].Text = "return mutation"
	stored := records(t, session)
	if stored[0].Message.Text() != "original" {
		t.Fatal("append ownership leaked")
	}
	stored[0].Message.Content[0].Text = "records mutation"
	messages, _ := session.Messages(context.Background())
	messages[0].Content[0].Text = "messages mutation"
	if records(t, session)[0].Message.Text() != "original" {
		t.Fatal("query ownership leaked")
	}
}

func TestMemorySessionRejectsForgedCausalParentAndStoreIdentity(t *testing.T) {
	session := core.NewMemorySession("causal")
	committed, err := session.AppendRecords(context.Background(), []abi.SessionRecord{conversation("root")})
	if err != nil {
		t.Fatal(err)
	}
	for _, fixture := range []string{"foreign", "future", "wrong-id", "owned-id"} {
		t.Run(fixture, func(t *testing.T) {
			record := conversation("bad")
			record.Parent = &abi.RecordReference{SessionID: "causal", RecordID: committed[0].ID, Sequence: 1}
			switch fixture {
			case "foreign":
				record.Parent.SessionID = "other"
			case "future":
				record.Parent.Sequence = 2
			case "wrong-id":
				record.Parent.RecordID = "forged"
			case "owned-id":
				record.ID = "forged"
			}
			if _, err := session.AppendRecords(context.Background(), []abi.SessionRecord{record}); err == nil {
				t.Fatal("forged metadata accepted")
			}
			if len(records(t, session)) != 1 {
				t.Fatal("failed batch changed state")
			}
		})
	}
}

func TestMemorySessionCancelledAppendHasNoEffect(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	session := core.NewMemorySession("cancelled")
	if _, err := session.AppendRecords(ctx, []abi.SessionRecord{conversation("x")}); err == nil {
		t.Fatal("cancelled append accepted")
	}
	if len(records(t, session)) != 0 {
		t.Fatal("cancelled append committed")
	}
}

func TestMemorySessionOutcomesRequireUniqueMatchingIntents(t *testing.T) {
	session := core.NewMemorySession("matching")
	intent := abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordToolIntent, ToolIntent: &abi.ToolIntent{RunID: "run", Call: wire.ToolCall{ID: "call", Name: "tool", Arguments: json.RawMessage(`{}`)}}})
	outcome := abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordToolOutcome, ToolOutcome: &abi.ToolOutcome{RunID: "run", CallID: "call", Status: abi.OutcomeSucceeded, Result: wire.ToolResult{CallID: "call"}}})
	if _, err := session.AppendRecords(context.Background(), []abi.SessionRecord{outcome}); err == nil {
		t.Fatal("orphan outcome accepted")
	}
	if _, err := session.AppendRecords(context.Background(), []abi.SessionRecord{intent, outcome}); err != nil {
		t.Fatal(err)
	}
	for _, duplicate := range []abi.SessionRecord{intent, outcome} {
		if _, err := session.AppendRecords(context.Background(), []abi.SessionRecord{duplicate}); err == nil {
			t.Fatal("duplicate protocol fact accepted")
		}
	}
	if len(records(t, session)) != 2 {
		t.Fatal("failed duplicate committed")
	}
}

func TestMemorySessionPreservesUnknownVersionedExtensionPayload(t *testing.T) {
	session := core.NewMemorySession("extensions")
	record := abi.SessionRecord{Kind: abi.RecordExtension, Namespace: "example.mcp", Type: "io.fragment", SchemaVersion: 37, Payload: json.RawMessage(`{"future":{"raw":"unchanged"}}`)}
	committed, err := session.AppendRecords(context.Background(), []abi.SessionRecord{record})
	if err != nil {
		t.Fatal(err)
	}
	record.Payload[0] = '['
	committed[0].Payload[0] = '['
	stored := records(t, session)
	if string(stored[0].Payload) != `{"future":{"raw":"unchanged"}}` || stored[0].SchemaVersion != 37 || stored[0].Namespace != "example.mcp" {
		t.Fatalf("extension changed: %#v", stored[0])
	}
	messages, _ := session.Messages(context.Background())
	if len(messages) != 0 {
		t.Fatal("extension entered model transcript")
	}
}
