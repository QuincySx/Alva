// INPUT: Explicit local product workspace, confined managed skills and temporary symlink fixtures
// OUTPUT: Skill initialization, exact document roundtrip, recoverable archive and scope regressions
// POS: [L7 Hub product] No global skill scan, downloads, model calls or script execution
package product

import (
	"context"
	"os"
	"path/filepath"
	"testing"
)

func TestProjectSkillsExplicitConfinedAndRecoverable(t *testing.T) {
	s, requests := fixture(t)
	ctx := context.Background()
	product, err := s.Create(ctx, "skills", "")
	if err != nil {
		t.Fatal(err)
	}
	if err = s.InitializeSkills(ctx, product.ID); err != nil {
		t.Fatal(err)
	}
	raw := "---\nname: exact-skill\ndescription: fixture skill\ninvocation: explicit\nallowed-tools: [read_file]\nmetadata:\n  purpose: preserve\n---\nComplete instructions; scripts are text, never automatically executed.\n"
	if err = s.InstallSkill(ctx, product.ID, raw); err != nil {
		t.Fatal(err)
	}
	actual, err := s.SkillRaw(ctx, product.ID, "exact-skill")
	if err != nil || actual != raw {
		t.Fatal("skill metadata/frontmatter was rewritten", err)
	}
	if err = s.InstallSkill(ctx, product.ID, raw); err == nil {
		t.Fatal("existing skill overwritten")
	}
	if err = s.Store.SetSkillsEnabled(ctx, product.ID, true); err != nil {
		t.Fatal(err)
	}
	settings, err := s.Store.Settings(ctx, product.ID)
	if err != nil || !settings.SkillsEnabled {
		t.Fatal("next-turn choice not durable", err)
	}
	receipt, err := s.ArchiveSkill(ctx, product.ID, "exact-skill")
	if err != nil {
		t.Fatal(err)
	}
	list, err := s.Skills(ctx, product.ID)
	if err != nil || len(list) != 0 {
		t.Fatal("archive did not remove active listing", err)
	}
	other, err := s.Create(ctx, "foreign", "")
	if err != nil {
		t.Fatal(err)
	}
	if err = s.RestoreSkill(ctx, other.ID, receipt); err == nil {
		t.Fatal("foreign archive receipt accepted")
	}
	if err = s.RestoreSkill(ctx, product.ID, receipt); err != nil {
		t.Fatal(err)
	}
	actual, err = s.SkillRaw(ctx, product.ID, "exact-skill")
	if err != nil || actual != raw {
		t.Fatal("recoverable archive lost original", err)
	}
	if s.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("skill management started an Agent")
	}
}
func TestProjectSkillInitializationRejectsSymlinkAncestor(t *testing.T) {
	s, _ := fixture(t)
	ctx := context.Background()
	workspace := t.TempDir()
	outside := t.TempDir()
	if err := os.Symlink(outside, filepath.Join(workspace, ".alva")); err != nil {
		t.Skip(err)
	}
	product, err := s.Create(ctx, "hostile", workspace)
	if err != nil {
		t.Fatal(err)
	}
	if err = s.InitializeSkills(ctx, product.ID); err == nil {
		t.Fatal("symlink ancestor accepted")
	}
	if _, err = os.Stat(filepath.Join(outside, "skills")); !os.IsNotExist(err) {
		t.Fatal("initialization escaped saved workspace")
	}
}
