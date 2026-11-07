// INPUT: Explicit disposable managed Skill directories and existing product preparation/branch contracts.
// OUTPUT: Pre-session Skill catalogs, immutable workspace/SHA pins and selected-only typed input regressions.
// POS: [L7 product tests] No global discovery, implicit enablement, session creation or model execution occurs during selection.
package product

import (
	"errors"
	"os"
	"path/filepath"
	"reflect"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
)

func homeComposerSkillFixture(t *testing.T) (string, string) {
	t.Helper()
	workspace, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	directory := filepath.Join(workspace, ManagedSkillDirectory, "exact-home")
	if err = os.MkdirAll(directory, 0700); err != nil {
		t.Fatal(err)
	}
	raw := "---\nname: exact-home\ndescription: Review explicit home source\ninvocation: explicit\n---\nOnly the selected source is requested.\n"
	if err = os.WriteFile(filepath.Join(directory, "SKILL.md"), []byte(raw), 0600); err != nil {
		t.Fatal(err)
	}
	return workspace, raw
}

func TestComposerHomeSkillsAreColdPinnedAndDoNotEnableGlobalCatalog(t *testing.T) {
	s, requests := fixture(t)
	workspace, raw := homeComposerSkillFixture(t)
	page, err := s.ComposerHomeCatalog(t.Context(), workspace, ComposerCatalogQuery{Kind: ComposerSkillEntry, Search: "explicit home"})
	if err != nil || page.Total != 1 || len(page.Entries) != 1 || !page.Entries[0].Available {
		t.Fatal("explicit project Skill unavailable before session creation", page, err)
	}
	prepared, err := s.PrepareComposerHomeSelection(t.Context(), workspace, ComposerSelectionRequest{EntryID: "skill:exact-home", Arguments: map[string]string{"request": "review this task"}})
	if err != nil || prepared.SkillRef == nil || prepared.SkillRef.Workspace != workspace || prepared.SkillRef.SessionID != "" || prepared.SkillRef.BranchID != "" {
		t.Fatal("home selection invented a session or lost source scope", prepared, err)
	}
	commands, err := EncodeComposerDraftPreparations([]ComposerPreparation{prepared})
	if err != nil {
		t.Fatal(err)
	}
	decoded, err := DecodeComposerDraftPreparations(hub.ComposerDraft{Scope: hub.ComposerDraftScope{Kind: "home", ID: "new-conversation"}, PreparedCommands: commands})
	if err != nil || !reflect.DeepEqual(decoded, []ComposerPreparation{prepared}) {
		t.Fatal("durable home preparation did not preserve exact source", decoded, err)
	}
	sessions, err := s.Store.ListSessions(t.Context(), 8)
	if err != nil || len(sessions) != 0 || len(requests) != 0 || s.Runtime.HasActive() {
		t.Fatal("home catalog or selection created work", err)
	}
	session, err := s.Create(t.Context(), "explicit first Send identity", workspace)
	if err != nil {
		t.Fatal(err)
	}
	bound, err := s.rebindComposerSkillsLocked(t.Context(), session.ID, []ComposerPreparation{prepared})
	if err != nil || len(bound) != 1 || bound[0].SkillRef.SessionID != session.ID || bound[0].SkillRef.BranchID != session.ActiveBranchID || prepared.SkillRef.SessionID != "" {
		t.Fatal("first Send did not bind a copied exact source", bound, err)
	}
	detail, err := s.Store.Detail(t.Context(), session.ID)
	if err != nil {
		t.Fatal(err)
	}
	inputs, err := s.composerSkillInputs(t.Context(), detail, bound)
	if err != nil || len(inputs) != 1 || inputs[0].Name != "exact-home" || inputs[0].Path != filepath.Join(workspace, ManagedSkillDirectory, "exact-home", "SKILL.md") || inputs[0].SHA256 != prepared.SkillRef.SHA256 {
		t.Fatal("typed input lost exact selected source", inputs, err)
	}
	settings, err := s.Store.Settings(t.Context(), session.ID)
	if err != nil || settings.SkillsEnabled {
		t.Fatal("explicit source selection enabled the whole project catalog", err)
	}
	if err = os.WriteFile(inputs[0].Path, []byte(raw+"Changed before Send.\n"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err = s.composerSkillInputs(t.Context(), detail, bound); err == nil {
		t.Fatal("changed source was accepted for actual engine input")
	}
	if len(requests) != 0 || s.Runtime.HasActive() {
		t.Fatal("source revalidation executed a model")
	}
}

func TestComposerHomeSkillCatalogNeverCreatesOrEscapesExplicitProject(t *testing.T) {
	s, _ := fixture(t)
	workspace := t.TempDir()
	page, err := s.ComposerHomeCatalog(t.Context(), workspace, ComposerCatalogQuery{Kind: ComposerSkillEntry})
	if err != nil || page.Total != 0 {
		t.Fatal(page, err)
	}
	if _, err = os.Stat(filepath.Join(workspace, ".alva")); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("cold home catalog initialized a project", err)
	}
	if err = os.Symlink(t.TempDir(), filepath.Join(workspace, ".alva")); err != nil {
		t.Fatal(err)
	}
	if _, err = s.ComposerHomeCatalog(t.Context(), workspace, ComposerCatalogQuery{Kind: ComposerSkillEntry}); err == nil {
		t.Fatal("home Skill catalog followed a symlink ancestor")
	}
}
