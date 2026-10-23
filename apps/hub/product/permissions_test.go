// INPUT: Shared product policy setter and real native/product SQLite fixtures.
// OUTPUT: Explicit Full consent, cold saves and running/missing/external scope rejection.
// POS: [L7 service tests] Checks authority changes separately from model execution.
package product

import (
	"errors"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

func TestPermissionPolicyIsExplicitColdAndFrozen(t *testing.T) {
	s, requests := fixture(t)
	p, err := s.Create(t.Context(), "policy", "")
	if err != nil {
		t.Fatal(err)
	}
	if err = s.SetPermissionPolicy(t.Context(), p.ID, hub.PermissionFull, false); err == nil {
		t.Fatal("Full accepted without scope confirmation")
	}
	for _, policy := range []hub.PermissionPolicy{hub.PermissionAuto, hub.PermissionFull, hub.PermissionAsk} {
		if err = s.SetPermissionPolicy(t.Context(), p.ID, policy, policy == hub.PermissionFull); err != nil {
			t.Fatal(err)
		}
		stored, err := s.Store.Settings(t.Context(), p.ID)
		if err != nil || stored.PermissionPolicy.Effective() != policy || stored.PlanMode {
			t.Fatal("committed policy mismatch", stored.PermissionPolicy, err)
		}
	}
	if len(requests) != 0 || s.Runtime.HasActive() {
		t.Fatal("saving policy executed a model")
	}
	if err = s.SetPermissionPolicy(t.Context(), "missing", hub.PermissionAuto, false); !errors.Is(err, hub.ErrNotFound) {
		t.Fatal(err)
	}
	if _, err = s.Store.BeginTurn(t.Context(), p.ID, "already admitted"); err != nil {
		t.Fatal(err)
	}
	if err = s.SetPermissionPolicy(t.Context(), p.ID, hub.PermissionFull, true); err == nil {
		t.Fatal("active recipe changed")
	}
	other, err := s.Create(t.Context(), "external", "")
	if err != nil {
		t.Fatal(err)
	}
	target := s.DefaultTarget
	target.EngineID = "external"
	if _, err = s.Store.Switch(t.Context(), other.ID, target, "explicit branch"); err != nil {
		t.Fatal(err)
	}
	if err = s.SetPermissionPolicy(t.Context(), other.ID, hub.PermissionAuto, false); !errors.Is(err, engine.ErrUnsupported) {
		t.Fatal("external engine borrowed native policy", err)
	}
}
