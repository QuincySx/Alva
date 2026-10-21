// INPUT: Native and standard copies sharing logical names, actual roots and existing SDK composition.
// OUTPUT: One catalog entry for equal definitions/resources, source-qualified conflicts and scope-preserving rules.
// POS: [L4 extension] Verifies deduplication without permission unions, body-only identity or silent overrides.
package compatibility

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/integration/protocol/skill"
	agentcontext "github.com/QuincySx/Alva/sdk/agent/context"
	"github.com/QuincySx/Alva/sdk/agent/core"
)

func duplicateDocuments(t *testing.T, root string) {
	t.Helper()
	put(t, root, "native/review/SKILL.md", "---\nname: review\ndescription: shared trigger\nlicense: MIT\nallowed-tools: [Read, Grep]\ninvocation: auto\n---\nShared instructions\n")
	put(t, root, "standard/review/SKILL.md", "---\nname: review\ndescription: shared trigger\nlicense: MIT\nallowed-tools: Grep Read\n---\nShared instructions\n")
	for _, directory := range []string{"native", "standard"} {
		put(t, root, directory+"/review/scripts/check.js", "// selected resource text, never executed")
	}
}

func TestSameNameNativeAndStandardSkillsMergeOnlyWhenDefinitionsAndResourcesMatch(t *testing.T) {
	ctx := t.Context()
	root := t.TempDir()
	duplicateDocuments(t, root)
	config := Config{Version: 1, Skills: []SkillSource{{"native", "alva"}, {"standard", "agent-skills"}, {"native", "alva"}}}
	snapshot, err := Open(ctx, root, config)
	if err != nil {
		t.Fatal(err)
	}
	defer snapshot.Close(ctx)
	list, err := snapshot.List(ctx)
	if err != nil || len(list) != 1 || list[0].Meta.Name != "review" || len(snapshot.Receipt().Sources) != 2 {
		t.Fatal(list, snapshot.Receipt(), err)
	}
	base, err := skill.OpenWithin(root, "native", false)
	if err != nil {
		t.Fatal(err)
	}
	defer base.Close()
	combined := &mergedRepository{snapshot: snapshot, base: base}
	list, err = combined.List(ctx)
	if err != nil || len(list) != 1 {
		t.Fatal(list, err)
	}
	native, _ := base.List(ctx)
	if list[0].Source != native[0].Source {
		t.Fatal("matching duplicates did not retain the native canonical source", list)
	}
	doc, err := combined.Load(ctx, "review")
	if err != nil || strings.Join(doc.Meta.AllowedTools, ",") != "Read,Grep" {
		t.Fatal("duplicate merge altered requested tools", doc, err)
	}
	refs, _ := agentcontext.NewMemoryReferences("native", 1<<20)
	plugin := snapshot.Plugin("native", refs, base)
	model := &captureModel{}
	agent, err := core.NewBuilder(model, core.NewMemorySession("native")).Plugin(plugin).Build(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if len(plugin.Tools()) != 1 || agent == nil {
		t.Fatal("duplicate Skill tool installed")
	}
	// Definition identity includes invocation/tool metadata; no union or source-priority override.
	put(t, root, "standard/review/SKILL.md", "---\nname: review\ndescription: shared trigger\nlicense: MIT\nallowed-tools: Write\n---\nShared instructions\n")
	if _, err := Open(ctx, root, config); err == nil || !strings.Contains(err.Error(), "native/review") || !strings.Contains(err.Error(), "standard/review") {
		t.Fatal("divergent definition merged or sources omitted", err)
	}
	if err := snapshot.Close(ctx); err != nil {
		t.Fatal(err)
	}
	if _, err := combined.Load(ctx, "review"); err == nil {
		t.Fatal("cached combined catalog outlived its closed snapshot")
	}
}

func TestSameSkillBodyWithDifferentResourcesIsAConflict(t *testing.T) {
	ctx := t.Context()
	root := t.TempDir()
	duplicateDocuments(t, root)
	put(t, root, "standard/review/scripts/check.js", "DIFFERENT-RESOURCE")
	if _, err := Open(ctx, root, Config{Version: 1, Skills: []SkillSource{{"native", "alva"}, {"standard", "agent-skills"}}}); err == nil || !strings.Contains(err.Error(), "resources") {
		t.Fatal("body-only deduplication hid divergent scripts", err)
	}
	snapshot, err := Open(ctx, root, Config{Version: 1, Skills: []SkillSource{{"standard", "agent-skills"}}})
	if err != nil {
		t.Fatal(err)
	}
	defer snapshot.Close(ctx)
	base, err := skill.OpenWithin(root, "native", false)
	if err != nil {
		t.Fatal(err)
	}
	defer base.Close()
	refs, _ := agentcontext.NewMemoryReferences("native", 1<<20)
	if _, err := core.NewBuilder(&captureModel{}, core.NewMemorySession("native")).Plugin(snapshot.Plugin("native", refs, base)).Build(ctx); err == nil || !strings.Contains(err.Error(), "resources") {
		t.Fatal("native canonical source silently shadowed conflicting assets", err)
	}
}

func TestAgentTemplateCopiesMergeButDirectoryScopedRulesRemain(t *testing.T) {
	ctx := t.Context()
	root := t.TempDir()
	put(t, root, "AGENTS.md", "SAME-SCOPED-RULE")
	put(t, root, "src/AGENTS.md", "SAME-SCOPED-RULE")
	put(t, root, "first/reviewer.md", "---\nname: reviewer\ndescription: review\ntools: Read, Grep\nmodel: inherit\n---\nShared prompt")
	put(t, root, "second/reviewer.md", "---\nname: reviewer\ndescription: review\ntools: Grep, Read\n---\nShared prompt")
	config := Config{Version: 1, Scope: "src", Rules: []string{"agents"}, Agents: []string{"first", "second", "first"}}
	snapshot, err := Open(ctx, root, config)
	if err != nil {
		t.Fatal(err)
	}
	defer snapshot.Close(ctx)
	if len(snapshot.Templates().List()) != 1 || len(snapshot.Receipt().Agents) != 1 || strings.Count(snapshot.prompt, "SAME-SCOPED-RULE") != 2 {
		t.Fatal("lost scoped precedence or duplicated template", snapshot.Receipt(), snapshot.prompt)
	}
	put(t, root, "second/reviewer.md", "---\nname: reviewer\ndescription: review\ntools: Read\n---\nChanged prompt")
	if _, err := Open(ctx, root, config); err == nil || !strings.Contains(err.Error(), "first/reviewer.md") || !strings.Contains(err.Error(), "second/reviewer.md") {
		t.Fatal("template conflict hidden", err)
	}
}

func TestDifferentSkillNamesAreNotMergedByBodyHash(t *testing.T) {
	root := t.TempDir()
	for _, name := range []string{"first", "second"} {
		put(t, root, "skills/"+name+"/SKILL.md", "---\nname: "+name+"\ndescription: identical trigger\n---\nIdentical body")
	}
	snapshot, err := Open(t.Context(), root, Config{Version: 1, Skills: []SkillSource{{"skills", "agent-skills"}}})
	if err != nil {
		t.Fatal(err)
	}
	defer snapshot.Close(t.Context())
	list, err := snapshot.List(t.Context())
	if err != nil || len(list) != 2 {
		t.Fatal("different logical identities were removed", list, err)
	}
	data, err := json.Marshal(snapshot.Receipt())
	if err != nil || !strings.Contains(string(data), "first") || !strings.Contains(string(data), "second") {
		t.Fatal(string(data), err)
	}
}

func TestMergedCatalogRejectsTypedNilNativeRepository(t *testing.T) {
	root := t.TempDir()
	put(t, root, "AGENTS.md", "rules")
	snapshot, err := Open(t.Context(), root, Config{Version: 1, Rules: []string{"agents"}})
	if err != nil {
		t.Fatal(err)
	}
	defer snapshot.Close(t.Context())
	var base *skill.FS
	repository := &mergedRepository{snapshot: snapshot, base: base}
	if _, err := repository.List(t.Context()); err == nil {
		t.Fatal("typed-nil native repository accepted")
	}
}
