// INPUT: Authenticated loopback routes and real committed temporary connection revisions.
// OUTPUT: HTTP route/fallback admission uses shared product policy without executing a model.
// POS: [L7 Hub HTTP testing] Transport validation preserves exact account approval and rejects unavailable targets.
package server

import (
	"encoding/json"
	"errors"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/bootstrap"
)

func TestHTTPRoutingUsesCommittedRoutesAndExplicitFallbackApproval(t *testing.T) {
	f := newFixture(t, bootstrap.Config{OpenStores: bootstrap.LocalStores, OpenMemory: bootstrap.LocalMemory, OpenBindings: bootstrap.LocalBindings, Demo: true})
	session := create(t, f, "route policy")
	prefix := "/v1/sessions/" + session.ID
	data, _ := json.Marshal(map[string]any{"targets": []hub.Target{f.app.Service.DefaultTarget}})
	body(t, f.request(t, "PUT", prefix+"/fallback", data, nil), 409)
	approved := f.app.Service.DefaultTarget
	approved.AutomaticAuthorized = true
	data, _ = json.Marshal(map[string]any{"targets": []hub.Target{approved}})
	body(t, f.request(t, "PUT", prefix+"/fallback", data, nil), 200)
	entry, err := f.app.Service.Connections.Save(t.Context(), hub.ConnectionUpdate{Name: "dynamic", Protocol: "openai_chat", Endpoint: "http://127.0.0.1:1/v1/chat/completions", Auth: "none", Enabled: true, Models: []hub.ConnectionModel{{ID: "model", Enabled: true}}})
	if err != nil {
		t.Fatal(err)
	}
	target := entry.Target(entry.Models[0])
	data, _ = json.Marshal(map[string]any{"target": target, "reason": "current committed connection"})
	body(t, f.request(t, "POST", prefix+"/branch", data, nil), 200)
	if _, err = f.app.Service.Connections.SetEnabled(t.Context(), entry.ID, entry.Revision, false); err != nil {
		t.Fatal(err)
	}
	body(t, f.request(t, "POST", prefix+"/branch", data, nil), 400)
	target.AutomaticAuthorized = true
	data, _ = json.Marshal(map[string]any{"targets": []hub.Target{target}})
	body(t, f.request(t, "PUT", prefix+"/fallback", data, nil), 400)
	detail, err := f.app.Store.Detail(t.Context(), session.ID)
	if err != nil {
		t.Fatal(err)
	}
	_, err = f.app.Service.Store.BranchTurn(t.Context(), session.ID, detail.Session.ActiveBranchID)
	if !errors.Is(err, hub.ErrNotFound) || f.app.Runtime.HasActive() {
		t.Fatal("route metadata operations executed", err)
	}
}
