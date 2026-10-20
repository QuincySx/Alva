// INPUT: Portable SDK/shared storage contract ASTs, reviewed golden snapshots and local update opt-in
// OUTPUT: Exported contract signature gate and regressions for field/type/generic/receiver/constant changes
// POS: Public SDK changes remain explicit review events instead of accidental product-driven API drift
package architecture_test

import (
	"bytes"
	"fmt"
	"go/ast"
	"go/format"
	"go/parser"
	"go/token"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"testing"
)

func exported(names []*ast.Ident) bool {
	for _, name := range names {
		if name.IsExported() {
			return true
		}
	}
	return false
}

func declarations(file *ast.File, set *token.FileSet) ([]string, error) {
	// Comments are documentation, not signatures. Preserve exported field tags,
	// generic bounds, aliases, receiver type parameters and interface seals.
	ast.Inspect(file, func(node ast.Node) bool {
		switch value := node.(type) {
		case *ast.FuncDecl:
			value.Doc = nil
		case *ast.GenDecl:
			value.Doc = nil
		case *ast.TypeSpec:
			value.Doc = nil
			value.Comment = nil
		case *ast.ValueSpec:
			value.Doc = nil
			value.Comment = nil
		case *ast.Field:
			value.Doc = nil
			value.Comment = nil
		}
		return true
	})
	var out []string
	emit := func(node any) error {
		var b bytes.Buffer
		if err := format.Node(&b, set, node); err != nil {
			return err
		}
		out = append(out, b.String())
		return nil
	}
	for _, raw := range file.Decls {
		switch decl := raw.(type) {
		case *ast.FuncDecl:
			if !decl.Name.IsExported() {
				continue
			}
			decl.Body = nil
			if err := emit(decl); err != nil {
				return nil, err
			}
		case *ast.GenDecl:
			switch decl.Tok {
			case token.IMPORT:
				// Import identities matter when an unchanged qualifier resolves to
				// a different package's exported types.
				if err := emit(decl); err != nil {
					return nil, err
				}
			case token.TYPE:
				for _, spec := range decl.Specs {
					typeSpec := spec.(*ast.TypeSpec)
					if !typeSpec.Name.IsExported() {
						continue
					}
					if fields, ok := typeSpec.Type.(*ast.StructType); ok {
						var visible []*ast.Field
						for _, field := range fields.Fields.List {
							if len(field.Names) == 0 || exported(field.Names) {
								visible = append(visible, field)
							}
						}
						fields.Fields.List = visible
					}
					if err := emit(&ast.GenDecl{Tok: token.TYPE, Specs: []ast.Spec{typeSpec}}); err != nil {
						return nil, err
					}
				}
			case token.CONST:
				// Keep the complete const group: private iota entries can alter
				// a later exported constant's actual numeric value.
				public := false
				for _, spec := range decl.Specs {
					public = public || exported(spec.(*ast.ValueSpec).Names)
				}
				if public {
					if err := emit(decl); err != nil {
						return nil, err
					}
				}
			case token.VAR:
				for _, spec := range decl.Specs {
					value := spec.(*ast.ValueSpec)
					if exported(value.Names) {
						if err := emit(&ast.GenDecl{Tok: token.VAR, Specs: []ast.Spec{value}}); err != nil {
							return nil, err
						}
					}
				}
			}
		}
	}
	return out, nil
}

func surface(directory string) (string, error) {
	entries, err := os.ReadDir(directory)
	if err != nil {
		return "", err
	}
	set := token.NewFileSet()
	seen := map[string]bool{}
	for _, entry := range entries {
		name := entry.Name()
		if entry.IsDir() || !strings.HasSuffix(name, ".go") || strings.HasSuffix(name, "_test.go") {
			continue
		}
		file, err := parser.ParseFile(set, filepath.Join(directory, name), nil, parser.ParseComments)
		if err != nil {
			return "", err
		}
		decls, err := declarations(file, set)
		if err != nil {
			return "", err
		}
		for _, decl := range decls {
			seen[decl] = true
		}
	}
	var ordered []string
	for decl := range seen {
		ordered = append(ordered, decl)
	}
	sort.Strings(ordered)
	return strings.Join(ordered, "\n\n") + "\n", nil
}

func TestPublicSDKSurfaceSnapshot(t *testing.T) {
	for _, pkg := range []string{"sdk/llm/wire", "sdk/kernel/bus", "sdk/kernel/abi", "sdk/kernel/core", "sdk/agent/abi", "sdk/agent/core", "sdk/sandbox/abi", "platform/storage", "platform/storage/blob"} {
		t.Run(pkg, func(t *testing.T) {
			actual, err := surface(filepath.Join(goRoot(), pkg))
			if err != nil {
				t.Fatal(err)
			}
			path := filepath.Join(goRoot(), "tests", "architecture", "testdata", "public-surface", strings.ReplaceAll(strings.TrimPrefix(pkg, "sdk/"), "/", "-")+".txt")
			if os.Getenv("ALVA_UPDATE_GO_SURFACE") == "1" {
				if err = os.WriteFile(path, []byte(actual), 0644); err != nil {
					t.Fatal(err)
				}
			}
			golden, err := os.ReadFile(path)
			if err != nil {
				t.Fatal("missing reviewed SDK surface; update explicitly with ALVA_UPDATE_GO_SURFACE=1", err)
			}
			if string(golden) != actual {
				t.Fatalf("%s exported surface changed; review responsibilities/consumers then explicitly update snapshot in the same commit", pkg)
			}
		})
	}
}

func TestSurfaceScannerDetectsContractChanges(t *testing.T) {
	scan := func(source string) string {
		t.Helper()
		set := token.NewFileSet()
		file, err := parser.ParseFile(set, "fixture.go", "package fixture\n"+source, parser.ParseComments)
		if err != nil {
			t.Fatal(err)
		}
		decls, err := declarations(file, set)
		if err != nil {
			t.Fatal(err)
		}
		return strings.Join(decls, "\n")
	}
	for _, pair := range [][2]string{
		{`type Public struct{ Value string }`, `type Public struct{ Value []byte }`},
		{`type Public struct{ Value string ` + "`json:\"one\"`" + ` }`, `type Public struct{ Value string ` + "`json:\"two\"`" + ` }`},
		{`type Public[T ~string] struct{ Value T }`, `type Public[T ~int] struct{ Value T }`},
		{`type Public interface{ sealed(); Value() string }`, `type Public interface{ Value() string }`},
		{`type Public = []byte`, `type Public []byte`},
		{`func (r *hidden[T]) Public(value T) error {return nil}`, `func (r *hidden[T]) Public(value []T) error {return nil}`},
		{`const ( private = iota; Public )`, `const ( private = iota; inserted; Public )`},
		{`import api "example.com/one"; func Public() api.Value {panic(0)}`, `import api "example.com/two"; func Public() api.Value {panic(0)}`},
	} {
		if scan(pair[0]) == scan(pair[1]) {
			t.Fatal("public change not detected", fmt.Sprint(pair))
		}
	}
	if scan(`type Public struct{ Value string; private int }`) != scan(`type Public struct{ Value string; private []byte }`) {
		t.Fatal("private implementation field leaked into exported surface")
	}
}
