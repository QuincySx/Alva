// INPUT: Real temporary product/native stores, committed connection revisions and cold route changes.
// OUTPUT: Shared account-route admission, explicit fallback approval and retained-history regressions.
// POS: [L7 Hub product testing] Old history stays accessible while unavailable routes create no Turn or model request.
package product

import (
	"context"
	"errors"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
)

func TestProductRoutingRejectsStaleConnectionsButPreservesBranchHistory(t *testing.T) {
	s, requests := fixture(t)
	var err error
	s.Connections, err = NewConnectionsService(t.Context(), s.Store.(hub.ConnectionsStore))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = s.Connections.Close(context.Background()) })
	entry, err := s.Connections.Save(t.Context(), hub.ConnectionUpdate{Name: "account", Protocol: "openai_chat", Endpoint: "http://127.0.0.1:1/v1/chat/completions", Auth: "none", Enabled: true, Models: []hub.ConnectionModel{{ID: "saved", Enabled: true}}})
	if err != nil {
		t.Fatal(err)
	}
	session, err := s.Create(t.Context(), "route history", "")
	if err != nil {
		t.Fatal(err)
	}
	old := entry.Target(entry.Models[0])
	retained, err := s.SwitchTarget(t.Context(), session.ID, old, "explicit configured selection")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.SwitchTarget(t.Context(), session.ID, s.DefaultTarget, "return to configured route"); err != nil {
		t.Fatal(err)
	}
	if _, err = s.Connections.SetEnabled(t.Context(), entry.ID, entry.Revision, false); err != nil {
		t.Fatal(err)
	}
	if _, err = s.SwitchTarget(t.Context(), session.ID, old, "stale new selection"); !errors.Is(err, ErrRouteUnavailable) {
		t.Fatal("stale route accepted", err)
	}
	approved := old
	approved.AutomaticAuthorized = true
	if err = s.SetFallback(t.Context(), session.ID, []hub.Target{approved}); !errors.Is(err, ErrRouteUnavailable) {
		t.Fatal("stale fallback accepted", err)
	}
	branch, err := s.SelectBranch(t.Context(), session.ID, retained.ID, "inspect retained branch")
	if err != nil || branch.ID != retained.ID {
		t.Fatal("history was lost after connection disabled", branch, err)
	}
	if _, err = s.Submit(t.Context(), session.ID, "must not create an unavailable-route Turn", nil); !errors.Is(err, ErrRouteUnavailable) {
		t.Fatal("unavailable history branch admitted execution", err)
	}
	_, err = s.Store.BranchTurn(t.Context(), session.ID, retained.ID)
	if !errors.Is(err, hub.ErrNotFound) || len(requests) != 0 || s.Runtime.HasActive() {
		t.Fatal("cold route operations executed or created a Turn", err)
	}
	other, err := s.Create(t.Context(), "foreign branch", "")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.SelectBranch(t.Context(), other.ID, retained.ID, "foreign branch"); !errors.Is(err, hub.ErrNotFound) {
		t.Fatal("foreign history branch selected", err)
	}
}

func TestProductFallbackRequiresExactRouteAndExplicitApproval(t *testing.T) {
	s, _ := fixture(t)
	session, err := s.Create(t.Context(), "fallback", "")
	if err != nil {
		t.Fatal(err)
	}
	approved := s.DefaultTarget
	if err = s.SetFallback(t.Context(), session.ID, []hub.Target{approved}); err == nil {
		t.Fatal("fallback approved implicitly")
	}
	approved.AutomaticAuthorized = true
	if err = s.SetFallback(t.Context(), session.ID, []hub.Target{approved}); err != nil {
		t.Fatal(err)
	}
	for _, field := range []string{"account", "billing", "capability"} {
		forged := approved
		switch field {
		case "account":
			forged.AccountSource = "other"
		case "billing":
			forged.BillingRoute = "other"
		case "capability":
			forged.RequiredCapabilities = []string{"unknown"}
		}
		if err = s.SetFallback(t.Context(), session.ID, []hub.Target{forged}); !errors.Is(err, ErrRouteUnavailable) {
			t.Fatal("fabricated route accepted", field, err)
		}
	}
	if err = s.SetFallback(t.Context(), session.ID, []hub.Target{approved, approved}); err == nil {
		t.Fatal("duplicate fallback accepted")
	}
	detail, err := s.Store.Detail(t.Context(), session.ID)
	if err != nil || len(detail.Fallback) != 1 || !detail.Fallback[0].AutomaticAuthorized {
		t.Fatal("rejected edit changed committed policy", detail.Fallback, err)
	}
	if err = s.SetFallback(t.Context(), session.ID, nil); err != nil {
		t.Fatal(err)
	}
}
