// INPUT: Temporary selected SQLite, loopback HTTP fixtures and explicit connection draft snapshots.
// OUTPUT: Protocol/auth correctness, bounded catalogs, source pinning, redaction and real cancel/Close regressions.
// POS: [L7 product testing] Uses no real accounts or generation; every network action is explicitly invoked.
package product

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	productsqlite "github.com/QuincySx/Alva/apps/hub/sqlite"
	"github.com/QuincySx/Alva/platform/storage"
)

func catalogServiceFixture(t *testing.T) *ConnectionsService {
	t.Helper()
	db, err := productsqlite.Open(t.Context(), filepath.Join(t.TempDir(), "catalog"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = db.Close() })
	service, err := NewConnectionsService(t.Context(), db)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = service.Close(context.Background()) })
	return service
}
func catalogDraft(protocol, address string) hub.ConnectionUpdate {
	path := "/v1/responses"
	switch protocol {
	case "openai_chat":
		path = "/v1/chat/completions"
	case "anthropic":
		path = "/v1/messages"
	case "gemini":
		path = "/v1beta/models/{model}:generateContent"
	}
	catalog := "/v1/models"
	if protocol == "gemini" {
		catalog = "/v1beta/models"
	}
	return hub.ConnectionUpdate{Name: "Fixture", Protocol: protocol, Endpoint: address + path, CatalogURL: address + catalog, Auth: "api_key", APIKey: "private-catalog-key", ReplaceHeaders: true, Headers: map[string]string{"X-Private-Fixture": "private-header-value"}}
}
func TestConnectionCatalogProtocolsUseOnlyExplicitGETAndNativeAuth(t *testing.T) {
	for _, protocol := range []string{"openai_chat", "openai_responses", "anthropic", "gemini"} {
		t.Run(protocol, func(t *testing.T) {
			var calls atomic.Int32
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				calls.Add(1)
				if r.Method != "GET" || r.ContentLength > 0 || r.Header.Get("X-Private-Fixture") != "private-header-value" {
					t.Error("wrong method/body/custom header")
				}
				expectedPath := "/v1/models"
				switch protocol {
				case "anthropic":
					if r.Header.Get("X-Api-Key") != "private-catalog-key" || r.Header.Get("Anthropic-Version") != "2023-06-01" || r.Header.Get("Authorization") != "" || r.URL.Query().Get("limit") != "256" {
						t.Error("wrong Anthropic auth/query")
					}
				case "gemini":
					expectedPath = "/v1beta/models"
					if r.Header.Get("X-Goog-Api-Key") != "private-catalog-key" || r.URL.Query().Get("key") != "" || r.URL.Query().Get("pageSize") != "256" {
						t.Error("wrong Gemini auth/query")
					}
				default:
					if r.Header.Get("Authorization") != "Bearer private-catalog-key" || r.URL.RawQuery != "" {
						t.Error("wrong OpenAI auth/query")
					}
				}
				if r.URL.Path != expectedPath {
					t.Error("request left exact configured catalog path")
				}
				w.Header().Set("Content-Type", "application/json")
				if protocol == "gemini" {
					fmt.Fprint(w, `{"models":[{"name":"models/exact-ID","displayName":"Readable"}]}`)
				} else {
					fmt.Fprint(w, `{"data":[{"id":"exact-ID","display_name":"Readable"}]}`)
				}
			}))
			defer server.Close()
			s := catalogServiceFixture(t)
			draft := catalogDraft(protocol, server.URL)
			saved, err := s.Save(t.Context(), draft)
			if err != nil || calls.Load() != 0 {
				t.Fatal("Save performed network or failed", err)
			}
			stored, readErr := s.Read(t.Context())
			if readErr != nil || len(stored) != 1 || stored[0].CatalogURL != draft.CatalogURL {
				t.Fatal("explicit catalog URL did not survive durable read", readErr)
			}
			edit := connectionEdit(saved)
			result, err := s.FetchModels(t.Context(), edit)
			if err != nil || len(result.Models) != 1 || result.Models[0].ID != "exact-ID" || result.Models[0].Enabled || result.Models[0].ImageCapable {
				t.Fatalf("catalog candidates not exact and disabled: %+v %v", result, err)
			}
			receipt, err := s.TestConnection(t.Context(), edit)
			if err != nil || receipt.Scope != "model_catalog_access" || receipt.ModelCount != 1 || calls.Load() != 2 || len(s.Targets()) != 0 {
				t.Fatal("test scope or network count wrong", err)
			}
			encoded, _ := json.Marshal(result)
			if strings.Contains(string(encoded), "private-") {
				t.Fatal("catalog exposed credentials")
			}
		})
	}
}
func TestConnectionCatalogTruthfulFailureEmptyAndRedaction(t *testing.T) {
	for _, tc := range []struct {
		name, body  string
		code        int
		unavailable bool
		empty       bool
	}{
		{"unauthorized", "private-catalog-key private-header-value", 401, false, false},
		{"forbidden", "private-catalog-key", 403, false, false},
		{"unsupported", "private-header-value", 404, true, false},
		{"method-unsupported", "", 405, true, false},
		{"empty", `{"data":[]}`, 200, false, true},
		{"missing-array", `{}`, 200, false, false},
		{"null-array", `{"data":null}`, 200, false, false},
		{"malformed", `{"data":[`, 200, false, false},
		{"secret-echo", `{"data":[{"id":"private-catalog-key"}]}`, 200, false, false},
		{"header-echo", `{"data":[{"id":"private-header-value"}]}`, 200, false, false},
		{"oversized", strings.Repeat("x", (1<<20)+1), 200, false, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			var calls atomic.Int32
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				calls.Add(1)
				w.WriteHeader(tc.code)
				fmt.Fprint(w, tc.body)
			}))
			defer server.Close()
			s := catalogServiceFixture(t)
			result, err := s.FetchModels(t.Context(), catalogDraft("openai_chat", server.URL))
			if tc.empty {
				if err != nil || len(result.Models) != 0 {
					t.Fatal("empty success confused with failure", err)
				}
			} else if err == nil || strings.Contains(err.Error(), "private-") || (errors.Is(err, ErrConnectionCatalogUnavailable) != tc.unavailable) {
				t.Fatal("failure not sanitized/typed", err)
			}
			if calls.Load() != 1 {
				t.Fatal("request retried")
			}
		})
	}
}
func TestConnectionCatalogNoRedirectAmbientProxyOrCrossOriginSecrets(t *testing.T) {
	var escaped atomic.Int32
	forbidden := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { escaped.Add(1); fmt.Fprint(w, `{"data":[]}`) }))
	defer forbidden.Close()
	origin := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, forbidden.URL+"/models", http.StatusTemporaryRedirect)
	}))
	defer origin.Close()
	s := catalogServiceFixture(t)
	draft := catalogDraft("openai_responses", origin.URL)
	if _, err := s.FetchModels(t.Context(), draft); err == nil || !strings.Contains(err.Error(), "redirect refused") {
		t.Fatal("redirect accepted", err)
	}
	draft.CatalogURL = forbidden.URL + "/models"
	if _, err := s.FetchModels(t.Context(), draft); err == nil {
		t.Fatal("cross-origin catalog accepted")
	}
	draft.CatalogURL = ""
	if _, err := s.FetchModels(t.Context(), draft); !errors.Is(err, ErrConnectionCatalogUnavailable) {
		t.Fatal("missing catalog guessed", err)
	}
	if escaped.Load() != 0 {
		t.Fatal("credentials crossed explicit origin")
	}
	// Proxy is explicitly nil on the owned transport; hostile ambient settings cannot reroute a request.
	t.Setenv("HTTP_PROXY", forbidden.URL)
	t.Setenv("HTTPS_PROXY", forbidden.URL)
	t.Setenv("ALL_PROXY", forbidden.URL)
	good := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { fmt.Fprint(w, `{"data":[]}`) }))
	defer good.Close()
	if _, err := s.FetchModels(t.Context(), catalogDraft("openai_chat", good.URL)); err != nil || escaped.Load() != 0 {
		t.Fatal("ambient proxy used", err)
	}
}
func TestConnectionCatalogPaginationIsBoundedAndPinned(t *testing.T) {
	for _, protocol := range []string{"anthropic", "gemini"} {
		t.Run(protocol, func(t *testing.T) {
			var calls atomic.Int32
			var endless atomic.Bool
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				n := calls.Add(1)
				if protocol == "anthropic" {
					if n%2 == 0 && !endless.Load() && r.URL.Query().Get("after_id") != "cursor-1" {
						t.Error("wrong Anthropic cursor")
					}
					if endless.Load() {
						fmt.Fprintf(w, `{"data":[{"id":"model-%d"}],"has_more":true,"last_id":"cursor-%d"}`, n, n)
					} else if n%2 == 1 {
						fmt.Fprint(w, `{"data":[{"id":"model-A"}],"has_more":true,"last_id":"cursor-1"}`)
					} else {
						fmt.Fprint(w, `{"data":[{"id":"model-A"},{"id":"model-B"}],"has_more":false}`)
					}
				} else {
					if n%2 == 0 && !endless.Load() && r.URL.Query().Get("pageToken") != "opaque+/=&token" {
						t.Error("wrong Gemini cursor")
					}
					if endless.Load() {
						fmt.Fprintf(w, `{"models":[{"name":"models/model-%d"}],"nextPageToken":"cursor-%d"}`, n, n)
					} else if n%2 == 1 {
						fmt.Fprint(w, `{"models":[{"name":"models/model-A"}],"nextPageToken":"opaque+/=&token"}`)
					} else {
						fmt.Fprint(w, `{"models":[{"name":"models/model-A"},{"name":"models/model-B"}]}`)
					}
				}
			}))
			defer server.Close()
			s := catalogServiceFixture(t)
			result, err := s.FetchModels(t.Context(), catalogDraft(protocol, server.URL))
			if err != nil || len(result.Models) != 2 || calls.Load() != 2 {
				t.Fatal("pagination/dedup failed", err)
			}
			calls.Store(0)
			endless.Store(true)
			if _, err = s.FetchModels(t.Context(), catalogDraft(protocol, server.URL)); err == nil || !strings.Contains(err.Error(), "eight pages") || calls.Load() != 8 {
				t.Fatal("pagination not bounded", err)
			}
		})
	}
}
func TestConnectionCatalogSavedSecretPinAndActualCancellationClose(t *testing.T) {
	s := catalogServiceFixture(t)
	entered := make(chan struct{}, 4)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { entered <- struct{}{}; <-r.Context().Done() }))
	defer server.Close()
	saved, err := s.Save(t.Context(), catalogDraft("openai_chat", server.URL))
	if err != nil {
		t.Fatal(err)
	}
	edit := connectionEdit(saved)
	edit.Endpoint = server.URL + "/different-source"
	if _, err = s.FetchModels(t.Context(), edit); err == nil || !strings.Contains(err.Error(), "source changed") {
		t.Fatal("edited source borrowed saved secret", err)
	}
	edit = connectionEdit(saved)
	ctx, cancel := context.WithTimeout(t.Context(), 100*time.Millisecond)
	_, err = s.FetchModels(ctx, edit)
	cancel()
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal("timeout not distinct", err)
	}
	select {
	case <-entered:
	case <-time.After(time.Second):
		t.Fatal("request did not reach explicit catalog")
	}
	result := make(chan error, 1)
	go func() { _, err := s.FetchModels(t.Context(), edit); result <- err }()
	select {
	case <-entered:
	case <-time.After(time.Second):
		t.Fatal("request did not reach explicit catalog")
	}
	if err = s.Close(t.Context()); err != nil {
		t.Fatal("Close did not join request", err)
	}
	if err = <-result; !errors.Is(err, context.Canceled) {
		t.Fatal("Close did not cancel owned request", err)
	}
	if _, err = s.FetchModels(t.Context(), edit); !errors.Is(err, storage.ErrClosed) {
		t.Fatal("closed service admitted request", err)
	}
}
