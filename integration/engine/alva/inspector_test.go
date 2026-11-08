// INPUT: Real isolated SQLite native facts, active run claim and bounded observer sink
// OUTPUT: Cold fixed snapshots, backward pages with pairing fields, exact reference/owner isolation, export and observer detach without execution cancellation regressions
// POS: [L7 Alva product adapter] Inspector tests use original committed facts without constructing an Agent or external process.
package alva

import (
	"context"
	"encoding/json"
	"errors"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestNativeInspectorColdSnapshotAndBackpressuredDetachKeepsRunOwner(t *testing.T) {
	ctx := t.Context()
	repo, err := session.Open(ctx, filepath.Join(t.TempDir(), "native"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := repo.Close(); err != nil {
			t.Error(err)
		}
	})
	log, err := repo.Create(ctx, "owner", "root", "")
	if err != nil {
		t.Fatal(err)
	}
	appendFact := func(text string) {
		t.Helper()
		raw, _ := json.Marshal(map[string]string{"text": text})
		_, err := log.AppendRecords(ctx, []abi.SessionRecord{{Kind: abi.RecordExtension, Namespace: "fixture", Type: "inspection", SchemaVersion: 1, Payload: raw}})
		if err != nil {
			t.Fatal(err)
		}
	}
	appendFact("first original")
	i, err := NewRepositoryInspector(repo)
	if err != nil {
		t.Fatal(err)
	}
	scope := engine.InspectionScope{OwnerID: "owner", NativeID: "root"}
	page, err := i.Page(ctx, scope, engine.InspectionQuery{Limit: 10})
	if err != nil || len(page.Entries) != 1 {
		t.Fatal(page, err)
	}
	high := page.HighWater
	text, err := i.Read(ctx, scope, page.Entries[0].Value.URI, 0)
	if err != nil || !strings.Contains(text.Text, "first original") {
		t.Fatal(text, err)
	}
	if _, err = i.Read(ctx, engine.InspectionScope{OwnerID: "foreign", NativeID: "root"}, text.URI, 0); err == nil {
		t.Fatal("foreign owner read record")
	}
	appendFact("second original")
	fixed, err := i.Page(ctx, scope, engine.InspectionQuery{Limit: 10, HighWater: high})
	if err != nil || fixed.HighWater != high || len(fixed.Entries) != 1 {
		t.Fatal("snapshot moved", fixed, err)
	}
	bundle, err := i.Export(ctx, scope, high, false)
	if err != nil || bundle.Records != 1 || strings.Contains(string(bundle.Data), "second original") {
		t.Fatal("export escaped fixed snapshot", err)
	}
	_, release, err := repo.BeginRun(ctx, "owner", "root")
	if err != nil {
		t.Fatal(err)
	}
	defer release()
	observing, detach := context.WithCancel(ctx)
	defer detach()
	entered, settle := make(chan struct{}), make(chan struct{})
	done := make(chan error, 1)
	sinkErr := errors.New("explicit observer detach")
	go func() {
		done <- i.Observe(observing, scope, high, func(p engine.InspectionPage) error {
			if len(p.Entries) != 1 || p.Entries[0].Sequence != high+1 {
				return errors.New("observer skipped committed cursor")
			}
			close(entered)
			<-settle
			return sinkErr
		})
	}()
	select {
	case <-entered:
	case <-time.After(5 * time.Second):
		t.Fatal("observer did not see committed fact")
	}
	appendFact("third original while observer backpressured")
	detach()
	select {
	case <-done:
		t.Fatal("observer returned before its actual sink settled")
	default:
	}
	close(settle)
	select {
	case err := <-done:
		if !errors.Is(err, sinkErr) {
			t.Fatal(err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("observer failed to detach")
	}
	if _, _, err = repo.BeginRun(ctx, "owner", "root"); !errors.Is(err, storage.ErrActive) {
		t.Fatal("observer released execution ownership", err)
	}
	latest, err := i.Page(ctx, scope, engine.InspectionQuery{Limit: 10})
	if err != nil || len(latest.Entries) != 3 {
		t.Fatal("observer detach changed committed facts", latest, err)
	}
}

func TestNativeInspectorBackwardPagesAndPairingFields(t *testing.T) {
	ctx := t.Context()
	repo, err := session.Open(ctx, filepath.Join(t.TempDir(), "native"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = repo.Close() })
	log, err := repo.Create(ctx, "owner", "root", "")
	if err != nil {
		t.Fatal(err)
	}
	started := time.Date(2026, 11, 7, 3, 0, 0, 0, time.UTC)
	for _, batch := range [][]abi.SessionRecord{
		{abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordModelIntent, ModelIntent: &abi.ModelIntent{RequestID: "request"}})},
		{abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordModelOutcome, ModelOutcome: &abi.ModelOutcome{RequestID: "request", Status: abi.OutcomeSucceeded}}),
			abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordToolIntent, ToolIntent: &abi.ToolIntent{RunID: "run", Call: wire.ToolCall{ID: "call", Name: "echo", Arguments: json.RawMessage(`{}`)}}})},
		{abi.BuiltinRecord(abi.SessionRecord{Kind: abi.RecordToolOutcome, ToolOutcome: &abi.ToolOutcome{RunID: "run", CallID: "call", Status: abi.OutcomeSucceeded, Result: wire.ToolResult{CallID: "call"}, StartedAt: started, EndedAt: started.Add(time.Second)}})},
	} {
		if _, err := log.AppendRecords(ctx, batch); err != nil {
			t.Fatal(err)
		}
	}
	i, err := NewRepositoryInspector(repo)
	if err != nil {
		t.Fatal(err)
	}
	if !i.InspectionCapabilities().BackwardPages {
		t.Fatal("RepositoryInspector honours Before but does not declare backward pages")
	}
	scope := engine.InspectionScope{OwnerID: "owner", NativeID: "root"}
	tail, err := i.Page(ctx, scope, engine.InspectionQuery{Before: engine.InspectionEnd, Limit: 2})
	if err != nil || len(tail.Entries) != 2 || tail.Entries[0].Sequence != 3 || tail.Entries[1].Sequence != 4 || tail.Before != 3 || !tail.HasEarlier || tail.HasMore || tail.Next != 4 {
		t.Fatal("tail page", tail, err)
	}
	outcome := tail.Entries[1]
	if outcome.RunID != "run" || outcome.CallID != "call" || outcome.Status != "succeeded" || !outcome.StartedAt.Equal(started) || !outcome.EndedAt.Equal(started.Add(time.Second)) {
		t.Fatal("tool outcome pairing lost through the engine DTO", outcome)
	}
	earlier, err := i.Page(ctx, scope, engine.InspectionQuery{Before: tail.Before, HighWater: tail.HighWater, Limit: 20})
	if err != nil || len(earlier.Entries) != 2 || earlier.HasEarlier || earlier.Entries[0].RequestID != "request" || earlier.Entries[1].Status != "succeeded" || !earlier.HasMore {
		t.Fatal("earlier page", earlier, err)
	}
	forward, err := i.Page(ctx, scope, engine.InspectionQuery{Limit: 20})
	if err != nil || forward.Before != 0 || forward.HasEarlier || forward.Next != 4 {
		t.Fatal("forward page changed meaning", forward, err)
	}
	for _, bad := range []engine.InspectionQuery{
		{Before: 3, After: 1, Limit: 2},
		{Before: engine.InspectionEnd, Literal: "echo", Limit: 2},
		{Before: engine.InspectionEnd, Limit: 21},
	} {
		if _, err := i.Page(ctx, scope, bad); err == nil {
			t.Fatal("invalid backward query accepted", bad)
		}
	}
}
