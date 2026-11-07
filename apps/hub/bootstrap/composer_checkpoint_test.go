// INPUT: Real shared bootstrap, persistent durable journal and local HTTP 429 provider failure.
// OUTPUT: Ordinary and composer Plan changes both reject an interrupted checkpoint without changing mode or admitting a Turn.
// POS: [L7 native acceptance] Composer Send shares the product's frozen-checkpoint policy after the execution owner has exited.
package bootstrap

import (
	"fmt"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/harness/durable"
	"github.com/QuincySx/Alva/integration/engine"
)

func TestComposerPlanRejectsInterruptedDurableCheckpoint(t *testing.T) {
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		calls.Add(1)
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusTooManyRequests)
		fmt.Fprint(w, `{"error":{"message":"rate limited","type":"rate_limit_error"}}`)
	}))
	defer server.Close()
	a, err := Open(t.Context(), waitConfig(t, server.URL))
	if err != nil {
		t.Fatal(err)
	}
	defer closeApp(t, a)
	p, err := a.Service.Create(t.Context(), "interrupted composer checkpoint", "")
	if err != nil {
		t.Fatal(err)
	}
	if err = a.Service.EnableDurableWaits(t.Context(), p.ID, true); err != nil {
		t.Fatal(err)
	}
	turnID, err := a.Service.Submit(t.Context(), p.ID, "Inspect the project before proposing changes", nil)
	if err != nil {
		t.Fatal(err)
	}
	before := waitStatus(t, a, p.ID, engine.Failed)
	if before.Snapshot.State.Status != durable.Interrupted || before.TurnID != turnID || calls.Load() != 1 {
		t.Fatal("fixture did not retain the actual failed execution's checkpoint", before, calls.Load())
	}
	history, err := a.Store.History(t.Context(), p.ID, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	turns := make(map[string]bool)
	for _, entry := range history {
		turns[entry.TurnID] = true
	}
	if len(turns) != 1 || !turns[turnID] {
		t.Fatal("fixture did not retain exactly one product Turn", turns)
	}
	if err = a.Service.SetPlanMode(t.Context(), p.ID, true); err == nil {
		t.Fatal("ordinary Plan change accepted an interrupted native checkpoint")
	}
	if err = a.Service.SetPermissionPolicy(t.Context(), p.ID, hub.PermissionFull, true); err == nil {
		t.Fatal("permission change accepted an interrupted native checkpoint")
	}
	selection, err := a.Service.PrepareComposerSelection(t.Context(), p.ID, product.ComposerSelectionRequest{EntryID: "builtin:plan"})
	if err != nil {
		t.Fatal(err)
	}
	if _, err = a.Service.SubmitComposerDraft(t.Context(), p.ID, "Propose a plan", []product.ComposerPreparation{selection}, nil); err == nil {
		t.Fatal("composer Plan admitted a new Turn before checkpoint review and abandonment")
	}
	settings, err := a.Store.Settings(t.Context(), p.ID)
	if err != nil || settings.PlanMode {
		t.Fatal("rejected composer Plan changed the saved permission policy", settings.PlanMode, err)
	}
	after, err := a.Service.Waits(t.Context(), p.ID)
	if err != nil || after.Status != engine.Failed || after.TurnID != before.TurnID || after.Snapshot.Revision != before.Snapshot.Revision || after.Snapshot.State.Status != durable.Interrupted {
		t.Fatal("rejected Plan change replaced the failed Turn or its checkpoint", after, err)
	}
	afterHistory, err := a.Store.History(t.Context(), p.ID, 0, 128)
	if err != nil || len(afterHistory) != len(history) || calls.Load() != 1 || a.Runtime.SessionActive(p.ID) {
		t.Fatal("rejected Plan change admitted product facts or another model request", len(afterHistory), len(history), calls.Load(), err)
	}
}
