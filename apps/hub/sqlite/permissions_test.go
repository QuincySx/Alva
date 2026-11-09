// INPUT: Real SQLite reopen, explicit policy transitions and ordinary settings imports.
// OUTPUT: Durable next-Turn policies, no import elevation and Plan exit without latent Full restoration.
// POS: [L7 storage tests] Permission changes share the session transaction and preserve independent capability consent.
package sqlite

import (
	"path/filepath"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
)

func TestPermissionPolicyPersistsAndImportCannotGrant(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "private")
	s, err := Open(t.Context(), dir)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = s.Close() }()
	p, err := s.CreateSession(t.Context(), "policy", t.TempDir(), target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	if err = s.SetPermissionPolicy(t.Context(), p.ID, hub.PermissionFull, false); err == nil {
		t.Fatal("unconfirmed Full accepted")
	}
	if err = s.SetPermissionPolicy(t.Context(), p.ID, "unrecognized", true); err == nil {
		t.Fatal("unknown policy accepted")
	}
	value, err := s.Settings(t.Context(), p.ID)
	if err != nil {
		t.Fatal(err)
	}
	value.PermissionPolicy = hub.PermissionFull
	if err = s.SaveSettings(t.Context(), value); err != nil {
		t.Fatal(err)
	}
	value, _ = s.Settings(t.Context(), p.ID)
	if value.PermissionPolicy.Effective() != hub.PermissionAsk {
		t.Fatal("configuration import elevated authority")
	}
	if err = s.SetPermissionPolicy(t.Context(), p.ID, hub.PermissionFull, true); err != nil {
		t.Fatal(err)
	}
	if err = s.Close(); err != nil {
		t.Fatal(err)
	}
	s, err = Open(t.Context(), dir)
	if err != nil {
		t.Fatal(err)
	}
	value, err = s.Settings(t.Context(), p.ID)
	if err != nil || value.PermissionPolicy != hub.PermissionFull {
		t.Fatal("policy was not durable", err)
	}
	value.PermissionPolicy = hub.PermissionAsk
	if err = s.SaveSettings(t.Context(), value); err != nil {
		t.Fatal(err)
	}
	value, _ = s.Settings(t.Context(), p.ID)
	if value.PermissionPolicy != hub.PermissionFull {
		t.Fatal("ordinary edit silently changed policy")
	}
	if err = s.SetPlanMode(t.Context(), p.ID, true); err != nil {
		t.Fatal(err)
	}
	value, _ = s.Settings(t.Context(), p.ID)
	if !value.PlanMode || value.PermissionPolicy != hub.PermissionFull {
		t.Fatal("entering Plan changed the permission tier")
	}
	if err = s.SetPlanMode(t.Context(), p.ID, false); err != nil {
		t.Fatal(err)
	}
	value, _ = s.Settings(t.Context(), p.ID)
	if value.PlanMode || value.PermissionPolicy != hub.PermissionFull {
		t.Fatal("leaving Plan changed the permission tier")
	}
	if _, err = s.BeginTurn(t.Context(), p.ID, "freeze"); err != nil {
		t.Fatal(err)
	}
	if err = s.SetPermissionPolicy(t.Context(), p.ID, hub.PermissionFull, true); err == nil {
		t.Fatal("running policy changed")
	}
}
