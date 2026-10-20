// INPUT: testing, contexts, shared storage ports and public SDK session conformance.
// OUTPUT: RunRepository and RunBindings reusable backend acceptance suites.
// POS: [storage contract testing] Verifies the same consumer-facing contracts without a concrete backend or SQL dependency.
package storagetest

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"sync"
	"testing"

	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/kernel/abi/sessiontest"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

// RunRepository requires a fresh empty repository per case, with no fixture
// redaction overrides. The factory owns cleanup after all calls have joined.
// This is the full application profile; SDK-only log and byte-store authors use
// sessiontest.Run and blob/blobtest.Run independently. Passing these small cases
// does not certify disk/cloud durability, outages, retention, process locks,
// corruption handling, Close retries or capacity limits; test those per adapter.
func RunRepository(t *testing.T, open func(*testing.T) storage.Repository) {
	t.Helper()
	t.Run("sdk_session", func(t *testing.T) {
		sessiontest.Run(t, func(t *testing.T) abi.PersistentSession {
			return create(t, open(t), "session")
		})
	})
	for _, test := range []struct {
		name string
		run  func(*testing.T, storage.Repository)
	}{
		{"owner_and_authority", ownerAndAuthority},
		{"snapshot_cas", snapshotCAS},
		{"fixed_history", fixedHistory},
		{"artifact_scope", artifactScope},
		{"execution_claim", executionClaim},
		{"causal_children", causalChildren},
	} {
		t.Run(test.name, func(t *testing.T) { test.run(t, open(t)) })
	}
}

func create(t *testing.T, repo storage.Sessions, id string) storage.Session {
	t.Helper()
	log, err := repo.Create(t.Context(), "owner", id, "")
	if err != nil {
		t.Fatal(err)
	}
	return log
}

func conversation(text string) abi.SessionRecord {
	m := wire.TextMessage(wire.RoleUser, text)
	return abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordConversation, Message: &m})
}

func appendRecords(t *testing.T, log storage.Session, records ...abi.SessionRecord) []abi.SessionRecord {
	t.Helper()
	committed, err := log.AppendRecords(t.Context(), records)
	if err != nil {
		t.Fatal(err)
	}
	return committed
}

func ownerAndAuthority(t *testing.T, repo storage.Repository) {
	log := create(t, repo, "a")
	other := create(t, repo, "b")
	reopened, err := repo.OpenSession(t.Context(), "owner", "a")
	if err != nil || !reopened.SameAuthority(log) || log.SameAuthority(other) || log.SameAuthority(core.NewMemorySession(log.ID())) {
		t.Fatal("provider/session authority replaced by a textual ID", err)
	}
	if _, err := repo.OpenSession(t.Context(), "foreign", "a"); !errors.Is(err, storage.ErrNotFound) {
		t.Fatal("foreign owner admitted", err)
	}
	if _, err := repo.OpenSession(t.Context(), "owner", "missing"); !errors.Is(err, storage.ErrNotFound) {
		t.Fatal("missing session did not use shared error", err)
	}
	if _, err := repo.Create(t.Context(), "foreign", "a", ""); err == nil {
		t.Fatal("existing session retargeted to another owner")
	}
	first, err := repo.List(t.Context(), "owner", "", 1)
	if err != nil || len(first.Sessions) != 1 || !first.HasMore || first.Next == "" {
		t.Fatal("invalid first session page", first, err)
	}
	next, err := repo.List(t.Context(), "owner", first.Next, 1)
	if err != nil || len(next.Sessions) != 1 || next.HasMore || next.Sessions[0].ID == first.Sessions[0].ID {
		t.Fatal("session cursor is not exclusive", next, err)
	}
	foreign, err := repo.List(t.Context(), "foreign", "", 10)
	if err != nil || len(foreign.Sessions) != 0 {
		t.Fatal("foreign session metadata disclosed", err)
	}
}

func snapshotCAS(t *testing.T, repo storage.Repository) {
	log := create(t, repo, "snapshot")
	initial, err := log.AppendExtensionSnapshot(t.Context(), "example.contract", "state", 0, json.RawMessage(`{"revision":1}`), conversation("initial"))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := log.AppendExtensionSnapshot(t.Context(), "example.contract", "state", 0, json.RawMessage(`{}`), conversation("stale must not commit")); !errors.Is(err, storage.ErrRevisionConflict) {
		t.Fatal("stale CAS accepted", err)
	}
	bad := conversation("invalid")
	bad.Message.Role = "invalid"
	if _, err := log.AppendExtensionSnapshot(t.Context(), "example.contract", "state", initial.Sequence, json.RawMessage(`{}`), bad); err == nil {
		t.Fatal("CAS partially accepted invalid additional records")
	}
	start := make(chan struct{})
	results := make(chan error, 2)
	var workers sync.WaitGroup
	for range 2 {
		workers.Go(func() {
			<-start
			_, err := log.AppendExtensionSnapshot(t.Context(), "example.contract", "state", initial.Sequence, json.RawMessage(`{"revision":2}`), conversation("one delivery"))
			results <- err
		})
	}
	close(start)
	workers.Wait()
	close(results)
	succeeded, conflicted := 0, 0
	for err := range results {
		if err == nil {
			succeeded++
		} else if errors.Is(err, storage.ErrRevisionConflict) {
			conflicted++
		} else {
			t.Fatal(err)
		}
	}
	if succeeded != 1 || conflicted != 1 {
		t.Fatal("CAS permitted multiple winners", succeeded, conflicted)
	}
	records, err := log.Records(t.Context(), 0)
	if err != nil || len(records) != 4 {
		t.Fatal("failed CAS or validation wrote facts", len(records), err)
	}
	messages, err := log.Messages(t.Context())
	if err != nil || len(messages) != 2 || messages[1].Text() != "one delivery" {
		t.Fatal("CAS result delivered more than once", err)
	}
	appendRecords(t, log, abi.SessionRecord{Kind: abi.RecordExtension, Namespace: "unrelated.contract", Type: "state", SchemaVersion: 1, Payload: json.RawMessage(`{}`)})
	latest, found, err := repo.LatestRecord(t.Context(), "owner", log.ID(), "example.contract", "state")
	if err != nil || !found || latest.Sequence <= initial.Sequence || latest.Sequence > 4 {
		t.Fatal("latest lookup mixed unrelated streams", latest, found, err)
	}
	if _, found, err := repo.LatestRecord(t.Context(), "foreign", log.ID(), "example.contract", "state"); err == nil || found {
		t.Fatal("foreign latest read admitted", err)
	}
}

func fixedHistory(t *testing.T, repo storage.Repository) {
	log := create(t, repo, "history")
	appendRecords(t, log, conversation("first needle"), conversation("second needle"), conversation("third needle"))
	page, err := repo.Page(t.Context(), "owner", log.ID(), 0, 1, 0)
	if err != nil || len(page.Entries) != 1 || !page.HasMore || page.HighWater != 3 {
		t.Fatal("invalid bounded metadata page", page, err)
	}
	tail, err := repo.BoundConversationTail(t.Context(), "owner", log.ID(), 3)
	if err != nil || tail.SessionID() != log.ID() || tail.HighWater() != 3 {
		t.Fatal("invalid conversation snapshot", err)
	}
	metadata, err := tail.Metadata()
	if err != nil || len(metadata) != 3 || metadata[0].Record.Sequence != 3 || metadata[2].Record.Sequence != 1 {
		t.Fatal("metadata must be newest first", metadata, err)
	}
	metadata[0].Reference.URI = "mutated"
	metadata[0].Record.RecordID = "mutated"
	again, err := tail.Metadata()
	if err != nil || again[0].Reference.URI == "mutated" || again[0].Record.RecordID == "mutated" {
		t.Fatal("metadata aliases the snapshot", err)
	}
	appendRecords(t, log, conversation("later needle"))
	next, err := repo.Page(t.Context(), "owner", log.ID(), page.Next, 8, page.HighWater)
	if err != nil || len(next.Entries) != 2 || next.HasMore || next.HighWater != page.HighWater || next.Entries[0].Sequence != 2 || next.Entries[1].Sequence != 3 {
		t.Fatal("page changed its fixed high-water or exclusive cursor", next, err)
	}
	messages, err := tail.Read(t.Context(), 2, 64<<10)
	if err != nil || len(messages) != 2 || messages[0].Text() != "second needle" || messages[1].Text() != "third needle" {
		t.Fatal("tail changed or lost chronological order", err)
	}
	if _, err := tail.Read(t.Context(), 2, 1); err == nil {
		t.Fatal("tail ignored byte budget")
	}
	search, err := repo.Search(t.Context(), "owner", log.ID(), "needle", 0, 1, page.HighWater)
	if err != nil || len(search.Matches) != 1 || search.HighWater != 3 || !search.HasMore {
		t.Fatal("search ignored limit or fixed high-water", search, err)
	}
	nextSearch, err := repo.Search(t.Context(), "owner", log.ID(), "needle", search.Next, 8, search.HighWater)
	if err != nil || len(nextSearch.Matches) != 2 || nextSearch.HasMore || nextSearch.Matches[0].Entry.Sequence <= search.Matches[0].Entry.Sequence {
		t.Fatal("search cursor repeated or admitted later records", nextSearch, err)
	}
	if _, err := repo.Page(t.Context(), "foreign", log.ID(), 0, 1, 0); !errors.Is(err, storage.ErrNotFound) {
		t.Fatal("foreign record page admitted", err)
	}
}

func artifactScope(t *testing.T, repo storage.Repository) {
	create(t, repo, "a")
	create(t, repo, "b")
	data := []byte("fixture original bytes 中")
	original := bytes.Clone(data)
	info, err := repo.PutArtifact(t.Context(), "owner", "a", storage.ArtifactInput{Kind: "fixture", Name: "bounded", MediaType: "text/plain", Processing: "original", Data: data, Text: true})
	hash := sha256.Sum256(original)
	if err != nil || info.SessionID != "a" || info.Bytes != int64(len(original)) || info.SHA256 != hex.EncodeToString(hash[:]) {
		t.Fatal("artifact receipt changed bytes or identity", info, err)
	}
	data[0] = '!'
	uri := info.Reference().URI
	page, err := repo.ReadArtifact(t.Context(), "owner", "a", uri, 0, 7)
	if err != nil || !bytes.Equal(page.Data, original[:7]) || page.NextOffset != 7 || !page.Truncated {
		t.Fatal("artifact input aliases or read was unbounded", err)
	}
	page.Data[0] = '!'
	_, full, err := repo.ReadArtifactFull(t.Context(), "owner", "a", uri, len(original))
	if err != nil || !bytes.Equal(full, original) {
		t.Fatal("artifact output aliases store", err)
	}
	if _, _, err := repo.ReadArtifactFull(t.Context(), "owner", "a", uri, len(original)-1); err == nil {
		t.Fatal("full artifact ignored byte budget")
	}
	for _, scope := range []struct{ owner, id string }{{"foreign", "a"}, {"owner", "b"}} {
		if _, err := repo.ReadArtifact(t.Context(), scope.owner, scope.id, uri, 0, 1); !errors.Is(err, storage.ErrNotFound) {
			t.Fatal("URI granted foreign artifact access", err)
		}
	}
	bound, err := repo.BoundReferences(t.Context(), "owner", "b")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := bound.ReadText(t.Context(), uri, 0, 20); err == nil {
		t.Fatal("bound references bypassed session authority")
	}
	bound, err = repo.BoundReferences(t.Context(), "owner", "a")
	if err != nil {
		t.Fatal(err)
	}
	text, err := bound.ReadText(t.Context(), uri, 0, 7)
	if err != nil || text.Text != string(original[:7]) || text.NextOffset != 7 {
		t.Fatal("bound reference did not retain bounded semantics", text, err)
	}
}

func executionClaim(t *testing.T, repo storage.Repository) {
	log := create(t, repo, "run")
	_, release, err := repo.BeginRun(t.Context(), "owner", log.ID())
	if err != nil {
		t.Fatal(err)
	}
	defer release()
	if _, duplicateRelease, err := repo.BeginRun(t.Context(), "owner", log.ID()); !errors.Is(err, storage.ErrActive) {
		if duplicateRelease != nil {
			duplicateRelease()
		}
		t.Fatal("parallel execution claim admitted", err)
	}
	if _, err := repo.Recover(t.Context(), "owner", log.ID()); !errors.Is(err, storage.ErrActive) {
		t.Fatal("recovery touched live execution", err)
	}
	release()
	release()
	appendRecords(t, log, abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordModelIntent, ModelIntent: &abi.ModelIntent{RequestID: "interrupted"}}))
	report, err := repo.Recover(t.Context(), "owner", log.ID())
	if err != nil || report.ModelIntents != 1 {
		t.Fatal("interrupted intent not settled", report, err)
	}
	again, err := repo.Recover(t.Context(), "owner", log.ID())
	if err != nil || again != (storage.RecoveryReport{}) {
		t.Fatal("recovery repeated work", again, err)
	}
}

func causalChildren(t *testing.T, repo storage.Repository) {
	parent := create(t, repo, "parent")
	if _, err := repo.CreateChild(t.Context(), "owner", parent.ID(), "child", "run", "call", "scope"); err == nil {
		t.Fatal("child created without actual parent intent")
	}
	if _, err := repo.OpenSession(t.Context(), "owner", "child"); !errors.Is(err, storage.ErrNotFound) {
		t.Fatal("rejected child leaked partial state", err)
	}
	appendRecords(t, parent, abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordToolIntent, ToolIntent: &abi.ToolIntent{RunID: "run", Call: wire.ToolCall{ID: "call", Name: "agent", Arguments: json.RawMessage(`{}`)}}}))
	child, err := repo.CreateChild(t.Context(), "owner", parent.ID(), "child", "run", "call", "scope")
	if err != nil || child.Session.ID() != "child" || child.Link.SessionID != parent.ID() {
		t.Fatal("child identity lost", err)
	}
	resolved, err := repo.ResolveChildLink(t.Context(), "owner", parent.ID(), child.Link.Sequence)
	if err != nil || resolved.ChildID != "child" || resolved.ParentID != parent.ID() || resolved.Link != child.Link {
		t.Fatal("actual causal link missing", resolved, err)
	}
	reversed, err := repo.ParentLink(t.Context(), "owner", "child")
	if err != nil || reversed != resolved {
		t.Fatal("reverse relation changed", reversed, err)
	}
	if _, err := repo.ResolveChildLink(t.Context(), "foreign", parent.ID(), child.Link.Sequence); !errors.Is(err, storage.ErrNotFound) {
		t.Fatal("foreign causal metadata admitted", err)
	}
}

// RunBindings requires a fresh empty namespace per subtest. Factory owns Close.
// It checks shared create-or-verify and cancellation semantics, not persistence
// or provider-specific process locking.
func RunBindings(t *testing.T, open func(*testing.T) storage.BindingStore) {
	t.Helper()
	t.Run("identity_and_cancellation", func(t *testing.T) {
		s := open(t)
		if _, err := s.ReadBinding(t.Context(), "missing"); !errors.Is(err, storage.ErrNotFound) {
			t.Fatal("missing binding did not use shared error", err)
		}
		want := storage.SessionBinding{ID: "native-id", Owner: "owner", Workspace: "/fixture"}
		for range 2 {
			if err := s.PutBinding(t.Context(), want); err != nil {
				t.Fatal("identical binding is not idempotent", err)
			}
		}
		for _, changed := range []storage.SessionBinding{{ID: want.ID, Owner: "foreign", Workspace: want.Workspace}, {ID: want.ID, Owner: want.Owner, Workspace: "/elsewhere"}} {
			if err := s.PutBinding(t.Context(), changed); !errors.Is(err, storage.ErrBindingConflict) {
				t.Fatal("binding retargeted", err)
			}
		}
		if got, err := s.ReadBinding(t.Context(), want.ID); err != nil || got != want {
			t.Fatal("rejected binding changed original", got, err)
		}
		ctx, cancel := context.WithCancel(t.Context())
		cancel()
		if err := s.PutBinding(ctx, storage.SessionBinding{ID: "cancelled", Owner: "owner", Workspace: "/fixture"}); !errors.Is(err, context.Canceled) {
			t.Fatal("cancelled binding accepted", err)
		}
		if _, err := s.ReadBinding(t.Context(), "cancelled"); !errors.Is(err, storage.ErrNotFound) {
			t.Fatal("pre-cancelled binding wrote state", err)
		}
	})
	t.Run("competing_owners", func(t *testing.T) {
		s := open(t)
		start := make(chan struct{})
		results := make(chan error, 2)
		var workers sync.WaitGroup
		for _, owner := range []string{"a", "b"} {
			workers.Go(func() {
				<-start
				results <- s.PutBinding(t.Context(), storage.SessionBinding{ID: "shared", Owner: owner, Workspace: "/fixture"})
			})
		}
		close(start)
		workers.Wait()
		close(results)
		succeeded, conflicted := 0, 0
		for err := range results {
			if err == nil {
				succeeded++
			} else if errors.Is(err, storage.ErrBindingConflict) {
				conflicted++
			} else {
				t.Fatal(err)
			}
		}
		if succeeded != 1 || conflicted != 1 {
			t.Fatal("create-or-verify allowed multiple owners", succeeded, conflicted)
		}
	})
}
