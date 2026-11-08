// INPUT: Real product fixture and a saved workspace with managed Skills
// OUTPUT: Skill form create/edit/rename, resource preservation and Chinese field-error regressions
// POS: [L7 Hub product] Editing never loses the old version or its resource files
package product

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestSkillFormCreatesEditsAndKeepsResources(t *testing.T) {
	s, _ := fixture(t)
	ctx := context.Background()
	session := settingsSession(t, s, "skills")
	if err := s.InitializeSkills(ctx, session.ID); err != nil {
		t.Fatal(err)
	}
	var problems FieldErrors
	if err := s.SaveSkillForm(ctx, session.ID, "", SkillForm{Name: "Bad Name", Description: "", Body: ""}); !errors.As(err, &problems) || problems.For("skill.name") == "" || problems.For("skill.description") == "" || problems.For("skill.body") == "" {
		t.Fatal("invalid form not reported per field", err)
	}
	if err := s.SaveSkillForm(ctx, session.ID, "", SkillForm{Name: "notes", Description: "Write release notes", Body: "Use the changelog."}); err != nil {
		t.Fatal(err)
	}
	resource := filepath.Join(session.Workspace, ManagedSkillDirectory, "notes", "template.md")
	if err := os.WriteFile(resource, []byte("kept resource"), 0600); err != nil {
		t.Fatal(err)
	}
	form, err := s.SkillFormFor(ctx, session.ID, "notes")
	if err != nil || form.Body != "Use the changelog." || form.Description != "Write release notes" {
		t.Fatal("form did not load the saved skill", form, err)
	}
	form.Name, form.Body, form.Explicit = "release-notes", "Use the changelog and template.md.", true
	if err = s.SaveSkillForm(ctx, session.ID, "notes", form); err != nil {
		t.Fatal(err)
	}
	edited, err := s.SkillDocument(ctx, session.ID, "release-notes")
	if err != nil || !strings.Contains(edited.Body, "template.md") || edited.Meta.Invocation != "explicit" {
		t.Fatal("edit not saved", edited, err)
	}
	data, err := os.ReadFile(filepath.Join(session.Workspace, ManagedSkillDirectory, "release-notes", "template.md"))
	if err != nil || string(data) != "kept resource" {
		t.Fatal("edit lost the skill's resource file", err)
	}
	if _, err = s.SkillDocument(ctx, session.ID, "notes"); err == nil {
		t.Fatal("renamed skill left the old version active")
	}
	view, err := s.Settings.Load(ctx, appScope(session.ID))
	if err != nil || len(view.Session.SkillArchives) != 1 {
		t.Fatal("old version was not archived recoverably", err)
	}
	// Creating over an existing name never overwrites it.
	if err = s.SaveSkillForm(ctx, session.ID, "", SkillForm{Name: "release-notes", Description: "Other", Body: "Other"}); !errors.As(err, &problems) {
		t.Fatal("existing skill was overwritten", err)
	}
}
