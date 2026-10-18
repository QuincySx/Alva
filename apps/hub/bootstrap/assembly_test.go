// INPUT: Shared product application recipe and temporary owner-only Go stores
// OUTPUT: Startup no-execution, retained native resume, explicit config and shutdown regressions
// POS: [L7 Hub product] Headless actual demo harness integration; no credentials, vendor executable or paid model
package bootstrap

import (
	"context"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/llm/provider"
)

func closeApp(t *testing.T, a *Application) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err := a.Close(ctx); err != nil {
		t.Fatal(err)
	}
}
func awaitTerminal(t *testing.T, a *Application, id string) hub.Detail {
	t.Helper()
	// Real Chrome startup/owned cleanup may each have independent finite
	// budgets. Do not turn an otherwise explicit terminal failure into a
	// misleading five-second "demo" polling timeout under full race builds.
	deadline := time.Now().Add(15 * time.Second)
	for time.Now().Before(deadline) {
		detail, err := a.Store.Detail(context.Background(), id)
		if err != nil {
			t.Fatal(err)
		}
		// Explicit test-only consent to the actual harmless fixture invocation.
		// The production recipe never approves unknown tools automatically.
		if branch, e := product.ActiveBranch(detail); e == nil && branch.NativeSessionID != "" {
			for _, request := range a.Approvals.Pending(branch.NativeSessionID) {
				if e := a.Service.RespondApproval(context.Background(), id, request.ID, true); e != nil {
					t.Fatal(e)
				}
			}
		}
		if detail.Session.Status != hub.Running && !a.Runtime.HasActive() {
			if detail.Session.Status != engine.Succeeded {
				t.Fatalf("unexpected terminal: %+v", detail.Session)
			}
			return detail
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("demo did not terminate")
	return hub.Detail{}
}
func TestSharedAssemblyStartupAndNativeResumeAcrossReopen(t *testing.T) {
	ctx := context.Background()
	dir := filepath.Join(t.TempDir(), "private")
	a, err := Open(ctx, Config{DataDirectory: dir, Demo: true})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { closeApp(t, a) })
	page, err := a.Store.PageSessions(ctx, "", hub.SessionCursor{}, 64)
	if err != nil || len(page.Sessions) != 0 || a.Runtime.HasActive() || len(a.Targets) != 1 {
		t.Fatal("startup created work", err)
	}
	s, err := a.Service.Create(ctx, "same real session", "")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = a.Service.Submit(ctx, s.ID, "first retained turn"); err != nil {
		t.Fatal(err)
	}
	detail := awaitTerminal(t, a, s.ID)
	branch, err := product.ActiveBranch(detail)
	if err != nil || branch.NativeSessionID == "" {
		t.Fatal("missing native authority", err)
	}
	closeApp(t, a)
	b, err := Open(ctx, Config{DataDirectory: dir, Demo: true})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { closeApp(t, b) })
	if _, err = b.Service.Submit(ctx, s.ID, "second retained turn"); err != nil {
		t.Fatal(err)
	}
	resumed := awaitTerminal(t, b, s.ID)
	resumedBranch, err := product.ActiveBranch(resumed)
	if err != nil || resumedBranch.NativeSessionID != branch.NativeSessionID {
		t.Fatal("reopen silently created new native identity", err)
	}
	history, err := b.Store.History(ctx, s.ID, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	inputs := 0
	for _, entry := range history {
		if entry.Input != "" {
			inputs++
		}
	}
	if inputs != 2 {
		t.Fatalf("duplicated/lost product turns: %d", inputs)
	}
}
func TestLiveOpenMakesNoHTTPCallsAndRejectsMislabeledRoute(t *testing.T) {
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { calls.Add(1); http.Error(w, "must not call", 500) }))
	defer server.Close()
	a, err := Open(context.Background(), Config{DataDirectory: filepath.Join(t.TempDir(), "private"), ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: server.URL + "/chat", Model: "explicit-model"}})
	if err != nil {
		t.Fatal(err)
	}
	defer closeApp(t, a)
	if calls.Load() != 0 || a.Runtime.HasActive() {
		t.Fatal("live open contacted model")
	}
	s, err := a.Service.Create(context.Background(), "route", "")
	if err != nil {
		t.Fatal(err)
	}
	target := a.Targets[0]
	target.ModelID = "mislabeled-model"
	if _, err = a.Store.Switch(context.Background(), s.ID, target, "explicit test route"); err != nil {
		t.Fatal(err)
	}
	if _, err = a.Service.Submit(context.Background(), s.ID, "do not run another model"); err != nil {
		t.Fatal(err)
	}
	deadline := time.Now().Add(5 * time.Second)
	for a.Runtime.HasActive() && time.Now().Before(deadline) {
		time.Sleep(time.Millisecond)
	}
	detail, err := a.Store.Detail(context.Background(), s.ID)
	if err != nil || detail.Session.Status != engine.Failed || calls.Load() != 0 {
		t.Fatal("route label did not bind actual provider", err)
	}
}
func TestUnapprovedExternalRejectedBeforeCreatingPrivateData(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "private")
	_, err := Open(context.Background(), Config{DataDirectory: dir, Demo: true, External: []ExternalEngine{{Vendor: "codex", Command: "not-started"}}})
	if err == nil {
		t.Fatal("unapproved external accepted")
	}
	if _, err = os.Stat(dir); !os.IsNotExist(err) {
		t.Fatal("invalid admission mutated data directory")
	}
}
