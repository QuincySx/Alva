// INPUT: Documented model catalog response shapes, product connection SQLite and the shared connection policy
// OUTPUT: Catalog context-window parsing and saved route window resolution regressions
// POS: [L7 product service testing] A window is used only when a catalog states it explicitly or the operator entered it.
package product

import (
	"path/filepath"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	productsqlite "github.com/QuincySx/Alva/apps/hub/sqlite"
)

func TestCatalogContextWindowOnlyFromExplicitDocumentedFields(t *testing.T) {
	cases := []struct {
		protocol, body string
		want           int
	}{
		{"gemini", `{"models":[{"name":"models/gemini-pro","displayName":"Gemini","inputTokenLimit":1048576,"outputTokenLimit":8192}]}`, 1048576},
		{"openai_chat", `{"data":[{"id":"router/model","context_length":200000}]}`, 200000},
		{"openai_responses", `{"data":[{"id":"served","max_model_len":32768}]}`, 32768},
		{"openai_responses", `{"data":[{"id":"gpt-plain","object":"model","owned_by":"openai"}]}`, 0},
		{"anthropic", `{"data":[{"id":"claude-x","display_name":"Claude","context_length":200000}]}`, 200000},
		{"openai_chat", `{"data":[{"id":"tiny","context_length":512}]}`, 0},
		{"openai_chat", `{"data":[{"id":"fractional","context_length":8192.5}]}`, 0},
		{"openai_chat", `{"data":[{"id":"huge","context_length":999999999999}]}`, 0},
		{"gemini", `{"models":[{"name":"models/g","context_length":200000}]}`, 0},
	}
	for _, c := range cases {
		models, _, err := decodeConnectionCatalog(c.protocol, []byte(c.body))
		if err != nil || len(models) != 1 || models[0].CatalogContextWindow != c.want || models[0].ContextWindow != 0 {
			t.Fatalf("%s %s: %+v %v", c.protocol, c.body, models, err)
		}
	}
	for _, odd := range []string{`"200000"`, `"128k"`, `null`, `{"tokens":1}`} {
		models, _, err := decodeConnectionCatalog("openai_chat", []byte(`{"data":[{"id":"odd","context_length":`+odd+`}]}`))
		if err != nil || len(models) != 1 || models[0].CatalogContextWindow != 0 {
			t.Fatal("a non-integer context length was used or broke the catalog", odd, models, err)
		}
	}
}

func TestSavedRouteWindowPrefersTheCatalogStatement(t *testing.T) {
	ctx := t.Context()
	db, err := productsqlite.Open(ctx, filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	service, err := NewConnectionsService(ctx, db)
	if err != nil {
		t.Fatal(err)
	}
	defer service.Close(ctx)
	u := productConnectionFixture()
	u.Models = []hub.ConnectionModel{{ID: "catalog", Enabled: true, CatalogContextWindow: 200000, ContextWindow: 64000}, {ID: "operator", Enabled: true, ContextWindow: 64000}, {ID: "unknown", Enabled: true}}
	saved, err := service.Save(ctx, u)
	if err != nil {
		t.Fatal(err)
	}
	for i, want := range []int{200000, 64000, 0} {
		if got := service.ContextWindow(saved.Target(saved.Models[i])); got != want {
			t.Fatal("route window", saved.Models[i].ID, got, want)
		}
	}
	if service.ContextWindow(hub.Target{EngineID: "alva", ModelID: "deleted"}) != 0 {
		t.Fatal("an unknown route claimed a window")
	}
	u = productConnectionFixture()
	u.Models = []hub.ConnectionModel{{ID: "bad", Enabled: true, ContextWindow: 100}}
	if _, err := service.Save(ctx, u); err == nil {
		t.Fatal("an implausible window was saved")
	}
}
