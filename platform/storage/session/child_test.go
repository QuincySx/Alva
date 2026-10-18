// INPUT: context, encoding/json, errors, testing, kernel/abi, llm/wire.
// OUTPUT: Atomic actual-child/link, ownership and metadata-only ancestry regressions.
// POS: [platform / durable storage] Child creation never fabricates a causal parent intent or acquires another session's history authority.
package session

import (
	"context"
	"encoding/json"
	"errors"
	"testing"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestCreateChildRequiresActualIntentAndCommitsExactLinkAtomically(t *testing.T) {
	repo, parent := repoSession(t)
	ctx := context.Background()
	if _, err := repo.CreateChild(ctx, "owner", parent.ID(), "child", "run", "call", "scope"); err == nil {
		t.Fatal("created child without committed parent intent")
	}
	if _, err := repo.OpenSession(ctx, "owner", "child"); !errors.Is(err, ErrNotFound) {
		t.Fatal("partial child leaked", err)
	}
	intent := abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordToolIntent, ToolIntent: &abi.ToolIntent{RunID: "run", Call: wire.ToolCall{ID: "call", Name: "agent", Arguments: json.RawMessage(`{"task":"independent"}`)}}})
	committed, err := parent.AppendRecords(ctx, []abi.SessionRecord{intent})
	if err != nil {
		t.Fatal(err)
	}
	if _, err = repo.CreateChild(ctx, "foreign", parent.ID(), "child", "run", "call", "scope"); !errors.Is(err, ErrNotFound) {
		t.Fatal("foreign owner granted", err)
	}
	creation, err := repo.CreateChild(ctx, "owner", parent.ID(), "child", "run", "call", "scope")
	if err != nil {
		t.Fatal(err)
	}
	if creation.Session.ID() != "child" || creation.Link.SessionID != parent.ID() {
		t.Fatal("actual target identity changed")
	}
	if p, err := repo.Parent(ctx, "owner", "child"); err != nil || p != parent.ID() {
		t.Fatal(p, err)
	}
	resolved, err := repo.ResolveChildLink(ctx, "owner", parent.ID(), creation.Link.Sequence)
	if err != nil || resolved.ChildID != "child" || resolved.Link != creation.Link {
		t.Fatal("typed relation resolution failed", resolved, err)
	}
	reversed, err := repo.ParentLink(ctx, "owner", "child")
	if err != nil || reversed != resolved {
		t.Fatal("reverse typed relation mismatch", reversed, err)
	}
	records, err := parent.Records(ctx, 0)
	if err != nil {
		t.Fatal(err)
	}
	link := records[1]
	var payload struct {
		Child string `json:"child_session_id"`
	}
	json.Unmarshal(link.Payload, &payload)
	if link.Namespace != "alva.agent" || link.Type != "child_session_linked" || link.SchemaVersion != 1 || link.Parent.RecordID != committed[0].ID || payload.Child != "child" {
		t.Fatalf("wrong typed causal link: %+v", link)
	}
	if _, err = repo.CreateChild(ctx, "owner", parent.ID(), "child", "run", "call", "scope"); err == nil {
		t.Fatal("reused actual child")
	}
	again, _ := parent.Records(ctx, 0)
	if len(again) != 2 {
		t.Fatal("failed duplicate appended extra link")
	}
	opened, _ := repo.OpenSession(ctx, "owner", parent.ID())
	if !opened.SameAuthority(parent) || opened.SameAuthority(creation.Session) {
		t.Fatal("authority comparison uses ID only")
	}
	refs, _ := repo.BoundReferences(ctx, "owner", parent.ID())
	ref, _ := refs.StoreText(ctx, "parent private body")
	if _, err = repo.ReadArtifact(ctx, "owner", "child", ref.URI, 0, 1024); !errors.Is(err, ErrNotFound) {
		t.Fatal("causal link granted history access", err)
	}
}

func TestResolveChildLinkReadsOnlyBoundedLinkAndMetadata(t *testing.T) {
	repo, parent := repoSession(t)
	ctx := context.Background()
	intent := abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordToolIntent, ToolIntent: &abi.ToolIntent{RunID: "run", Call: wire.ToolCall{ID: "call", Name: "agent", Arguments: json.RawMessage(`{"task":"original body need not be read"}`)}}})
	_, err := parent.AppendRecords(ctx, []abi.SessionRecord{intent})
	if err != nil {
		t.Fatal(err)
	}
	creation, err := repo.CreateChild(ctx, "owner", parent.ID(), "child", "run", "call", "scope")
	if err != nil {
		t.Fatal(err)
	}
	var intentKey string
	repo.db.QueryRow("SELECT artifact_id FROM records WHERE session_id=? AND seq=1", parent.ID()).Scan(&intentKey)
	if _, err = repo.db.Exec("UPDATE artifact_chunks SET data=? WHERE artifact_id=?", []byte("corrupt unrelated intent body"), intentKey); err != nil {
		t.Fatal(err)
	}
	if _, err = repo.ResolveChildLink(ctx, "owner", parent.ID(), creation.Link.Sequence); err != nil {
		t.Fatal("inspection decompressed/read parent tool body", err)
	}
	if _, err = repo.ParentLink(ctx, "owner", "child"); err != nil {
		t.Fatal("reverse lookup read unrelated intent body", err)
	}
	for _, scope := range []struct {
		owner, id string
		seq       uint64
	}{{"foreign", parent.ID(), creation.Link.Sequence}, {"owner", "child", creation.Link.Sequence}, {"owner", parent.ID(), 1}} {
		if _, err = repo.ResolveChildLink(ctx, scope.owner, scope.id, scope.seq); err == nil {
			t.Fatal("invalid/foreign typed source accepted")
		}
	}
	if _, err = repo.db.Exec("UPDATE records SET call_id='forged' WHERE session_id=? AND seq=1", parent.ID()); err != nil {
		t.Fatal(err)
	}
	if _, err = repo.ResolveChildLink(ctx, "owner", parent.ID(), creation.Link.Sequence); err == nil {
		t.Fatal("mismatched causal invocation index accepted")
	}
	if _, err = repo.db.Exec("UPDATE records SET call_id='call' WHERE session_id=? AND seq=1", parent.ID()); err != nil {
		t.Fatal(err)
	}
	var linkKey string
	repo.db.QueryRow("SELECT artifact_id FROM records WHERE session_id=? AND seq=?", parent.ID(), creation.Link.Sequence).Scan(&linkKey)
	if _, err = repo.db.Exec("UPDATE artifact_chunks SET data=? WHERE artifact_id=?", []byte("corrupt target link"), linkKey); err != nil {
		t.Fatal(err)
	}
	if _, err = repo.ResolveChildLink(ctx, "owner", parent.ID(), creation.Link.Sequence); err == nil {
		t.Fatal("corrupt target link bytes accepted")
	}
}

func TestParentLinkDoesNotInventOrScanOldMetadataRelations(t *testing.T) {
	repo, parent := repoSession(t)
	ctx := context.Background()
	if _, err := repo.Create(ctx, "owner", "metadata-only-child", parent.ID()); err != nil {
		t.Fatal(err)
	}
	if _, err := repo.ParentLink(ctx, "owner", "metadata-only-child"); !errors.Is(err, ErrNotFound) {
		t.Fatal("metadata relation became typed grant", err)
	}
	if _, err := repo.ParentLink(ctx, "foreign", "metadata-only-child"); !errors.Is(err, ErrNotFound) {
		t.Fatal("foreign reverse metadata disclosure", err)
	}
}
