// INPUT: Real SQLite product service and frozen typed composer selections at explicit Send.
// OUTPUT: Revalidation before admission, duplicate bounds and persistent Plan intent regressions.
// POS: [L7 Hub product tests] No external model, automatic Skill enablement or parallel admission authority.
package product

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestComposerDraftRejectsTamperingBeforeModeOrAdmission(t *testing.T) {
	s, requests := fixture(t)
	p, err := s.Create(t.Context(), "frozen selection", "")
	if err != nil {
		t.Fatal(err)
	}
	selection, err := s.PrepareComposerSelection(t.Context(), p.ID, ComposerSelectionRequest{EntryID: "builtin:plan"})
	if err != nil {
		t.Fatal(err)
	}
	selection.Text += " altered"
	if _, err = s.SubmitComposerDraft(t.Context(), p.ID, "task", []ComposerPreparation{selection}); err == nil {
		t.Fatal("altered preparation admitted")
	}
	settings, err := s.Store.Settings(t.Context(), p.ID)
	if err != nil || settings.PlanMode || s.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("failed revalidation changed mode or executed", err)
	}
	selection, err = s.PrepareComposerSelection(t.Context(), p.ID, ComposerSelectionRequest{EntryID: "builtin:review"})
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.SubmitComposerDraft(t.Context(), p.ID, "task", []ComposerPreparation{selection, selection}); err == nil {
		t.Fatal("duplicate selection admitted")
	}
}

func TestComposerDraftRejectsChangedSkillBeforeAdmission(t *testing.T) {
	s, requests := fixture(t)
	p, err := s.Create(t.Context(), "changed skill", "")
	if err != nil {
		t.Fatal(err)
	}
	directory := filepath.Join(p.Workspace, ManagedSkillDirectory, "inspect")
	if err = os.MkdirAll(directory, 0700); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(directory, "SKILL.md")
	raw := "---\nname: inspect\ndescription: Read project sources\n---\nRead relevant files.\n"
	if err = os.WriteFile(path, []byte(raw), 0600); err != nil {
		t.Fatal(err)
	}
	if err = s.Store.SetSkillsEnabled(t.Context(), p.ID, true); err != nil {
		t.Fatal(err)
	}
	selection, err := s.PrepareComposerSelection(t.Context(), p.ID, ComposerSelectionRequest{EntryID: "skill:inspect"})
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(path, []byte(strings.ReplaceAll(raw, "relevant", "different")), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err = s.SubmitComposerDraft(t.Context(), p.ID, "task", []ComposerPreparation{selection}); err == nil {
		t.Fatal("changed original admitted")
	}
	if s.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("stale Skill executed")
	}
}
