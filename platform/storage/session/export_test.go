// INPUT: Actual native facts/artifacts, committed child spawn links and fixed high-water export requests
// OUTPUT: Exact recursive scope, no raw-artifact fetch, precision/redaction and fail-closed whole-bundle regressions
// POS: [platform / durable storage] Export fixtures never create an Agent or execute the recorded tools.
package session

import (
	"bytes"
	"encoding/json"
	"errors"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestRecursiveBundleUsesExactTypedLinksAndNeverEmbedsReferencedOriginals(t *testing.T) {
	repo, root := repoSession(t)
	rawOriginal := "raw-upload-not-requested-by-export"
	artifact, err := repo.PutArtifact(t.Context(), "owner", root.ID(), ArtifactInput{Kind: "upload", Name: "original.txt", MediaType: "text/plain", Processing: "Original fixture bytes, not requested for export", Data: []byte(rawOriginal), Text: true})
	if err != nil {
		t.Fatal(err)
	}
	reference := artifact.Reference()
	message := wire.TextMessage(wire.RoleUser, "reference only")
	message.Content = append(message.Content, wire.ContentBlock{Kind: wire.ContentReference, Reference: &reference})
	intent := abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordToolIntent, ToolIntent: &abi.ToolIntent{RunID: "root-run", Call: wire.ToolCall{ID: "spawn", Name: "agent", Arguments: json.RawMessage(`{}`)}}})
	if _, err := root.AppendRecords(t.Context(), []abi.SessionRecord{abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: &message}), intent}); err != nil {
		t.Fatal(err)
	}
	child, err := repo.CreateChild(t.Context(), "owner", root.ID(), "real-child", "root-run", "spawn", "scope")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := child.Session.AppendRecords(t.Context(), []abi.SessionRecord{{Kind: abi.RecordExtension, Namespace: "fixture.audit", Type: "opaque", SchemaVersion: 2, Payload: json.RawMessage(`{"amount":12345678901234567890.12345678901234567890,"api_key":"private-field-fixture"}`)}}); err != nil {
		t.Fatal(err)
	}
	if _, err := repo.Create(t.Context(), "owner", "metadata-only-child", root.ID()); err != nil {
		t.Fatal(err)
	}
	page, err := repo.Page(t.Context(), "owner", root.ID(), 0, 1, 0)
	if err != nil {
		t.Fatal(err)
	}
	high := page.HighWater
	if _, err := root.AppendRecords(t.Context(), []abi.SessionRecord{conversation("after the selected root snapshot")}); err != nil {
		t.Fatal(err)
	}
	before, err := child.Session.Records(t.Context(), 0)
	if err != nil {
		t.Fatal(err)
	}
	original, _ := json.Marshal(before)
	data, summary, err := repo.ExportBundle(t.Context(), "owner", root.ID(), BundleOptions{HighWater: high, IncludeChildren: true})
	if err != nil {
		t.Fatal(err)
	}
	if summary.Sessions != 2 || summary.Records != 4 || summary.HighWaters[root.ID()] != high || summary.Bytes != len(data) || len(summary.SHA256) != 64 || len(repo.active) != 0 {
		t.Fatal("scope/count or no-runtime guarantee lost", summary)
	}
	for _, forbidden := range []string{rawOriginal, "private-field-fixture", "metadata-only-child", "after the selected root snapshot"} {
		if bytes.Contains(data, []byte(forbidden)) {
			t.Fatal("unrequested scope/private body exported")
		}
	}
	if !bytes.Contains(data, []byte("12345678901234567890.12345678901234567890")) || !bytes.Contains(data, []byte(`"type":"complete"`)) {
		t.Fatal("opaque payload precision or completion footer lost")
	}
	for _, line := range bytes.Split(bytes.TrimSpace(data), []byte{'\n'}) {
		if !json.Valid(line) {
			t.Fatal("invalid JSONL")
		}
	}
	after, _ := child.Session.Records(t.Context(), 0)
	remaining, _ := json.Marshal(after)
	if !bytes.Equal(original, remaining) {
		t.Fatal("export rewrote original facts")
	}
	_, raw, err := repo.ReadArtifactFull(t.Context(), "owner", root.ID(), reference.URI, 1024)
	if err != nil || string(raw) != rawOriginal {
		t.Fatal("original artifact changed", err)
	}
	if data, _, err := repo.ExportBundle(t.Context(), "foreign", root.ID(), BundleOptions{}); !errors.Is(err, ErrNotFound) || len(data) != 0 {
		t.Fatal("foreign owner received partial bundle", err)
	}
}

func TestBundleRejectsUnindexedChildClaimsAndOversizedRecordsWithoutPartialSuccess(t *testing.T) {
	repo, root := repoSession(t)
	if _, err := root.AppendRecords(t.Context(), []abi.SessionRecord{conversation(strings.Repeat("x", 1<<20))}); err != nil {
		t.Fatal(err)
	}
	if data, summary, err := repo.ExportBundle(t.Context(), "owner", root.ID(), BundleOptions{}); err == nil || len(data) != 0 || summary.Records != 0 {
		t.Fatal("oversized record masqueraded as full export")
	}
	other, err := repo.Create(t.Context(), "owner", "fake-root", "")
	if err != nil {
		t.Fatal(err)
	}
	intent := abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordToolIntent, ToolIntent: &abi.ToolIntent{RunID: "run", Call: wire.ToolCall{ID: "call", Name: "agent", Arguments: json.RawMessage(`{}`)}}})
	facts, err := other.AppendRecords(t.Context(), []abi.SessionRecord{intent})
	if err != nil {
		t.Fatal(err)
	}
	if _, err := repo.Create(t.Context(), "owner", "fake-child", other.ID()); err != nil {
		t.Fatal(err)
	}
	payload, _ := json.Marshal(map[string]string{"parent_session_id": other.ID(), "child_session_id": "fake-child", "parent_run_id": "run", "spawn_tool_call_id": "call", "child_scope_id": "scope"})
	if _, err := other.AppendRecords(t.Context(), []abi.SessionRecord{{Kind: abi.RecordExtension, Namespace: "alva.agent", Type: "child_session_linked", SchemaVersion: 1, Parent: &abi.RecordReference{SessionID: other.ID(), RecordID: facts[0].ID, Sequence: facts[0].Sequence}, Payload: payload}}); err != nil {
		t.Fatal(err)
	}
	if data, _, err := repo.ExportBundle(t.Context(), "owner", other.ID(), BundleOptions{IncludeChildren: true}); err == nil || len(data) != 0 {
		t.Fatal("unindexed claimed ancestry became an export grant")
	}
}
