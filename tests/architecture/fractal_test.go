// INPUT: Repository-owned Go source files and local directory-level architecture documentation
// OUTPUT: Mandatory INPUT/OUTPUT/POS headers and three-level source directory documentation checks
// POS: CI guard for the accepted fractal documentation contract, including tagged and helper sources
package architecture_test

import (
	"io/fs"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestFractalSourceHeadersAndDirectories(t *testing.T) {
	directories := map[string]bool{}
	err := filepath.WalkDir(goRoot(), func(path string, entry fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if entry.IsDir() {
			if entry.Name() == "build" || entry.Name() == ".mygo" || entry.Name() == "vendor" {
				return filepath.SkipDir
			}
			return nil
		}
		if !strings.HasSuffix(path, ".go") {
			return nil
		}
		directories[filepath.Dir(path)] = true
		data, err := os.ReadFile(path)
		if err != nil {
			return err
		}
		lines := strings.SplitN(string(data), "\n", 4)
		for i, marker := range []string{"// INPUT:", "// OUTPUT:", "// POS:"} {
			if len(lines) <= i || !strings.HasPrefix(lines[i], marker) || strings.TrimSpace(strings.TrimPrefix(lines[i], marker)) == "" {
				t.Errorf("%s: line %d must contain a nonempty %s header", path, i+1, marker)
			}
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	for dir := range directories {
		path := filepath.Join(dir, "AGENTS.md")
		data, err := os.ReadFile(path)
		if err != nil {
			t.Errorf("%s: source directory requires AGENTS.md: %v", dir, err)
			continue
		}
		for _, section := range []string{"## 地位", "## 逻辑", "## 约束", "## 业务域清单"} {
			if !strings.Contains(string(data), section) {
				t.Errorf("%s: missing %s", path, section)
			}
		}
	}
}
