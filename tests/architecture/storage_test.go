// INPUT: Source imports, explicit composition locations and common storage dependency graphs.
// OUTPUT: Contract-only consumer enforcement, backend-selection location and SQL-error leakage regressions.
// POS: Keeps the same storage ports from Plugins through the product; only outer assembly chooses implementations.
package architecture_test

import (
	"go/parser"
	"go/token"
	"io/fs"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
)

func allowedStorageImport(file, imported string) bool {
	// bootstrap contains both runtime orchestration and an explicit stock recipe.
	// Only that recipe selects native stores; the rest consumes their ports.
	if strings.HasPrefix(file, "apps/hub/bootstrap/") && concreteStorage(imported) {
		return file == "apps/hub/bootstrap/storage.go"
	}
	if filepath.ToSlash(filepath.Dir(file)) == "platform/storage" && (imported == "os" || imported == "net/http" || imported == "database/sql") {
		return false
	}
	return true
}

func TestStorageSelectionIsConfinedToExplicitRecipe(t *testing.T) {
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
		if !strings.HasSuffix(path, ".go") || strings.HasSuffix(path, "_test.go") {
			return nil
		}
		rel, err := filepath.Rel(goRoot(), path)
		if err != nil {
			return err
		}
		file, err := parser.ParseFile(token.NewFileSet(), path, nil, parser.ImportsOnly)
		if err != nil {
			return err
		}
		for _, declaration := range file.Imports {
			imported, err := strconv.Unquote(declaration.Path.Value)
			if err != nil {
				return err
			}
			if !allowedStorageImport(filepath.ToSlash(rel), strings.TrimPrefix(imported, module)) {
				t.Errorf("%s selects storage outside the explicit composition recipe: %s", rel, imported)
			}
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestStorageFirewallRejectsBackendLeaks(t *testing.T) {
	for name, graph := range map[string]importGraph{
		"Plugin concrete session":               {"extension/media": {"platform/storage/session"}},
		"harness helper hides backend":          {"harness": {"platform/storage/helper"}, "platform/storage/helper": {"platform/storage/session"}},
		"contract imports its implementation":   {"platform/storage": {"platform/storage/session"}},
		"contract inherits SQL":                 {"platform/storage": {"sdk/kernel/abi"}, "sdk/kernel/abi": {"database/sql"}},
		"Inspector depends on provider":         {"integration/engine/alva": {"platform/storage/session"}},
		"UI depends on SQL":                     {"apps/hub/desktop": {"apps/hub/sqlite"}},
		"product classifies SQL errors":         {"apps/hub/product": {"database/sql"}},
		"external runtime selects bindings":     {"integration/engine/process": {"platform/storage/bindings"}},
		"memory service selects backend":        {"extension/memory": {"platform/storage/memory"}},
		"directory primitive inherits database": {"platform/system/privatefs": {"platform/storage/sqlite"}},
		"runtime imports conformance helper":    {"extension/media": {"platform/storage/storagetest"}},
		"runtime hides test helper dependency":  {"sdk/kernel/abi": {"sdk/kernel/abi/helper"}, "sdk/kernel/abi/helper": {"sdk/kernel/abi/sessiontest"}},
		"external example pulls platform":       {"examples/sdk-agent": {"platform/storage"}},
		"conformance helper chooses database":   {"platform/storage/storagetest": {"platform/storage/session"}},
	} {
		t.Run(name, func(t *testing.T) {
			if failures := validate(graph); len(failures) == 0 {
				t.Fatal("concrete backend leaked through storage port", graph)
			}
		})
	}
	if allowedStorageImport("apps/hub/bootstrap/assembly.go", "platform/storage/session") {
		t.Fatal("runtime recipe can select an implicit provider")
	}
	if !allowedStorageImport("apps/hub/bootstrap/storage.go", "platform/storage/session") {
		t.Fatal("explicit stock recipe was rejected")
	}
}

func TestStorageFirewallAcceptsSharedPortsAndExplicitComposition(t *testing.T) {
	graph := importGraph{
		"platform/storage":      {"context", "sdk/kernel/abi", "sdk/agent/memory", "platform/storage/blob"},
		"platform/storage/blob": {"context"},
		"extension/media":       {"platform/storage"}, "harness": {"platform/storage"},
		"apps/hub/product":         {"platform/storage", "apps/hub"},
		"apps/hub/bootstrap":       {"apps/hub/product", "platform/storage/session", "apps/hub/sqlite"},
		"platform/storage/session": {"platform/storage", "platform/storage/sqlite"},
		"apps/hub/sqlite":          {"apps/hub", "platform/storage/sqlite"},
		"platform/storage/sqlite":  {"database/sql", "github.com/ncruces/go-sqlite3/driver"},
		"cmd/alva":                 {"platform/storage/session", "harness"},
	}
	if failures := validate(graph); len(failures) != 0 {
		t.Fatal(failures)
	}
}
