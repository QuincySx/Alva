// INPUT: testing, explicit temporary directories and Skill protocol APIs
// OUTPUT: Progressive loading, metadata, path safety and recoverable mutation regressions
// POS: [integration / external protocol] Local-only Skill protocol acceptance without script execution or global discovery
package skill

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func manifest(name, invocation string) []byte {
	return []byte("---\nname: " + name + "\ndescription: |\n  Multiline skill\n  trigger text\ninvocation: " + invocation + "\nallowed-tools: [read_file]\nmetadata: {version: '1'}\n---\n# Hidden body\nDo not execute scripts automatically.\n")
}
func fixture(t *testing.T) *FS {
	t.Helper()
	r, err := Open([]Root{{Path: t.TempDir(), Writable: true}})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { r.Close() })
	return r
}
func TestProjectSkillAncestorCannotEscapeAndOpenRemainsPinned(t *testing.T) {
	workspace, outside := t.TempDir(), t.TempDir()
	if err := os.MkdirAll(filepath.Join(outside, "skills"), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(outside, filepath.Join(workspace, ".alva")); err != nil {
		t.Skip(err)
	}
	if repo, err := OpenWithin(workspace, ".alva/skills", true); err == nil {
		repo.Close()
		t.Fatal("symlink ancestor granted outside project")
	}
	if err := os.Remove(filepath.Join(workspace, ".alva")); err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(filepath.Join(workspace, ".alva/skills"), 0700); err != nil {
		t.Fatal(err)
	}
	repo, err := OpenWithin(workspace, ".alva/skills", true)
	if err != nil {
		t.Fatal(err)
	}
	defer repo.Close()
	if err = os.Rename(filepath.Join(workspace, ".alva"), filepath.Join(workspace, ".alva-pinned")); err != nil {
		t.Fatal(err)
	}
	if err = os.Symlink(outside, filepath.Join(workspace, ".alva")); err != nil {
		t.Fatal(err)
	}
	if err = repo.Install(context.Background(), "example", map[string][]byte{"SKILL.md": manifest("example", "auto")}); err != nil {
		t.Fatal(err)
	}
	if _, err = os.Stat(filepath.Join(outside, "skills/example")); !os.IsNotExist(err) {
		t.Fatal("pinned write escaped to replacement")
	}
	if _, err = os.Stat(filepath.Join(workspace, ".alva-pinned/skills/example/SKILL.md")); err != nil {
		t.Fatal(err)
	}
}
func TestSkillParserAndInvocation(t *testing.T) {
	doc, err := Parse(manifest("example", "explicit"))
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(doc.Meta.Description, "trigger text") || !strings.Contains(doc.Body, "Hidden body") || doc.EstimatedTokens == 0 {
		t.Fatalf("bad document: %+v", doc)
	}
	summary, err := Summary([]Skill{{Meta: doc.Meta, Enabled: true}})
	if err != nil || summary != "" {
		t.Fatalf("explicit skill advertised: %q %v", summary, err)
	}
	for _, data := range [][]byte{[]byte("no frontmatter"), []byte("---\nname: ../escape\ndescription: bad\n---\nx"), []byte("---\nname: x\nname: y\ndescription: bad\n---\nx"), []byte("---\nname: x\ndescription: '<tag>'\n---\nx"), []byte("---\nname: x\ndescription: good\ninvocation: mystery\n---\nx")} {
		if _, err := Parse(data); err == nil {
			t.Errorf("invalid manifest accepted: %s", data)
		}
	}
}
func TestProgressiveLoadAndArchiveRestore(t *testing.T) {
	r := fixture(t)
	ctx := context.Background()
	if err := r.Install(ctx, "example", map[string][]byte{"SKILL.md": manifest("example", "auto"), "references/api.md": []byte("large resource kept out of metadata")}); err != nil {
		t.Fatal(err)
	}
	list, err := r.List(ctx)
	if err != nil || len(list) != 1 {
		t.Fatalf("catalog: %+v %v", list, err)
	}
	summary, err := Summary(list)
	if err != nil || strings.Contains(summary, "Hidden body") || strings.Contains(summary, "large resource") {
		t.Fatalf("eager body: %q %v", summary, err)
	}
	list[0].Meta.AllowedTools[0] = "hacked"
	listAgain, _ := r.List(ctx)
	if listAgain[0].Meta.AllowedTools[0] != "read_file" {
		t.Fatal("catalog ownership leaked")
	}
	full, err := Injection(ctx, r, []Ref{{Name: "example", Policy: Strict}})
	if err != nil || !strings.Contains(full, "Hidden body") || !strings.Contains(full, "does not grant") {
		t.Fatalf("injection: %s %v", full, err)
	}
	paths, err := r.Resources(ctx, "example")
	if err != nil || len(paths) != 1 || paths[0] != "references/api.md" {
		t.Fatalf("resources: %v %v", paths, err)
	}
	resource, err := r.Resource(ctx, "example", paths[0])
	if err != nil || !strings.Contains(string(resource.Bytes), "large resource") {
		t.Fatal(err)
	}
	id, err := r.Archive(ctx, "example")
	if err != nil {
		t.Fatal(err)
	}
	list, err = r.List(ctx)
	if err != nil || len(list) != 0 {
		t.Fatalf("archived still listed: %v %v", list, err)
	}
	if err = r.Restore(ctx, id); err != nil {
		t.Fatal(err)
	}
	if _, err = r.Load(ctx, "example"); err != nil {
		t.Fatal(err)
	}
}
func TestSkillPathsSymlinksAndBudgets(t *testing.T) {
	r := fixture(t)
	ctx := context.Background()
	if err := r.Install(ctx, "example", map[string][]byte{"SKILL.md": manifest("example", "auto")}); err != nil {
		t.Fatal(err)
	}
	for _, p := range []string{"../outside", "/etc/passwd", "a\\b", "a/../../b"} {
		if _, err := r.Resource(ctx, "example", p); err == nil {
			t.Errorf("unsafe path %q accepted", p)
		}
	}
	if err := r.Install(ctx, "bad", map[string][]byte{"SKILL.md": manifest("bad", "auto"), "../outside": []byte("bad")}); err == nil {
		t.Fatal("bundle traversal accepted")
	}
	if err := r.Install(ctx, "big", map[string][]byte{"SKILL.md": manifest("big", "auto"), "x": make([]byte, MaxDocumentBytes+1)}); err == nil {
		t.Fatal("oversized resource accepted")
	}
	if err := os.Symlink("SKILL.md", filepath.Join(r.roots[0].root.Name(), "example", "alias")); err != nil {
		t.Fatal(err)
	}
	if _, err := r.Resource(ctx, "example", "alias"); err == nil {
		t.Fatal("symlink accepted")
	}
	if _, err := r.Archive(ctx, ".."); err == nil {
		t.Fatal("broad archive accepted")
	}
	cancelled, cancel := context.WithCancel(ctx)
	cancel()
	if _, err := r.Load(cancelled, "example"); err == nil {
		t.Fatal("cancelled read succeeded")
	}
}
func TestReadOnlyRootsAndExplicitRefresh(t *testing.T) {
	dir := t.TempDir()
	r, err := Open([]Root{{Path: dir}})
	if err != nil {
		t.Fatal(err)
	}
	defer r.Close()
	ctx := context.Background()
	if err = r.Install(ctx, "example", map[string][]byte{"SKILL.md": manifest("example", "auto")}); err == nil {
		t.Fatal("wrote readonly root")
	}
	if _, err = r.List(ctx); err != nil {
		t.Fatal(err)
	}
	if err = os.Mkdir(filepath.Join(dir, "example"), 0700); err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(dir, "example", "SKILL.md"), manifest("example", "auto"), 0600); err != nil {
		t.Fatal(err)
	}
	list, _ := r.List(ctx)
	if len(list) != 0 {
		t.Fatal("implicit rescan")
	}
	if err = r.Refresh(ctx); err != nil {
		t.Fatal(err)
	}
	list, _ = r.List(ctx)
	if len(list) != 1 {
		t.Fatal("refresh missed skill")
	}
}
