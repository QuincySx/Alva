// INPUT: Authenticated real HTTP, durable product store and the shared native demo runtime
// OUTPUT: Paused-first optimistic schedule CRUD, billing confirmation and cold cross-owner denial
// POS: [L7 Hub product] Transport acceptance exposes no tick override, prompt bypass, replay or invented scheduled result
package server

import (
	"encoding/json"
	"fmt"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	bootstrap "github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/integration/engine"
)

func TestHTTPAutomationCRUDPinnedAndPausedNoImplicitExecution(t *testing.T) {
	f := newFixture(t, bootstrap.Config{Demo: true})
	p, other := create(t, f, "schedule"), create(t, f, "other")
	base := "/v1/sessions/" + p.ID + "/automations"
	var a hub.Automation
	if err := json.Unmarshal(body(t, f.request(t, "POST", base, []byte(`{"title":"review","prompt":"inspect","interval_minutes":5}`), nil), 201), &a); err != nil || a.Enabled || a.NextRunAt != 0 || a.ID == "" {
		t.Fatal("not paused", a, err)
	}
	path := base + "/" + a.ID
	body(t, f.request(t, "GET", strings.Replace(path, p.ID, other.ID, 1), nil, nil), 404)
	body(t, f.request(t, "GET", base, nil, nil), 200)
	body(t, f.request(t, "PUT", path+"/enabled", []byte(fmt.Sprintf(`{"revision":%d,"enabled":true}`, a.Revision)), nil), 409)
	body(t, f.request(t, "PUT", path+"/enabled", []byte(fmt.Sprintf(`{"revision":%d,"enabled":null,"account_and_billing_approved":true}`, a.Revision)), nil), 400)
	body(t, f.request(t, "PUT", path+"/enabled", []byte(fmt.Sprintf(`{"revision":%d,"enabled":true,"enabled":false,"account_and_billing_approved":true}`, a.Revision)), nil), 400)
	if err := json.Unmarshal(body(t, f.request(t, "PUT", path+"/enabled", []byte(fmt.Sprintf(`{"revision":%d,"enabled":true,"account_and_billing_approved":true}`, a.Revision)), nil), 200), &a); err != nil || !a.Enabled || a.NextRunAt == 0 {
		t.Fatal("not enabled", a, err)
	}
	body(t, f.request(t, "DELETE", path, []byte(`{"revision":1}`), nil), 409)
	body(t, f.request(t, "POST", path+"/tick", []byte(`{}`), nil), 404)
	body(t, f.request(t, "PUT", path, []byte(fmt.Sprintf(`{"revision":%d,"draft":{"title":"edit","prompt":"new","interval_minutes":10}}`, a.Revision)), nil), 200)
	a = hub.Automation{}
	if err := json.Unmarshal(body(t, f.request(t, "GET", path, nil, nil), 200), &a); err != nil || a.Enabled || a.NextRunAt != 0 || a.Prompt != "new" {
		t.Fatal("edit did not require review", a, err)
	}
	body(t, f.request(t, "DELETE", path, []byte(fmt.Sprintf(`{"revision":%d}`, a.Revision)), nil), 200)
	body(t, f.request(t, "GET", path, nil, nil), 404)
	body(t, f.request(t, "GET", "/v1/sessions/"+p.ID, nil, nil), 200)
	if history, err := f.app.Store.History(t.Context(), p.ID, 0, 32); err != nil || len(history) != 0 || f.app.Runtime.HasActive() {
		t.Fatal("CRUD ran/deleted conversation", history, err)
	}
}

func TestHTTPHandoffReadsOnlyExactOwnerSnapshot(t *testing.T) {
	f := newFixture(t, bootstrap.Config{Demo: true})
	p, other := create(t, f, "handoff"), create(t, f, "other")
	claim, err := f.app.Store.BeginTurn(t.Context(), p.ID, "original")
	if err != nil {
		t.Fatal(err)
	}
	if err = f.app.Store.Record(t.Context(), claim.Attempt.ID, engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded}, false); err != nil {
		t.Fatal(err)
	}
	if err = f.app.Store.FinishTurn(t.Context(), claim.Turn.ID, engine.Succeeded); err != nil {
		t.Fatal(err)
	}
	target := f.app.Service.DefaultTarget
	target.ModelID = "second-explicit-test-route"
	if _, err = f.app.Store.Switch(t.Context(), p.ID, target, "actual saved handoff test"); err != nil {
		t.Fatal(err)
	}
	claim, err = f.app.Store.BeginTurn(t.Context(), p.ID, "next")
	if err != nil {
		t.Fatal(err)
	}
	if claim.Handoff == nil {
		t.Fatal("normal claim lacks actual snapshot")
	}
	path := "/v1/sessions/" + p.ID + "/handoffs/" + claim.Handoff.ID
	var h hub.Handoff
	if err = json.Unmarshal(body(t, f.request(t, "GET", path, nil, nil), 200), &h); err != nil || h.ID != claim.Handoff.ID || h.SessionID != p.ID {
		t.Fatal("not exact snapshot", h, err)
	}
	body(t, f.request(t, "GET", strings.Replace(path, p.ID, other.ID, 1), nil, nil), 404)
	body(t, f.request(t, "PUT", path, []byte(`{}`), nil), 405)
	if f.app.Runtime.HasActive() {
		t.Fatal("snapshot read restored/executed runtime")
	}
}
