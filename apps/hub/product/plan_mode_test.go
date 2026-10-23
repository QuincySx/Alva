// INPUT: Shared product service, real product/native SQLite ports and explicit native or external branches.
// OUTPUT: Cold next-turn plan changes, missing/native ownership checks and running recipe freeze regressions.
// POS: [L7 product tests] Tests configuration admission separately from bootstrap's real tool enforcement.
package product

import (
	"errors"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

func TestSetPlanModeIsColdAndNativeOnly(t *testing.T) {
	s, requests := fixture(t)
	p, err := s.Create(t.Context(), "plan choice", "")
	if err != nil {
		t.Fatal(err)
	}
	for _, enabled := range []bool{true, false, true} {
		if err = s.SetPlanMode(t.Context(), p.ID, enabled); err != nil {
			t.Fatal(err)
		}
		value, err := s.Store.Settings(t.Context(), p.ID)
		if err != nil || value.PlanMode != enabled {
			t.Fatal("actual settings do not reflect committed choice", value.PlanMode, err)
		}
	}
	if len(requests) != 0 || s.Runtime.SessionActive(p.ID) {
		t.Fatal("saving plan choice executed an Agent")
	}
	if err = s.SetPlanMode(t.Context(), "missing", true); !errors.Is(err, hub.ErrNotFound) {
		t.Fatal("missing saved-session scope accepted", err)
	}
	target := s.DefaultTarget
	target.EngineID = "external"
	if _, err = s.Store.Switch(t.Context(), p.ID, target, "explicit external branch"); err != nil {
		t.Fatal(err)
	}
	if err = s.SetPlanMode(t.Context(), p.ID, true); !errors.Is(err, engine.ErrUnsupported) {
		t.Fatal("external engine borrowed a native plan profile", err)
	}
}

func TestSetPlanModeRejectsRunningTurn(t *testing.T) {
	s, _ := fixture(t)
	p, err := s.Create(t.Context(), "frozen plan", "")
	if err != nil {
		t.Fatal(err)
	}
	if err = s.SetPlanMode(t.Context(), p.ID, true); err != nil {
		t.Fatal(err)
	}
	if _, err = s.Store.BeginTurn(t.Context(), p.ID, "already admitted"); err != nil {
		t.Fatal(err)
	}
	if err = s.SetPlanMode(t.Context(), p.ID, false); err == nil {
		t.Fatal("running next-turn recipe changed")
	}
	value, err := s.Store.Settings(t.Context(), p.ID)
	if err != nil || !value.PlanMode {
		t.Fatal("rejected choice changed stored mode", err)
	}
}
