// INPUT: encoding/json, strings, testing and the shared redaction policy
// OUTPUT: Field-level Redactor parity with RedactRecord and marker/key/literal cases
// POS: [platform / durable storage] Keeps event-field redaction identical to durable record redaction
package redaction

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestRedactorMatchesRecordRedaction(t *testing.T) {
	values := []string{
		`curl -H "Authorization: Bearer fake-token-123" https://example.test`,
		`{"matches":[{"path":".env","text":"API_KEY=fake-key-456"},{"path":"a.go","text":"ordinary"}]}`,
		"plain ordinary text",
		`{"api_key":"fake-key-789","name":"kept"}`,
	}
	r, err := NewRedactor()
	if err != nil {
		t.Fatal(err)
	}
	for _, value := range values {
		record := abi.SessionRecord{Kind: abi.RecordToolOutcome, ToolOutcome: &abi.ToolOutcome{RunID: "r", CallID: "c", Status: abi.OutcomeSucceeded, Result: wire.ToolResult{CallID: "c", Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: value}}}}}
		if err := RedactRecord(&record); err != nil {
			t.Fatal(err)
		}
		if got, want := r.Text(value), record.ToolOutcome.Result.Content[0].Text; got != want {
			t.Fatalf("field redaction %q differs from record redaction %q", got, want)
		}
	}
	if strings.Contains(r.Text(values[0]), "fake-token-123") || strings.Contains(r.Text(values[1]), "fake-key-456") || !strings.Contains(r.Text(values[1]), "ordinary") {
		t.Fatal("marker redaction lost or over-redacted", r.Text(values[0]), r.Text(values[1]))
	}
	arguments := json.RawMessage(`{"command":"echo ok","password":"fake-pass"}`)
	clean, err := r.JSON(arguments)
	if err != nil || strings.Contains(string(clean), "fake-pass") || !strings.Contains(string(clean), "echo ok") {
		t.Fatal("JSON key redaction", string(clean), err)
	}
	configured, err := NewRedactor("1", "fake-env-secret")
	if err != nil {
		t.Fatal(err)
	}
	if got := configured.Text("exit 1 with fake-env-secret"); strings.Contains(got, "fake-env-secret") || strings.Contains(got, "1") {
		t.Fatal("configured literals not replaced", got)
	}
	if got := configured.Text("1"); !strings.Contains(got, "[REDACTED]") {
		t.Fatal("numeric configured literal not replaced", got)
	}
	var none *Redactor
	if none.Text("Authorization: Bearer fake-token") != "[REDACTED]" {
		t.Fatal("nil redactor skipped the record markers")
	}
}
