// INPUT: go/ast, go/parser, filesystem and non-test sources under apps/hub/desktop, apps/hub/server and cmd/alva-hub
// OUTPUT: Direct Store/Runtime/Repository selection rejection with a reasoned, stale-checked allowlist and scanner regressions
// POS: Keeps Hub desktop, HTTP and CLI on the narrow product services (ADR 0013 §6) instead of the product store or execution runtime.
package architecture_test

import (
	"fmt"
	"go/ast"
	"go/parser"
	"go/token"
	"io/fs"
	"os"
	"path"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"testing"
)

// productInterfaceRoots are the Hub entries that must reach Hub facts and
// execution only through product services. cmd/alva is the direct CLI and
// consumes the harness by design, so it is not listed.
var productInterfaceRoots = []string{"apps/hub/desktop", "apps/hub/server", "cmd/alva-hub"}

// productInterfaceFields are the exported owners an entry could reach past
// the product layer: the Hub product store, the execution runtime and the
// native record repository.
var productInterfaceFields = map[string]bool{"Store": true, "Runtime": true, "Repository": true}

// Settings reads and writes move to a product SettingsService in milestone M5,
// developed in parallel; these entries are removed when it merges.
const settingsByM5 = "收口由 M5：设置读写由 M5 的 SettingsService 收口，合并时删除"

// productInterfaceAllowlist holds reviewed selections, keyed
// path#enclosing function#Field.Method, each with a one-line reason. It may
// only shrink; entries that no longer occur fail as stale.
var productInterfaceAllowlist = map[string]string{
	"apps/hub/desktop/app_desktop.go#NativePage.loadTail#Store.Settings":   settingsByM5,
	"apps/hub/desktop/app_desktop.go#NativePage.poll#Store.Settings":       settingsByM5,
	"apps/hub/server/routes.go#Server.nested#Store.SetAEPEnabled":          settingsByM5,
	"apps/hub/server/routes.go#Server.nested#Store.SetBrowserEnabled":      settingsByM5,
	"apps/hub/server/routes.go#Server.nested#Store.SetCommandEnabled":      settingsByM5,
	"apps/hub/server/routes.go#Server.nested#Store.SetMCPEnabled":          settingsByM5,
	"apps/hub/server/routes.go#Server.nested#Store.SetMediaDecoderEnabled": settingsByM5,
	"apps/hub/server/routes.go#Server.nested#Store.SetMediaEnabled":        settingsByM5,
	"apps/hub/server/routes.go#Server.nested#Store.SetPersistentMemory":    settingsByM5,
	"apps/hub/server/routes.go#Server.nested#Store.SetSkillsEnabled":       settingsByM5,
	"apps/hub/server/routes.go#Server.nested#Store.SetSubagentsEnabled":    settingsByM5,
	"apps/hub/server/routes.go#Server.sessionRoute#Store.Settings":         settingsByM5,
}

type productInterfaceHit struct{ key, position string }

// findProductInterfaceSelections reports `x.Store`, `x.Runtime` and
// `x.Repository` selections, naming the member used on them. It is
// name-based on purpose, like the other gates: a reused field name must be
// reviewed, not inferred safe. Package-qualified names (engine.Runtime) are
// types, not owners, and are skipped.
func findProductInterfaceSelections(set *token.FileSet, file *ast.File, rel string) []productInterfaceHit {
	packages := map[string]bool{}
	for _, spec := range file.Imports {
		importPath, err := strconv.Unquote(spec.Path.Value)
		if err != nil {
			continue
		}
		name := path.Base(importPath)
		if len(name) > 1 && name[0] == 'v' && strings.Trim(name[1:], "0123456789") == "" {
			name = path.Base(path.Dir(importPath))
		}
		name = strings.TrimSuffix(strings.TrimPrefix(name, "go-"), ".go")
		if spec.Name != nil {
			name = spec.Name.Name
		}
		packages[name] = true
	}
	owner := func(selector *ast.SelectorExpr) bool {
		if !productInterfaceFields[selector.Sel.Name] {
			return false
		}
		ident, ok := selector.X.(*ast.Ident)
		return !ok || !packages[ident.Name]
	}
	member := map[*ast.SelectorExpr]string{}
	var owners []*ast.SelectorExpr
	ast.Inspect(file, func(node ast.Node) bool {
		selector, ok := node.(*ast.SelectorExpr)
		if !ok {
			return true
		}
		if inner, ok := selector.X.(*ast.SelectorExpr); ok && owner(inner) {
			member[inner] = selector.Sel.Name
		}
		if owner(selector) {
			owners = append(owners, selector)
		}
		return true
	})
	var hits []productInterfaceHit
	for _, selector := range owners {
		name := selector.Sel.Name
		if method := member[selector]; method != "" {
			name += "." + method
		}
		hits = append(hits, productInterfaceHit{key: rel + "#" + enclosingDeclaration(file, selector.Pos()) + "#" + name, position: set.Position(selector.Pos()).String()})
	}
	return hits
}

// enclosingDeclaration names the top-level function or method containing pos.
func enclosingDeclaration(file *ast.File, pos token.Pos) string {
	for _, decl := range file.Decls {
		function, ok := decl.(*ast.FuncDecl)
		if !ok || pos < function.Pos() || pos > function.End() {
			continue
		}
		if function.Recv != nil && len(function.Recv.List) == 1 {
			receiver := function.Recv.List[0].Type
			if star, ok := receiver.(*ast.StarExpr); ok {
				receiver = star.X
			}
			if generic, ok := receiver.(*ast.IndexExpr); ok {
				receiver = generic.X
			}
			if generic, ok := receiver.(*ast.IndexListExpr); ok {
				receiver = generic.X
			}
			if ident, ok := receiver.(*ast.Ident); ok {
				return ident.Name + "." + function.Name.Name
			}
		}
		return function.Name.Name
	}
	return "<package>"
}

func TestHubEntriesUseProductServicesOnly(t *testing.T) {
	root := goRoot()
	seen := map[string]bool{}
	var violations []string
	for _, base := range productInterfaceRoots {
		err := filepath.WalkDir(filepath.Join(root, base), func(file string, entry fs.DirEntry, err error) error {
			if err != nil {
				return err
			}
			if entry.IsDir() {
				if entry.Name() == "testdata" || entry.Name() == "assets" {
					return filepath.SkipDir
				}
				return nil
			}
			if !strings.HasSuffix(file, ".go") || strings.HasSuffix(file, "_test.go") {
				return nil
			}
			rel, err := filepath.Rel(root, file)
			if err != nil {
				return err
			}
			rel = filepath.ToSlash(rel)
			set := token.NewFileSet()
			parsed, err := parser.ParseFile(set, file, nil, parser.SkipObjectResolution)
			if err != nil {
				return err
			}
			for _, hit := range findProductInterfaceSelections(set, parsed, rel) {
				seen[hit.key] = true
				if productInterfaceAllowlist[hit.key] == "" {
					violations = append(violations, fmt.Sprintf("%s (%s)", hit.position, hit.key))
				}
			}
			return nil
		})
		if err != nil && !os.IsNotExist(err) {
			t.Fatal(err)
		}
	}
	sort.Strings(violations)
	for _, violation := range violations {
		t.Errorf("direct product store/runtime/repository use %s: call a product service (Conversations, Turns, Teams, Automations, …) instead", violation)
	}
	for key := range productInterfaceAllowlist {
		if !seen[key] {
			t.Errorf("stale product-interface allowlist entry %s: remove it now that the selection is gone", key)
		}
	}
}

func TestProductInterfaceScannerRegressions(t *testing.T) {
	source := `package fixture
import (
	"github.com/QuincySx/Alva/integration/engine"
	hubcore "github.com/QuincySx/Alva/apps/hub"
)
var _ engine.Runtime
var _ *hubcore.Runtime
func (p *Page) load() {
	_, _ = p.Service.Store.Detail(ctx, id)
	_, _ = p.Service.Teams.Store.Team(ctx, lead, team)
	_ = p.Service.Runtime.HasActive()
	store, _ := p.Service.Store.(Catalog)
	_ = store
	_ = p.Service.Conversations.Detail(ctx, id)
	_ = p.store.Detail(ctx, id)
}
func free() { _, _ = app.Repository.Page(ctx) }
`
	set := token.NewFileSet()
	file, err := parser.ParseFile(set, "fixture.go", source, parser.SkipObjectResolution)
	if err != nil {
		t.Fatal(err)
	}
	var keys []string
	for _, hit := range findProductInterfaceSelections(set, file, "fixture.go") {
		keys = append(keys, hit.key)
	}
	want := []string{"fixture.go#Page.load#Store.Detail", "fixture.go#Page.load#Store.Team", "fixture.go#Page.load#Runtime.HasActive", "fixture.go#Page.load#Store", "fixture.go#free#Repository.Page"}
	if strings.Join(keys, ",") != strings.Join(want, ",") {
		t.Fatalf("scanner keys %v, want %v", keys, want)
	}
}
