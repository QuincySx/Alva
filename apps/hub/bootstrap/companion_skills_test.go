// INPUT: Real temporary managed Skills and a local HTTP model fixture with mixed sharing choices.
// OUTPUT: Restricted catalog/load/resource coverage, changed-source rejection and actual group request isolation.
// POS: [L7 assembly tests] Exercises the real Skill boundary without external accounts or user data.
package bootstrap

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/llm/provider"
	"github.com/QuincySx/Alva/integration/protocol/skill"
)

func companionSkillFixture(t *testing.T) (*skill.FS, string, hub.CompanionSkill) {
	t.Helper()
	workspace := t.TempDir()
	if err := os.MkdirAll(filepath.Join(workspace, ".alva", "skills"), 0700); err != nil {
		t.Fatal(err)
	}
	repository, err := skill.OpenWithin(workspace, ".alva/skills", true)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = repository.Close() })
	for _, raw := range []string{
		"---\nname: shared-guide\ndescription: Shared metadata\n---\nShared instructions.\n",
		"---\nname: private-guide\ndescription: Private metadata\n---\nPrivate instructions.\n",
	} {
		document, parseErr := skill.Parse([]byte(raw))
		if parseErr != nil {
			t.Fatal(parseErr)
		}
		if err = repository.Install(t.Context(), document.Meta.Name, map[string][]byte{"SKILL.md": []byte(raw)}); err != nil {
			t.Fatal(err)
		}
	}
	if err = os.WriteFile(filepath.Join(workspace, ".alva", "skills", "shared-guide", "notes.txt"), []byte("selected resource"), 0600); err != nil {
		t.Fatal(err)
	}
	original, err := repository.Resource(t.Context(), "shared-guide", "SKILL.md")
	if err != nil {
		t.Fatal(err)
	}
	sum := sha256.Sum256(original.Bytes)
	ref := hub.CompanionSkill{SourceSessionID: "source-session", SourceBranchID: "source-branch", Name: "shared-guide", SHA256: hex.EncodeToString(sum[:]), GroupAllowed: true}
	return repository, workspace, ref
}

func TestCompanionSkillScopeRepositoryFiltersEveryOperationAndChangedSource(t *testing.T) {
	repository, workspace, ref := companionSkillFixture(t)
	ordinary, err := filterCompanionSkills(repository, nil)
	if err != nil || ordinary != repository {
		t.Fatal("ordinary repository changed", err)
	}
	scope := &hub.CompanionSkillScope{Skills: []hub.CompanionSkill{ref}}
	filtered, err := filterCompanionSkills(repository, scope)
	if err != nil {
		t.Fatal(err)
	}
	// Editing the caller's draft cannot broaden this admitted repository.
	scope.Skills[0].Name = "private-guide"
	list, err := filtered.List(t.Context())
	if err != nil || len(list) != 1 || list[0].Meta.Name != "shared-guide" {
		t.Fatal("catalog exposed an unselected Skill", list, err)
	}
	document, err := filtered.Load(t.Context(), ref.Name)
	if err != nil || document.Body != "Shared instructions." {
		t.Fatal("selected instructions missing", document, err)
	}
	paths, err := filtered.Resources(t.Context(), ref.Name)
	if err != nil || len(paths) != 1 || paths[0] != "notes.txt" {
		t.Fatal("selected resource index missing", paths, err)
	}
	resource, err := filtered.Resource(t.Context(), ref.Name, "notes.txt")
	if err != nil || string(resource.Bytes) != "selected resource" {
		t.Fatal("selected resource missing", resource, err)
	}
	if _, err = filtered.Load(t.Context(), "private-guide"); err == nil {
		t.Fatal("unshared instructions loaded")
	}
	if _, err = filtered.Resources(t.Context(), "private-guide"); err == nil {
		t.Fatal("unshared resource index loaded")
	}
	if _, err = filtered.Resource(t.Context(), "private-guide", "SKILL.md"); err == nil {
		t.Fatal("unshared original loaded")
	}
	if err = os.WriteFile(filepath.Join(workspace, ".alva", "skills", ref.Name, "SKILL.md"), []byte("---\nname: shared-guide\ndescription: Replacement metadata\n---\nReplacement instructions.\n"), 0600); err != nil {
		t.Fatal(err)
	}
	checks := []func() error{
		func() error { _, err := filtered.List(t.Context()); return err },
		func() error { _, err := filtered.Load(t.Context(), ref.Name); return err },
		func() error { _, err := filtered.Resources(t.Context(), ref.Name); return err },
		func() error { _, err := filtered.Resource(t.Context(), ref.Name, "notes.txt"); return err },
	}
	for i, check := range checks {
		if err = check(); err == nil || !strings.Contains(err.Error(), "changed") {
			t.Fatalf("operation %d accepted changed Skill instructions: %v", i, err)
		}
	}
}

func TestCompanionSkillScopeEmptyRejectsEveryReadWithoutRepository(t *testing.T) {
	repository, err := filterCompanionSkills(nil, &hub.CompanionSkillScope{})
	if err != nil {
		t.Fatal(err)
	}
	if list, err := repository.List(t.Context()); err != nil || len(list) != 0 {
		t.Fatal("explicit empty scope was unrestricted", list, err)
	}
	if _, err = repository.Load(t.Context(), "any-guide"); err == nil {
		t.Fatal("empty scope loaded instructions")
	}
	if _, err = repository.Resources(t.Context(), "any-guide"); err == nil {
		t.Fatal("empty scope loaded resource index")
	}
	if _, err = repository.Resource(t.Context(), "any-guide", "SKILL.md"); err == nil {
		t.Fatal("empty scope loaded resource bytes")
	}
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	if _, err = repository.List(ctx); err != context.Canceled {
		t.Fatal("empty scope ignored cancellation", err)
	}
}

func TestCompanionSkillScopeActualGroupRequestOmitsUnsharedSkills(t *testing.T) {
	bodies := make(chan json.RawMessage, 4)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var body json.RawMessage
		if err := json.NewDecoder(http.MaxBytesReader(w, r.Body, 1<<20)).Decode(&body); err != nil {
			http.Error(w, "bad request", 400)
			return
		}
		bodies <- body
		waitReply(w, "", "", "local group response")
	}))
	defer server.Close()
	app, err := Open(t.Context(), Config{OpenStores: testStores, OpenMemory: LocalMemory, OpenBindings: LocalBindings, DataDirectory: filepath.Join(t.TempDir(), "private"), ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: server.URL + "/chat", Model: "gpt-5.6"}})
	if err != nil {
		t.Fatal(err)
	}
	defer closeApp(t, app)
	project, err := app.Service.Create(t.Context(), "explicit project", t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	if err = app.Service.InitializeSkills(t.Context(), project.ID); err != nil {
		t.Fatal(err)
	}
	for _, raw := range []string{
		"---\nname: public-guide\ndescription: PUBLIC-GROUP-METADATA\n---\nPublic selected instructions.\n",
		"---\nname: private-guide\ndescription: PRIVATE-SKILL-METADATA-MUST-NOT-ENTER-GROUP\n---\nPrivate-only selected instructions.\n",
	} {
		if err = app.Service.InstallSkill(t.Context(), project.ID, raw); err != nil {
			t.Fatal(err)
		}
	}
	if err = app.Store.SetSkillsEnabled(t.Context(), project.ID, true); err != nil {
		t.Fatal(err)
	}
	public, err := app.Service.PinCompanionSkill(t.Context(), project.ID, "public-guide", true)
	if err != nil {
		t.Fatal(err)
	}
	private, err := app.Service.PinCompanionSkill(t.Context(), project.ID, "private-guide", false)
	if err != nil {
		t.Fatal(err)
	}
	p, err := app.Service.SaveCompanionProfile(t.Context(), hub.CompanionProfile{OwnerID: hub.LocalCompanionOwner, Name: "reviewer", Target: app.Targets[0], Workspace: project.Workspace, Skills: []hub.CompanionSkill{public, private}, Enabled: true}, 0)
	if err != nil {
		t.Fatal(err)
	}
	g, err := app.Service.SaveCompanionGroup(t.Context(), hub.CompanionGroup{OwnerID: p.OwnerID, Name: "mixed sharing", Members: []string{p.ID}, Workspace: project.Workspace, ReplyMode: "all", ReplyLimit: 1, Enabled: true}, 0)
	if err != nil {
		t.Fatal(err)
	}
	round, err := app.Service.RunCompanionGroup(t.Context(), p.OwnerID, g.ID, g.Revision, "Use public guide only", nil, true)
	if err != nil || round.Status != "completed" {
		t.Fatal("actual group execution failed", round.Status, err)
	}
	raw := <-bodies
	if strings.Contains(string(raw), "PRIVATE-SKILL-METADATA-MUST-NOT-ENTER-GROUP") || strings.Contains(string(raw), "Private-only selected instructions.") {
		t.Fatal("unshared Skill appeared in the actual group model request")
	}
	if !strings.Contains(string(raw), "PUBLIC-GROUP-METADATA") {
		t.Fatal("allowed Skill metadata missing from actual group model request")
	}
}
