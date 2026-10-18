// INPUT: Real isolated SQLite native facts, active run claim and bounded observer sink
// OUTPUT: Cold fixed snapshots, exact reference/owner isolation, export and observer detach without execution cancellation regressions
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

	"github.com/QuincySx/Alva/go/integration/engine"
	"github.com/QuincySx/Alva/go/platform/storage/session"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
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
	if _, _, err = repo.BeginRun(ctx, "owner", "root"); !errors.Is(err, session.ErrActive) {
		t.Fatal("observer released execution ownership", err)
	}
	latest, err := i.Page(ctx, scope, engine.InspectionQuery{Limit: 10})
	if err != nil || len(latest.Entries) != 3 {
		t.Fatal("observer detach changed committed facts", latest, err)
	}
}
