// INPUT: Real shared Hub assembly, temporary private SQLite and a local HTTP provider fixture.
// OUTPUT: Durable dynamic routes reaching actual model requests, native resume and stale-route rejection.
// POS: [L7 assembly acceptance] Connection CRUD changes the real Alva recipe without paid models or external accounts.
package bootstrap

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
)

func TestDynamicConnectionUsesRealProviderAcrossReopenAndRejectsOldRoute(t *testing.T) {
	var requests atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		requests.Add(1)
		if r.URL.Path != "/v1/chat/completions" || r.Header.Get("Authorization") != "Bearer synthetic-dynamic-key" || r.Header.Get("X-Explicit-Fixture") != "synthetic-header-value" {
			http.Error(w, "fixture route/auth mismatch", 400)
			return
		}
		var body struct {
			Model string `json:"model"`
		}
		if json.NewDecoder(r.Body).Decode(&body) != nil || body.Model != "exact-dynamic-model" {
			http.Error(w, "fixture model mismatch", 400)
			return
		}
		w.Header().Set("Content-Type", "text/event-stream")
		_, _ = w.Write([]byte("data: {\"id\":\"fixture\",\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"real dynamic answer\"},\"finish_reason\":null}]}\n\ndata: {\"id\":\"fixture\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"))
	}))
	defer server.Close()
	config := Config{OpenStores: LocalStores, DataDirectory: filepath.Join(t.TempDir(), "private"), Demo: true}
	a, err := Open(t.Context(), config)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { closeApp(t, a) }()
	saved, err := a.Service.Connections.Save(t.Context(), hub.ConnectionUpdate{Name: "Local explicit fixture", Protocol: "openai_chat", Endpoint: server.URL + "/v1/chat/completions", Auth: "api_key", APIKey: "synthetic-dynamic-key", Headers: map[string]string{"X-Explicit-Fixture": "synthetic-header-value"}, ReplaceHeaders: true, Enabled: true, Models: []hub.ConnectionModel{{ID: "exact-dynamic-model", Enabled: true}}})
	if err != nil {
		t.Fatal(err)
	}
	if requests.Load() != 0 {
		t.Fatal("saving connection executed a model request")
	}
	targets := a.Service.AvailableTargets()
	if len(targets) != 2 {
		t.Fatal("enabled model did not enter route list")
	}
	target := targets[1]
	session, err := a.Service.Create(t.Context(), "dynamic model", "")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = a.Store.Switch(t.Context(), session.ID, target, "explicit fixture model selection"); err != nil {
		t.Fatal(err)
	}
	if _, err = a.Service.Submit(t.Context(), session.ID, "actual dynamic request"); err != nil {
		t.Fatal(err)
	}
	detail := awaitTerminal(t, a, session.ID)
	branch, err := product.ActiveBranch(detail)
	if err != nil {
		t.Fatal(err)
	}
	history, err := a.Store.History(t.Context(), session.ID, 0, 128)
	if err != nil {
		t.Fatal(err)
	}
	encoded, _ := json.Marshal(history)
	if strings.Contains(string(encoded), "synthetic-dynamic-key") || strings.Contains(string(encoded), "synthetic-header-value") {
		t.Fatal("credential entered product history")
	}
	closeApp(t, a)
	a, err = Open(t.Context(), config)
	if err != nil {
		t.Fatal(err)
	}
	if requests.Load() != 1 {
		t.Fatal("reopen replayed a model request")
	}
	if _, err = a.Service.Submit(t.Context(), session.ID, "resume exact route"); err != nil {
		t.Fatal(err)
	}
	detail = awaitTerminal(t, a, session.ID)
	resumed, err := product.ActiveBranch(detail)
	if err != nil || resumed.NativeSessionID != branch.NativeSessionID || requests.Load() != 2 {
		t.Fatal("durable connection did not resume exact native route", err)
	}
	disabled, err := a.Service.Connections.SetEnabled(t.Context(), saved.ID, saved.Revision, false)
	if err != nil || disabled.Enabled {
		t.Fatal("disable", err)
	}
	if len(a.Service.AvailableTargets()) != 1 {
		t.Fatal("disabled route remained selectable")
	}
	if _, err = a.Service.Submit(t.Context(), session.ID, "must not call changed route"); err != nil {
		t.Fatal(err)
	}
	deadline := time.Now().Add(5 * time.Second)
	for a.Runtime.HasActive() && time.Now().Before(deadline) {
		time.Sleep(time.Millisecond)
	}
	detail, err = a.Store.Detail(t.Context(), session.ID)
	if err != nil || detail.Session.Status != engine.Failed || requests.Load() != 2 {
		t.Fatal("stale route silently reused a provider", detail.Session.Status, err)
	}
	if err = a.Service.Connections.Delete(t.Context(), saved.ID, disabled.Revision); err != nil {
		t.Fatal(err)
	}
}
