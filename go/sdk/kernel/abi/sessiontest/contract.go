// INPUT: context, encoding/json, testing, kernel/abi, llm/wire
// OUTPUT: Run shared ConversationLog/PersistentSession backend conformance tests
// POS: [SDK contract testing] Third-party backends reuse execution-fact guarantees without a platform dependency.
package sessiontest

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

// Run requires a fresh, empty backend for each subtest. The factory owns cleanup.
// It tests shared record semantics, not persistence, backend-specific redaction,
// close APIs or byte budgets, which must be documented and tested separately.
func Run(t *testing.T, open func(*testing.T) abi.PersistentSession) {
	t.Helper()
	for name, test := range map[string]func(*testing.T, abi.PersistentSession){
		"atomic_validation":        atomicValidation,
		"ownership_and_projection": ownership,
		"causal_identity":          causalIdentity,
		"matching_outcomes":        matchingOutcomes,
		"extensions_and_cursor":    extensions,
		"cancellation":             cancellation,
	} {
		t.Run(name, func(t *testing.T) { test(t, open(t)) })
	}
}

func conversation(text string) abi.SessionRecord {
	message := wire.TextMessage(wire.RoleUser, text)
	return abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: &message})
}

func read(t *testing.T, log abi.PersistentSession) []abi.SessionRecord {
	t.Helper()
	records, err := log.Records(t.Context(), 0)
	if err != nil {
		t.Fatal(err)
	}
	return records
}

func atomicValidation(t *testing.T, log abi.PersistentSession) {
	bad := conversation("invalid")
	bad.Message.Content[0].Kind = "invalid"
	if _, err := log.AppendRecords(t.Context(), []abi.SessionRecord{conversation("rollback"), bad}); err == nil {
		t.Fatal("invalid batch accepted")
	}
	if len(read(t, log)) != 0 {
		t.Fatal("failed append changed records")
	}
	messages, err := log.Messages(t.Context())
	if err != nil || len(messages) != 0 {
		t.Fatal("failed append changed messages", err)
	}
	if _, err := log.AppendRecords(t.Context(), []abi.SessionRecord{conversation("valid after rejection")}); err != nil {
		t.Fatal(err)
	}
}

func ownership(t *testing.T, log abi.PersistentSession) {
	input := conversation("original")
	committed, err := log.AppendRecords(t.Context(), []abi.SessionRecord{input, conversation("second")})
	if err != nil {
		t.Fatal(err)
	}
	if len(committed) != 2 || committed[0].Sequence != 1 || committed[1].Sequence != 2 || committed[0].ID == "" || committed[0].Time.IsZero() {
		t.Fatal("invalid store identity")
	}
	if committed[0].Message.ID != committed[0].ID || committed[1].Parent == nil || committed[1].Parent.RecordID != committed[0].ID || committed[1].Parent.SessionID != log.ID() {
		t.Fatal("invalid conversation or causal identity")
	}
	input.Message.Content[0].Text = "input mutation"
	committed[0].Message.Content[0].Text = "receipt mutation"
	stored := read(t, log)
	if stored[0].Message.Text() != "original" {
		t.Fatal("append aliases caller memory")
	}
	stored[0].Message.Content[0].Text = "read mutation"
	messages, err := log.Messages(t.Context())
	if err != nil || len(messages) != 2 || messages[0].Text() != "original" {
		t.Fatal("projection changed", err)
	}
	messages[0].Content[0].Text = "projection mutation"
	if read(t, log)[0].Message.Text() != "original" {
		t.Fatal("read aliases stored memory")
	}
	if err := log.Flush(t.Context()); err != nil {
		t.Fatal(err)
	}
}

func causalIdentity(t *testing.T, log abi.PersistentSession) {
	committed, err := log.AppendRecords(t.Context(), []abi.SessionRecord{conversation("root")})
	if err != nil {
		t.Fatal(err)
	}
	for _, kind := range []string{"foreign", "future", "forged", "owned"} {
		record := conversation("rejected")
		record.Parent = &abi.RecordReference{SessionID: log.ID(), RecordID: committed[0].ID, Sequence: 1}
		switch kind {
		case "foreign":
			record.Parent.SessionID += "-foreign"
		case "future":
			record.Parent.Sequence = 2
		case "forged":
			record.Parent.RecordID += "-forged"
		case "owned":
			record.ID = "caller-owned"
		}
		if _, err := log.AppendRecords(t.Context(), []abi.SessionRecord{conversation("rollback"), record}); err == nil {
			t.Fatalf("accepted %s identity", kind)
		}
		if len(read(t, log)) != 1 {
			t.Fatal("identity rejection changed records")
		}
	}
}

func matchingOutcomes(t *testing.T, log abi.PersistentSession) {
	intent := abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordToolIntent, ToolIntent: &abi.ToolIntent{RunID: "run", Call: wire.ToolCall{ID: "call", Name: "tool", Arguments: json.RawMessage(`{}`)}}})
	outcome := abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordToolOutcome, ToolOutcome: &abi.ToolOutcome{RunID: "run", CallID: "call", Status: abi.OutcomeSucceeded, Result: wire.ToolResult{CallID: "call"}}})
	if _, err := log.AppendRecords(t.Context(), []abi.SessionRecord{outcome}); err == nil {
		t.Fatal("orphan outcome accepted")
	}
	if _, err := log.AppendRecords(t.Context(), []abi.SessionRecord{intent, outcome}); err != nil {
		t.Fatal(err)
	}
	for _, duplicate := range []abi.SessionRecord{intent, outcome} {
		if _, err := log.AppendRecords(t.Context(), []abi.SessionRecord{duplicate}); err == nil {
			t.Fatal("duplicate protocol fact accepted")
		}
	}
	if len(read(t, log)) != 2 {
		t.Fatal("protocol rejection changed records")
	}
}

func extensions(t *testing.T, log abi.PersistentSession) {
	record := abi.SessionRecord{Kind: abi.RecordExtension, Namespace: "example.contract", Type: "future", SchemaVersion: 37, Payload: json.RawMessage(`{"future":"unchanged🙂"}`)}
	committed, err := log.AppendRecords(t.Context(), []abi.SessionRecord{record, conversation("visible")})
	if err != nil {
		t.Fatal(err)
	}
	record.Payload[0] = '['
	committed[0].Payload[0] = '['
	stored := read(t, log)
	if string(stored[0].Payload) != `{"future":"unchanged🙂"}` || stored[0].SchemaVersion != 37 {
		t.Fatal("unknown extension altered")
	}
	messages, err := log.Messages(t.Context())
	if err != nil || len(messages) != 1 || messages[0].Text() != "visible" {
		t.Fatal("extension leaked into transcript", err)
	}
	tail, err := log.Records(t.Context(), stored[0].Sequence)
	if err != nil || len(tail) != 1 || tail[0].ID != stored[1].ID {
		t.Fatal("cursor is not exclusive record sequence", err)
	}
}

func cancellation(t *testing.T, log abi.PersistentSession) {
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	if _, err := log.AppendRecords(ctx, []abi.SessionRecord{conversation("cancelled")}); err == nil {
		t.Fatal("cancelled append accepted")
	}
	if len(read(t, log)) != 0 {
		t.Fatal("cancelled append changed state")
	}
	if _, err := log.Messages(ctx); err == nil {
		t.Fatal("cancelled read accepted")
	}
	if err := log.Flush(ctx); err == nil {
		t.Fatal("cancelled flush accepted")
	}
}
