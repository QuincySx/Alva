// INPUT: Authenticated Hub management HTTP, configured live local model route and embedded Gateway
// OUTPUT: Explicit no-start status, separate start consent, one-time secret and shared owner shutdown regressions
// POS: [L7 Hub product] Management endpoints cannot import provider credentials or call a model by merely starting/listing.
package server

import (
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"

	"github.com/QuincySx/Alva/apps/gateway"
	"github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/integration/llm/provider"
)

func TestHTTPEmbeddedGatewayManagementDoesNotInvokeModels(t *testing.T) {
	var calls atomic.Int32
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { calls.Add(1); http.Error(w, "must not invoke", 500) }))
	defer upstream.Close()
	f := newFixture(t, bootstrap.Config{OpenStores: bootstrap.LocalStores, OpenMemory: bootstrap.LocalMemory, OpenBindings: bootstrap.LocalBindings, ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: upstream.URL, Model: "explicit"}})
	status := body(t, f.request(t, "GET", "/v1/gateway", nil, nil), 200)
	if strings.Contains(string(status), `"running":true`) || calls.Load() != 0 {
		t.Fatal("cold management started work")
	}
	body(t, f.request(t, "POST", "/v1/gateway", []byte(`{"model_usage_approved":false}`), nil), 409)
	var credentials gateway.Credentials
	if json.Unmarshal(body(t, f.request(t, "POST", "/v1/gateway", []byte(`{"model_usage_approved":true}`), nil), 200), &credentials) != nil || credentials.Token == "" {
		t.Fatal("explicit start credentials absent")
	}
	status = body(t, f.request(t, "GET", "/v1/gateway", nil, nil), 200)
	if strings.Contains(string(status), credentials.Token) {
		t.Fatal("status returned bearer")
	}
	r, err := http.NewRequest("GET", credentials.Address+"/v1/models", nil)
	if err != nil {
		t.Fatal(err)
	}
	r.Header.Set("Authorization", "Bearer "+credentials.Token)
	response, err := f.http.Client().Do(r)
	if err != nil {
		t.Fatal(err)
	}
	data, _ := io.ReadAll(response.Body)
	response.Body.Close()
	if response.StatusCode != 200 || !strings.Contains(string(data), "alva") {
		t.Fatal("actual embedded models route absent")
	}
	body(t, f.request(t, "DELETE", "/v1/gateway", nil, nil), 200)
	if f.app.Service.Gateway.Snapshot().Running || calls.Load() != 0 || f.app.Runtime.HasActive() {
		t.Fatal("management invoked model or retained runtime")
	}
}
