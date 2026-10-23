// INPUT: Authenticated HTTP, shared product preferences and selected local SQLite storage.
// OUTPUT: Cold cross-observer persistence, strict transport and sanitized failure regressions.
// POS: [L7 transport tests] Verifies application preferences without creating a session or invoking a model.
package server

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/integration/llm/provider"
)

func preferenceBody(t *testing.T, response *http.Response) hub.Preferences {
	t.Helper()
	var value hub.Preferences
	if err := json.Unmarshal(body(t, response, http.StatusOK), &value); err != nil {
		t.Fatal(err)
	}
	return value
}

func preferencesRemainCold(t *testing.T, f *fixture) {
	t.Helper()
	sessions, err := f.app.Store.ListSessions(t.Context(), 10)
	if err != nil || len(sessions) != 0 || f.app.Runtime.HasActive() {
		t.Fatal("preferences created a session or started runtime", sessions, err)
	}
}

func TestHTTPPreferencesShareCommittedServiceAndSelectedSQLite(t *testing.T) {
	var calls atomic.Int32
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		calls.Add(1)
		http.Error(w, "preferences must not call a model", http.StatusInternalServerError)
	}))
	defer upstream.Close()
	f := newFixture(t, bootstrap.Config{
		OpenStores: bootstrap.LocalStores, ProviderKind: provider.OpenAIChatKind,
		Provider: provider.Config{Endpoint: upstream.URL, Model: "explicit"},
	})
	owner := f.app.Service.Preferences
	if owner == nil {
		t.Fatal("bootstrap did not assemble preferences")
	}
	initial := preferenceBody(t, f.request(t, http.MethodGet, "/v1/preferences", nil, nil))
	if initial != hub.DefaultPreferences() || owner.Snapshot() != initial {
		t.Fatal("HTTP and native observer did not share the initial snapshot", initial)
	}
	density := "compact"
	native, err := owner.Update(t.Context(), hub.PreferenceUpdate{ExpectedRevision: initial.Revision, Density: &density})
	if err != nil {
		t.Fatal(err)
	}
	if got := preferenceBody(t, f.request(t, http.MethodGet, "/v1/preferences", nil, nil)); got != native {
		t.Fatalf("HTTP missed shared observer update: got %+v want %+v", got, native)
	}
	updated := preferenceBody(t, f.request(t, http.MethodPatch, "/v1/preferences", []byte(`{"expected_revision":2,"theme":"dark","font_size":16}`), nil))
	if updated.Revision != 3 || updated.Theme != "dark" || updated.Density != density || updated.FontSize != 16 || owner.Snapshot() != updated {
		t.Fatal("HTTP update was not published to the shared observer", updated)
	}
	stored, err := f.app.Store.Preferences(t.Context())
	if err != nil || stored != updated {
		t.Fatal("HTTP update did not commit to the selected SQLite store", stored, err)
	}
	body(t, f.request(t, http.MethodPatch, "/v1/preferences", []byte(`{"expected_revision":1,"theme":"light"}`), nil), http.StatusConflict)
	if owner.Snapshot() != updated {
		t.Fatal("stale client rolled back the shared snapshot")
	}
	preferencesRemainCold(t, f)
	if calls.Load() != 0 {
		t.Fatal("preferences invoked the model")
	}
}

func TestHTTPPreferencesAuthenticateAndRejectStrictInvalidRequests(t *testing.T) {
	f := newFixture(t, bootstrap.Config{OpenStores: bootstrap.LocalStores, Demo: true})
	for _, method := range []string{http.MethodGet, http.MethodPatch} {
		for _, test := range []struct {
			headers map[string]string
			status  int
		}{
			{map[string]string{"Authorization": ""}, http.StatusUnauthorized},
			{map[string]string{"Origin": "https://foreign.example"}, http.StatusForbidden},
			{map[string]string{"Host": "foreign.example:8080"}, http.StatusForbidden},
		} {
			body(t, f.request(t, method, "/v1/preferences", []byte(`{"expected_revision":1,"theme":"dark"}`), test.headers), test.status)
		}
	}
	for _, raw := range []string{
		`null`, `{}`, `[]`, `{"expected_revision":1}`, `{"expected_revision":1,"theme":null}`,
		`{"expected_revision":1,"theme":"dark","extra":true}`,
		`{"expected_revision":1,"Theme":"dark"}`,
		`{"expected_revision":1,"theme":"dark","theme":"light"}`,
		`{"expected_revision":1,"theme":"\ud800"}`,
		`{"expected_revision":1,"theme":"dark"} {}`,
		`{"expected_revision":1,"theme":"unknown"}`,
		`{"expected_revision":1,"density":"dense"}`,
		`{"expected_revision":1,"font_size":15}`,
		`{"expected_revision":1,"font_size":"16"}`,
		`{"expected_revision":0,"theme":"dark"}`,
	} {
		t.Run(raw, func(t *testing.T) {
			body(t, f.request(t, http.MethodPatch, "/v1/preferences", []byte(raw), nil), http.StatusBadRequest)
		})
	}
	body(t, f.request(t, http.MethodPatch, "/v1/preferences", []byte(`{"expected_revision":1,"theme":"dark"}`), map[string]string{"Content-Type": "text/plain"}), http.StatusUnsupportedMediaType)
	body(t, f.request(t, http.MethodPatch, "/v1/preferences", []byte(`{"expected_revision":1,"theme":"`+strings.Repeat("x", 4<<10)+`"}`), nil), http.StatusRequestEntityTooLarge)
	body(t, f.request(t, http.MethodGet, "/v1/preferences?session=foreign", nil, nil), http.StatusBadRequest)
	body(t, f.request(t, http.MethodPatch, "/v1/preferences?path=/host/private", []byte(`{"expected_revision":1,"theme":"dark"}`), nil), http.StatusBadRequest)
	response := f.request(t, http.MethodPut, "/v1/preferences", nil, nil)
	if response.Header.Get("Allow") != "GET, PATCH" {
		t.Fatal("preferences method contract missing")
	}
	body(t, response, http.StatusMethodNotAllowed)
	if got := preferenceBody(t, f.request(t, http.MethodGet, "/v1/preferences", nil, nil)); got != hub.DefaultPreferences() || f.app.Service.Preferences.Snapshot() != got {
		t.Fatal("rejected request mutated preferences", got)
	}
	preferencesRemainCold(t, f)
}

type unavailablePreferences struct {
	bootstrap.ProductStorage
	failing atomic.Bool
}

func (s *unavailablePreferences) Preferences(ctx context.Context) (hub.Preferences, error) {
	if s.failing.Load() {
		return hub.Preferences{}, errors.New("sqlite failure at /private/operator/product.sqlite: backend-secret")
	}
	return s.ProductStorage.Preferences(ctx)
}

func (s *unavailablePreferences) UpdatePreferences(ctx context.Context, update hub.PreferenceUpdate) (hub.Preferences, error) {
	if s.failing.Load() {
		return hub.Preferences{}, errors.New("sqlite failure at /private/operator/product.sqlite: backend-secret")
	}
	return s.ProductStorage.UpdatePreferences(ctx, update)
}

func TestHTTPPreferencesBackendFailureIsSanitizedAndKeepsCommittedCache(t *testing.T) {
	var selected *unavailablePreferences
	f := newFixture(t, bootstrap.Config{
		Demo: true,
		OpenStores: func(ctx context.Context, directory string) (bootstrap.Stores, error) {
			stores, err := bootstrap.LocalStores(ctx, directory)
			if err != nil {
				return stores, err
			}
			selected = &unavailablePreferences{ProductStorage: stores.Product}
			stores.Product = selected
			return stores, nil
		},
	})
	committed := preferenceBody(t, f.request(t, http.MethodPatch, "/v1/preferences", []byte(`{"expected_revision":1,"theme":"dark"}`), nil))
	selected.failing.Store(true)
	for _, method := range []string{http.MethodGet, http.MethodPatch} {
		raw := body(t, f.request(t, method, "/v1/preferences", []byte(`{"expected_revision":2,"theme":"light"}`), nil), http.StatusInternalServerError)
		if string(raw) != "{\"error\":\"Internal Server Error\"}\n" {
			t.Fatalf("backend details escaped transport: %s", raw)
		}
		if f.app.Service.Preferences.Snapshot() != committed {
			t.Fatal("failed selected backend replaced the committed cache")
		}
	}
	selected.failing.Store(false)
	if got := preferenceBody(t, f.request(t, http.MethodGet, "/v1/preferences", nil, nil)); got != committed {
		t.Fatal("failed write changed the durable preference row", got)
	}
	preferencesRemainCold(t, f)
}
