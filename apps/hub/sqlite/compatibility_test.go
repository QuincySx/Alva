// INPUT: Product storage port and explicit next-turn compatibility file settings.
// OUTPUT: Default-off, import reset, confined path, idle-only and reopen persistence regressions.
// POS: [L7 storage provider] Stores project configuration selection without reading files or loading plugins.
package sqlite

import (
	"path/filepath"
	"testing"
)

func TestCompatibilitySettingsAreExplicitConfinedAndDurable(t *testing.T) {
	ctx := t.Context()
	dir := filepath.Join(t.TempDir(), "private")
	s, err := Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	p, err := s.CreateSession(ctx, "compatibility", t.TempDir(), target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	settings, err := s.Settings(ctx, p.ID)
	if err != nil || settings.CompatibilityFile != "" {
		t.Fatal(settings, err)
	}
	for _, file := range []string{".", "../outside.json", "/outside.json", `a\b.json`} {
		if err := s.SetCompatibilityFile(ctx, p.ID, file); err == nil {
			t.Fatal("unconfined config", file)
		}
	}
	settings.CompatibilityFile = "compat.json"
	if err := s.SaveSettings(ctx, settings); err != nil {
		t.Fatal(err)
	}
	saved, _ := s.Settings(ctx, p.ID)
	if saved.CompatibilityFile != "" {
		t.Fatal("generic import enabled compatibility")
	}
	if err := s.SetCompatibilityFile(ctx, p.ID, "compat.json"); err != nil {
		t.Fatal(err)
	}
	if err := s.Close(); err != nil {
		t.Fatal(err)
	}
	s, err = Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	defer s.Close()
	saved, err = s.Settings(ctx, p.ID)
	if err != nil || saved.CompatibilityFile != "compat.json" {
		t.Fatal(saved, err)
	}
	if _, err := s.BeginTurn(ctx, p.ID, "freeze"); err != nil {
		t.Fatal(err)
	}
	if err := s.SetCompatibilityFile(ctx, p.ID, ""); err == nil {
		t.Fatal("active recipe changed")
	}
}
