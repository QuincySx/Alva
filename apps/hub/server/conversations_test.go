// INPUT: Authenticated HTTP requests against a real product service
// OUTPUT: Session rename, pin, soft-delete and stopped-reply continue route regressions
// POS: [L7 Hub product] The HTTP entry reaches management only through the narrow Conversations/Turns services
package server

import (
	"encoding/json"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	bootstrap "github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/integration/engine"
)

func TestSessionManagementRoutes(t *testing.T) {
	f := newFixture(t, bootstrap.Config{OpenStores: bootstrap.LocalStores, OpenMemory: bootstrap.LocalMemory, OpenBindings: bootstrap.LocalBindings, Demo: true})
	one := create(t, f, "first")
	two := create(t, f, "second")
	path := "/v1/sessions/" + one.ID

	var renamed hub.Session
	if err := json.Unmarshal(body(t, f.request(t, "PATCH", path, []byte(`{"title":"  重命名 ","pinned":true}`), nil), 200), &renamed); err != nil || renamed.Title != "重命名" || !renamed.Pinned {
		t.Fatal("rename+pin did not apply", renamed, err)
	}
	for _, bad := range []string{`{}`, `{"title":"   "}`, `{"title":"x","extra":1}`, `{"title":"a\u202eb"}`, `{"title":"nul\u0000"}`} {
		body(t, f.request(t, "PATCH", path, []byte(bad), nil), 400)
	}
	body(t, f.request(t, "PATCH", "/v1/sessions/hub-missing", []byte(`{"title":"x"}`), nil), 404)

	body(t, f.request(t, "DELETE", path, nil, nil), 200)
	body(t, f.request(t, "GET", path, nil, nil), 404)
	body(t, f.request(t, "PATCH", path, []byte(`{"title":"x"}`), nil), 404)
	var page struct{ Sessions []hub.Session }
	if err := json.Unmarshal(body(t, f.request(t, "GET", "/v1/sessions", nil, nil), 200), &page); err != nil || len(page.Sessions) != 1 || page.Sessions[0].ID != two.ID {
		t.Fatal("deleted session still listed", page, err)
	}
}

func TestSessionDeleteRefusesRunningAndContinueNeedsStoppedReply(t *testing.T) {
	f := newFixture(t, bootstrap.Config{OpenStores: bootstrap.LocalStores, OpenMemory: bootstrap.LocalMemory, OpenBindings: bootstrap.LocalBindings, Demo: true})
	s := create(t, f, "busy")
	body(t, f.request(t, "POST", "/v1/sessions/"+s.ID+"/continue", nil, nil), 409) // no turn yet: nothing to continue
	claim, err := f.app.Store.BeginTurn(t.Context(), s.ID, "running")
	if err != nil {
		t.Fatal(err)
	}
	body(t, f.request(t, "DELETE", "/v1/sessions/"+s.ID, nil, nil), 409) // running session refuses
	if err = f.app.Store.FinishTurn(t.Context(), claim.Turn.ID, engine.Cancelled); err != nil {
		t.Fatal(err)
	}
	raw := body(t, f.request(t, "POST", "/v1/sessions/"+s.ID+"/continue", nil, nil), 202)
	var started map[string]string
	if json.Unmarshal(raw, &started) != nil || started["turn_id"] == "" || started["execution_id"] == "" {
		t.Fatal("continue did not return the turn and execution identity", string(raw))
	}
}
