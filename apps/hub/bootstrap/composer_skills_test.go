// INPUT: Explicit pre-session Skill preparation, two real managed sources and a local HTTP model fixture.
// OUTPUT: Selected-only real model catalog/tool delivery with the full project Skill setting left disabled.
// POS: [L7 assembly testing] Exercises selection through product, SQLite, Runtime and the existing native Skill plugin.
package bootstrap

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"

	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/llm/provider"
)

func TestComposerSelectedSkillUsesExistingPluginWithoutEnablingWholeCatalog(t *testing.T) {
	_, workspace, _ := companionSkillFixture(t)
	var requests atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var body map[string]any
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil {
			t.Error(err)
		}
		raw, _ := json.Marshal(body)
		if strings.Contains(string(raw), "Private metadata") || strings.Contains(string(raw), "Private instructions.") {
			t.Error("unselected sibling Skill entered actual model request")
		}
		if !strings.Contains(string(raw), "Shared metadata") || !strings.Contains(string(raw), `"name":"skill"`) {
			t.Error("selected native Skill capability missing from actual model request")
		}
		if requests.Add(1) == 1 {
			waitReply(w, "skill", `{"name":"shared-guide","operation":"load"}`, "")
			return
		}
		if !strings.Contains(string(raw), "Shared instructions.") {
			t.Error("actual Skill tool did not deliver selected instructions")
		}
		waitReply(w, "", "", "selected source used")
	}))
	defer server.Close()
	app, err := Open(t.Context(), Config{OpenStores: testStores, OpenMemory: LocalMemory, OpenBindings: LocalBindings, DataDirectory: filepath.Join(t.TempDir(), "private"), ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: server.URL, Model: "selected-skill-fixture"}})
	if err != nil {
		t.Fatal(err)
	}
	defer closeApp(t, app)
	prepared, err := app.Service.PrepareComposerHomeSelection(t.Context(), workspace, product.ComposerSelectionRequest{EntryID: "skill:shared-guide", Arguments: map[string]string{"request": "use the chosen guide"}})
	if err != nil {
		t.Fatal(err)
	}
	session, err := app.Service.Create(t.Context(), "selected first Send", workspace)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = app.Service.SubmitComposerDraft(t.Context(), session.ID, "use the chosen guide", []product.ComposerPreparation{prepared}, nil); err != nil {
		t.Fatal(err)
	}
	awaitTerminal(t, app, session.ID)
	settings, err := app.Store.Settings(t.Context(), session.ID)
	if err != nil || settings.SkillsEnabled || requests.Load() != 2 {
		t.Fatal("selected Skill changed whole-catalog settings or did not execute", settings.SkillsEnabled, requests.Load(), err)
	}
}
