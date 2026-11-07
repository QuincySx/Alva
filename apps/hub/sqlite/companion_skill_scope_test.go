// INPUT: Real companion/native sessions, explicit pinned Skill scope and ordinary settings imports.
// OUTPUT: Restricted scope persistence, no import widening, idle gating and reopen regressions.
// POS: [L7 storage tests] No workspace Skill bodies, model or user database is accessed.
package sqlite

import (
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
)

func TestCompanionSkillScopeCannotBeWidenedByOrdinarySettings(t *testing.T) {
	ctx := t.Context()
	dir := filepath.Join(t.TempDir(), "product")
	s, err := Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = s.Close() })
	p := companionProfile(t, s, "restricted")
	p.Target = target("alva")
	p, err = s.SaveCompanion(ctx, p, p.Revision)
	if err != nil {
		t.Fatal(err)
	}
	c, err := s.OpenCompanionConversation(ctx, p.OwnerID, p.ID, "", 0, t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	initial, err := s.Settings(ctx, c.SessionID)
	if err != nil || initial.CompanionSkills == nil || len(initial.CompanionSkills.Skills) != 0 {
		t.Fatal("companion opened without an explicit empty restriction", initial, err)
	}
	scope := &hub.CompanionSkillScope{Skills: []hub.CompanionSkill{{SourceSessionID: "source", SourceBranchID: "branch", Name: "shared", SHA256: strings.Repeat("a", 64), GroupAllowed: true}}}
	if err := s.SetCompanionSkills(ctx, c.SessionID, scope); err != nil {
		t.Fatal(err)
	}
	scope.Skills[0].Name = "caller-mutated"
	settings, err := s.Settings(ctx, c.SessionID)
	if err != nil || settings.CompanionSkills.Skills[0].Name != "shared" || !settings.SkillsEnabled {
		t.Fatal("scoped save did not retain its value", settings, err)
	}
	settings.CompanionSkills = nil
	if err := s.SaveSettings(ctx, settings); err != nil {
		t.Fatal(err)
	}
	if err := s.SetCompanionSkills(ctx, c.SessionID, nil); err == nil {
		t.Fatal("companion restriction cleared")
	}
	ordinary, err := s.CreateSession(ctx, "ordinary", t.TempDir(), target("alva"))
	if err != nil {
		t.Fatal(err)
	}
	if err := s.SetCompanionSkills(ctx, ordinary.ID, &hub.CompanionSkillScope{}); err == nil {
		t.Fatal("ordinary session treated as a companion")
	}
	if err := s.Close(); err != nil {
		t.Fatal(err)
	}
	s, err = Open(ctx, dir)
	if err != nil {
		t.Fatal(err)
	}
	settings, err = s.Settings(ctx, c.SessionID)
	if err != nil || settings.CompanionSkills == nil || settings.CompanionSkills.Skills[0].Name != "shared" {
		t.Fatal("reopen or settings import lost restriction", settings, err)
	}
	if _, err := s.BeginTurn(ctx, c.SessionID, "freeze"); err != nil {
		t.Fatal(err)
	}
	if err := s.SetCompanionSkills(ctx, c.SessionID, &hub.CompanionSkillScope{}); err == nil {
		t.Fatal("active recipe scope changed")
	}
}
