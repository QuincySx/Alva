// INPUT: Authenticated local HTTP, saved workspace and explicit project convention config.
// OUTPUT: Cold opt-in/disable, strict path/body rejection and unrelated-setting isolation.
// POS: [L7 transport] Exercises the shared product control without starting a model or exposing host paths.
package server

import (
	"encoding/json"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/bootstrap"
)

func TestHTTPCompatibilityConfigIsColdAndWorkspaceConfined(t *testing.T) {
	f := newFixture(t, bootstrap.Config{OpenStores: bootstrap.LocalStores, Demo: true})
	s := create(t, f, "compatibility HTTP")
	if err := os.WriteFile(filepath.Join(s.Workspace, "compat.json"), []byte(`{"version":1,"rules":["agents"]}`), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(s.Workspace, "AGENTS.md"), []byte("HTTP-RULE"), 0600); err != nil {
		t.Fatal(err)
	}
	path := "/v1/sessions/" + s.ID + "/settings/compatibility"
	body(t, f.request(t, http.MethodPut, path, []byte(`{"file":"compat.json"}`), nil), 200)
	var settings hub.Settings
	if err := json.Unmarshal(body(t, f.request(t, http.MethodGet, "/v1/sessions/"+s.ID+"/settings", nil, nil), 200), &settings); err != nil || settings.CompatibilityFile != "compat.json" {
		t.Fatal(settings, err)
	}
	for _, raw := range []string{`null`, `{}`, `{"file":null}`, `{"File":"compat.json"}`, `{"file":"compat.json","file":"different.json"}`} {
		body(t, f.request(t, http.MethodPut, path, []byte(raw), nil), 400)
	}
	for _, raw := range []string{`{"file":"/private/host/secret"}`, `{"file":"../outside.json"}`, `{"file":"not-there.json"}`} {
		data := body(t, f.request(t, http.MethodPut, path, []byte(raw), nil), 409)
		if strings.Contains(string(data), s.Workspace) || strings.Contains(string(data), "/private/host") {
			t.Fatal("host path exposed")
		}
	}
	body(t, f.request(t, http.MethodPut, path, []byte(`{"file":""}`), nil), 200)
	settings, err := f.app.Store.Settings(t.Context(), s.ID)
	if err != nil || settings.CompatibilityFile != "" {
		t.Fatal(settings, err)
	}
	history, err := f.app.Store.History(t.Context(), s.ID, 0, 16)
	if err != nil || len(history) != 0 || f.app.Runtime.HasActive() {
		t.Fatal("configuration started execution", history, err)
	}
}
