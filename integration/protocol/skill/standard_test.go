// INPUT: Official Agent Skills document shapes and native Alva format controls.
// OUTPUT: Opt-in string tool lists, preserved requirements and strict malformed/control-field denial.
// POS: [integration / protocol] Verifies format normalization without changing native parsing or granting tools.
package skill

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestAgentSkillsFormatIsExplicitAndPreservesMetadata(t *testing.T) {
	data := []byte("---\nname: example\ndescription: " + strings.Repeat("中", 1024) + "\ncompatibility: Needs a local compiler\nlicense: MIT\nallowed-tools: Read Bash(git:*)\nmetadata:\n  author: example\n---\nOnly load this body on demand.\n")
	doc, err := ParseAgentSkills(data)
	if err != nil || doc.Meta.Compatibility != "Needs a local compiler" || len(doc.Meta.AllowedTools) != 2 || doc.Meta.Metadata["author"] != "example" {
		t.Fatal(doc, err)
	}
	if _, err := Parse(data); err == nil {
		t.Fatal("native parser silently adopted the foreign allowed-tools format")
	}
	root := t.TempDir()
	if err := os.MkdirAll(filepath.Join(root, "skills", "example"), 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "skills", "example", "SKILL.md"), data, 0600); err != nil {
		t.Fatal(err)
	}
	r, err := OpenAgentSkillsWithin(root, "skills")
	if err != nil {
		t.Fatal(err)
	}
	defer r.Close()
	list, err := r.List(context.Background())
	if err != nil || len(list) != 1 || list[0].Meta.Compatibility != doc.Meta.Compatibility {
		t.Fatal(list, err)
	}
	if err := r.Install(context.Background(), "another", map[string][]byte{"SKILL.md": []byte("---\nname: another\ndescription: test\n---\n")}); err == nil {
		t.Fatal("standard compatibility root became writable")
	}
}

func TestAgentSkillsRejectsInvalidOrVendorExecutionFields(t *testing.T) {
	for _, head := range []string{
		"name: two--hyphens\ndescription: test", "name: example\ndescription: ''", "name: example\ndescription: test\ncompatibility: " + strings.Repeat("a", 501),
		"name: example\ndescription: test\nallowed-tools: [Read]", "name: example\ndescription: test\nmetadata: {revision: 2}",
		"name: example\ndescription: test\nhooks: {PreToolUse: run}", "name: example\ndescription: test\ninvocation: explicit", "name: example\nname: duplicate\ndescription: test",
	} {
		if _, err := ParseAgentSkills([]byte("---\n" + head + "\n---\nbody")); err == nil {
			t.Fatal("accepted invalid standard document", head)
		}
	}
}
