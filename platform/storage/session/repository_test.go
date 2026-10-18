// INPUT: context, encoding/json, errors, strings, testing, kernel/abi, llm/wire.
// OUTPUT: Atomic, durable, bounded, owner-scoped session/artifact/recovery regressions.
// POS: [platform / durable storage] Real local SQLite acceptance without models, MCP, user data or replayed side effects.
package session

import (
	"context"
	"encoding/json"
	"errors"
	"path/filepath"
	"strings"
	"testing"
	"unicode/utf8"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func conversation(text string) abi.SessionRecord {
	message := wire.TextMessage(wire.RoleUser, text)
	return abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: &message})
}
func repoSession(t *testing.T) (*Repository, *Session) {
	t.Helper()
	r, err := Open(context.Background(), filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := r.Close(); err != nil {
			t.Error(err)
		}
	})
	s, err := r.Create(context.Background(), "owner", "native", "")
	if err != nil {
		t.Fatal(err)
	}
	return r, s
}

func TestAtomicAppendReopenAndUnknownExtensions(t *testing.T) {
	ctx := context.Background()
	dir := filepath.Join(t.TempDir(), "private")
	r, err := Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	s, err := r.Create(ctx, "owner", "native", "")
	if err != nil {
		t.Fatal(err)
	}
	bad := conversation("bad")
	bad.SchemaVersion = 2
	if _, err = s.AppendRecords(ctx, []abi.SessionRecord{conversation("must rollback"), bad}); err == nil {
		t.Fatal("invalid batch accepted")
	}
	page, err := r.Page(ctx, "owner", "native", 0, 100, 0)
	if err != nil || len(page.Entries) != 0 {
		t.Fatalf("partial append: %+v %v", page, err)
	}
	ext := abi.SessionRecord{Kind: abi.RecordExtension, Namespace: "example.future", Type: "opaque", SchemaVersion: 37, Payload: json.RawMessage(`{"future":"原样🙂"}`)}
	committed, err := s.AppendRecords(ctx, []abi.SessionRecord{conversation("hello"), ext})
	if err != nil {
		t.Fatal(err)
	}
	if committed[0].Message.ID != committed[0].ID || committed[1].Parent.RecordID != committed[0].ID {
		t.Fatal("message/causal identities missing")
	}
	if err = r.Close(); err != nil {
		t.Fatal(err)
	}
	r, err = Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	defer r.Close()
	s, err = r.OpenSession(ctx, "owner", "native")
	if err != nil {
		t.Fatal(err)
	}
	records, err := s.Records(ctx, 0)
	if err != nil {
		t.Fatal(err)
	}
	if len(records) != 2 || string(records[1].Payload) != string(ext.Payload) || records[1].SchemaVersion != 37 {
		t.Fatalf("facts changed: %#v", records)
	}
	page, err = r.Page(ctx, "owner", "native", 0, 1, 0)
	if err != nil || !page.Entries[0].Time.Equal(records[0].Time) || !page.HasMore {
		t.Fatalf("metadata differs: %+v %v", page, err)
	}
}

func TestDuplicateOutcomeRollsBackArtifactsAndAllFacts(t *testing.T) {
	r, s := repoSession(t)
	ctx := context.Background()
	intent := abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordModelIntent, ModelIntent: &abi.ModelIntent{RequestID: "same"}})
	if _, err := s.AppendRecords(ctx, []abi.SessionRecord{intent, intent}); err == nil {
		t.Fatal("duplicate intent accepted")
	}
	var records, artifacts int
	r.db.QueryRow("SELECT count(*) FROM records").Scan(&records)
	r.db.QueryRow("SELECT count(*) FROM artifacts").Scan(&artifacts)
	if records != 0 || artifacts != 0 {
		t.Fatalf("rollback leaked facts/artifacts: %d %d", records, artifacts)
	}
	if _, err := s.AppendRecords(ctx, []abi.SessionRecord{abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordModelOutcome, ModelOutcome: &abi.ModelOutcome{RequestID: "unknown", Status: abi.OutcomeSucceeded}})}); err == nil {
		t.Fatal("orphan outcome accepted")
	}
}

func TestMetadataInspectionDoesNotLoadCorruptRecordBody(t *testing.T) {
	r, s := repoSession(t)
	ctx := context.Background()
	if _, err := s.AppendRecords(ctx, []abi.SessionRecord{conversation(strings.Repeat("日志🙂", 10000))}); err != nil {
		t.Fatal(err)
	}
	page, err := r.Page(ctx, "owner", "native", 0, 1, 0)
	if err != nil {
		t.Fatal(err)
	}
	key, _ := artifactID(page.Entries[0].Value.URI)
	if _, err = r.db.Exec("UPDATE artifact_chunks SET data=? WHERE artifact_id=? AND chunk_index=0", []byte("corrupt"), key); err != nil {
		t.Fatal(err)
	}
	if _, err = r.OpenSession(ctx, "owner", "native"); err != nil {
		t.Fatal("cold lookup loaded body", err)
	}
	if metadata, err := r.Page(ctx, "owner", "native", 0, 1, 0); err != nil || len(metadata.Entries) != 1 {
		t.Fatalf("metadata restored runtime: %v", err)
	}
	if _, err = s.Records(ctx, 0); err == nil {
		t.Fatal("corrupt body restored")
	}
	if _, err = r.ReadArtifact(ctx, "owner", "native", page.Entries[0].Value.URI, 0, 100); err == nil {
		t.Fatal("corrupt range delivered")
	}
}

func TestOwnerAndSessionBoundArtifactsCannotBeReadByHashOrURI(t *testing.T) {
	r, _ := repoSession(t)
	ctx := context.Background()
	if _, err := r.Create(ctx, "other", "foreign", ""); err != nil {
		t.Fatal(err)
	}
	if _, err := r.Create(ctx, "owner", "sibling", ""); err != nil {
		t.Fatal(err)
	}
	backend, err := r.BoundReferences(ctx, "owner", "native")
	if err != nil {
		t.Fatal(err)
	}
	reference, err := backend.StoreText(ctx, "private source")
	if err != nil {
		t.Fatal(err)
	}
	for _, scope := range [][2]string{{"other", "native"}, {"other", "foreign"}, {"owner", "sibling"}} {
		if _, err := r.ReadArtifact(ctx, scope[0], scope[1], reference.URI, 0, 100); !errors.Is(err, ErrNotFound) {
			t.Fatalf("foreign scope granted: %v", err)
		}
	}
	if _, err := r.ReadArtifact(ctx, "owner", "native", "alva-artifact://"+reference.SHA256, 0, 100); err == nil {
		t.Fatal("hash granted global read")
	}
	if _, err := r.OpenSession(ctx, "owner", "missing"); !errors.Is(err, ErrNotFound) {
		t.Fatal(err)
	}
	list, _ := r.List(ctx, "owner", "", 100)
	if len(list.Sessions) != 2 {
		t.Fatal("unknown lookup created session")
	}
}

func TestUnicodeReferencesBoundedReadsAndCrossChunkLiteralSearch(t *testing.T) {
	r, _ := repoSession(t)
	ctx := context.Background()
	backend, _ := r.BoundReferences(ctx, "owner", "native")
	body := strings.Repeat("a", 4090) + "日志🙂跨块 needle" + strings.Repeat("尾🙂", 2000)
	reference, err := backend.StoreText(ctx, body)
	if err != nil {
		t.Fatal(err)
	}
	page, err := backend.ReadText(ctx, reference.URI, 4090, 4096)
	if err != nil {
		t.Fatal(err)
	}
	if len(page.Text) > 4096 || !utf8.ValidString(page.Text) || page.NextOffset <= page.Offset || !page.Truncated {
		t.Fatalf("invalid bounded Unicode page: %+v", page)
	}
	if _, err = backend.ReadText(ctx, reference.URI, 4091, 100); err == nil {
		t.Fatal("mid-rune read accepted")
	}
	matches, err := backend.SearchText(ctx, reference.URI, "日志🙂跨块", 20)
	if err != nil {
		t.Fatal(err)
	}
	if len(matches) != 1 || matches[0].Offset != 4090 || len(matches[0].Text) > 256 {
		t.Fatalf("cross-boundary literal match lost: %+v", matches)
	}
	if _, err = backend.SearchText(ctx, reference.URI, "[", 1); err != nil {
		t.Fatal("literal search treated regex as expression", err)
	}
}

func TestRecoveryMarksUnknownWithoutReplayingOrImplicitReads(t *testing.T) {
	ctx := context.Background()
	dir := filepath.Join(t.TempDir(), "private")
	r, err := Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	s, _ := r.Create(ctx, "owner", "native", "")
	call := wire.ToolCall{ID: "call", Name: "external_side_effect", Arguments: []byte(`{}`)}
	message := wire.Message{Role: wire.RoleAssistant, Content: []wire.ContentBlock{{Kind: wire.ContentToolCall, ToolCall: &call}}}
	batch := []abi.SessionRecord{abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordModelIntent, ModelIntent: &abi.ModelIntent{RequestID: "unresolved-model"}}), abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: &message}), abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordToolIntent, ToolIntent: &abi.ToolIntent{RunID: "interrupted-run", Call: call}})}
	if _, err = s.AppendRecords(ctx, batch); err != nil {
		t.Fatal(err)
	}
	r.Close()
	r, err = Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	defer r.Close()
	before, _ := r.Page(ctx, "owner", "native", 0, 100, 0)
	if before.HighWater != 3 {
		t.Fatal("Open replayed/recovered implicitly")
	}
	report, err := r.Recover(ctx, "owner", "native")
	if err != nil {
		t.Fatal(err)
	}
	if report.ModelIntents != 1 || report.UnknownToolExecutions != 1 {
		t.Fatalf("wrong recovery: %+v", report)
	}
	s, _ = r.OpenSession(ctx, "owner", "native")
	records, err := s.Records(ctx, 0)
	if err != nil {
		t.Fatal(err)
	}
	unknown := false
	outcome := false
	for _, record := range records {
		if record.Namespace == "alva.native.recovery" {
			unknown = strings.Contains(string(record.Payload), "unknown")
		}
		if record.ToolOutcome != nil {
			outcome = record.ToolOutcome.Status == abi.OutcomeCancelled && strings.Contains(record.ToolOutcome.Error, "unknown")
		}
	}
	if !unknown || !outcome {
		t.Fatal("interruption pretended to be success/not-executed")
	}
	again, err := r.Recover(ctx, "owner", "native")
	if err != nil || again != (RecoveryReport{}) {
		t.Fatalf("recovery repeated work: %+v %v", again, err)
	}
}

func TestRunClaimPreventsRecoveryAndConcurrentExecution(t *testing.T) {
	r, _ := repoSession(t)
	ctx := context.Background()
	_, release, err := r.BeginRun(ctx, "owner", "native")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = r.Recover(ctx, "owner", "native"); !errors.Is(err, ErrActive) {
		t.Fatalf("live recovery allowed: %v", err)
	}
	if _, _, err = r.BeginRun(ctx, "owner", "native"); !errors.Is(err, ErrActive) {
		t.Fatal("parallel run claim granted")
	}
	if err = r.Close(); err == nil {
		t.Fatal("live repo closed")
	}
	release()
	release()
}

func TestTenThousandMetadataRowsUseStableHighWaterAndBoundedPages(t *testing.T) {
	if testing.Short() {
		t.Skip("10k durable stress fixture")
	}
	r, s := repoSession(t)
	ctx := context.Background()
	for batch := 0; batch < 40; batch++ {
		records := make([]abi.SessionRecord, 250)
		for index := range records {
			records[index] = abi.SessionRecord{Kind: abi.RecordExtension, Namespace: "fixture", Type: "event", SchemaVersion: 1, Payload: []byte(`{"message":"needle [literal] 日志"}`)}
		}
		if _, err := s.AppendRecords(ctx, records); err != nil {
			t.Fatal(err)
		}
	}
	first, err := r.Page(ctx, "owner", "native", 0, 100, 0)
	if err != nil || first.HighWater != 10000 || len(first.Entries) != 100 || !first.HasMore {
		t.Fatalf("first page %+v %v", first, err)
	}
	if _, err = s.AppendRecords(ctx, []abi.SessionRecord{conversation("new after snapshot")}); err != nil {
		t.Fatal(err)
	}
	count := len(first.Entries)
	cursor := first.Next
	for cursor < first.HighWater {
		page, err := r.Page(ctx, "owner", "native", cursor, 100, first.HighWater)
		if err != nil {
			t.Fatal(err)
		}
		if len(page.Entries) > 100 {
			t.Fatal("unbounded page")
		}
		count += len(page.Entries)
		cursor = page.Next
	}
	if count != 10000 {
		t.Fatalf("snapshot drifted: %d", count)
	}
	search, err := r.Search(ctx, "owner", "native", "[literal]", 0, 20, first.HighWater)
	if err != nil || len(search.Matches) != 20 || !search.HasMore {
		t.Fatalf("literal bounded search: %+v %v", search, err)
	}
	if _, err = s.Records(ctx, 0); err == nil {
		t.Fatal("full runtime restore silently exceeded hard budget")
	}
}
