// INPUT: go/ast, go/parser, filesystem and non-test sources under apps/extension/harness/integration/platform/host
// OUTPUT: Whole-session read rejection with a reasoned, stale-checked allowlist and scanner regressions
// POS: Keeps long-session consumers on bounded tail/page/latest-record reads instead of loading complete history.
package architecture_test

import (
	"fmt"
	"go/ast"
	"go/parser"
	"go/token"
	"io/fs"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"testing"
)

// Roots whose consumers may face multi-day, gigabyte-sized native sessions.
var boundedReadRoots = []string{"apps", "extension", "harness", "integration", "platform/host"}

// boundedReadAllowlist holds reviewed calls that are bounded or only touch a
// small private session, each with a one-line reason. Key: slash
// path#enclosing function#callee. Empty since shell recovery and AEP state
// reads moved to LatestRecord/Page and BoundConversationTail.
var boundedReadAllowlist = map[string]string{}

type unboundedRead struct {
	key, position string
}

// findUnboundedReads reports `x.Messages(arg)` calls and `x.Records(arg, 0)`
// calls with a literal zero cursor. It is name-based on purpose: a new
// whole-history reader under a reused name must be reviewed, not inferred safe.
func findUnboundedReads(set *token.FileSet, file *ast.File, rel string) []unboundedRead {
	var found []unboundedRead
	enclosing := func(pos token.Pos) string {
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
	ast.Inspect(file, func(node ast.Node) bool {
		call, ok := node.(*ast.CallExpr)
		if !ok {
			return true
		}
		selector, ok := call.Fun.(*ast.SelectorExpr)
		if !ok {
			return true
		}
		hit := false
		switch selector.Sel.Name {
		case "Messages":
			hit = len(call.Args) == 1
		case "Records":
			if len(call.Args) == 2 {
				literal, ok := call.Args[1].(*ast.BasicLit)
				hit = ok && literal.Kind == token.INT && literal.Value == "0"
			}
		}
		if hit {
			found = append(found, unboundedRead{key: rel + "#" + enclosing(call.Pos()) + "#" + selector.Sel.Name, position: set.Position(call.Pos()).String()})
		}
		return true
	})
	return found
}

func TestNoNewUnboundedSessionReads(t *testing.T) {
	root := goRoot()
	seen := map[string]bool{}
	var violations []string
	for _, base := range boundedReadRoots {
		err := filepath.WalkDir(filepath.Join(root, base), func(path string, entry fs.DirEntry, err error) error {
			if err != nil {
				return err
			}
			if entry.IsDir() {
				if entry.Name() == "build" || entry.Name() == ".mygo" || entry.Name() == "vendor" || entry.Name() == "testdata" {
					return filepath.SkipDir
				}
				return nil
			}
			if !strings.HasSuffix(path, ".go") || strings.HasSuffix(path, "_test.go") {
				return nil
			}
			rel, err := filepath.Rel(root, path)
			if err != nil {
				return err
			}
			rel = filepath.ToSlash(rel)
			set := token.NewFileSet()
			file, err := parser.ParseFile(set, path, nil, parser.SkipObjectResolution)
			if err != nil {
				return err
			}
			for _, read := range findUnboundedReads(set, file, rel) {
				seen[read.key] = true
				if boundedReadAllowlist[read.key] == "" {
					violations = append(violations, fmt.Sprintf("%s (%s)", read.position, read.key))
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
		t.Errorf("unbounded session read %s: use BoundConversationTail, Page or LatestRecord, or add a reviewed allowlist reason", violation)
	}
	for key := range boundedReadAllowlist {
		if !seen[key] {
			t.Errorf("stale bounded-read allowlist entry %s: remove it now that the call is gone", key)
		}
	}
}

func TestUnboundedReadScannerRegressions(t *testing.T) {
	source := `package fixture
func (s *Service) Load(ctx context.Context) {
	_, _ = s.log.Messages(ctx)
	_, _ = s.log.Records(ctx, 0)
}
func bounded(ctx context.Context) {
	_, _ = log.Records(ctx, after)
	_, _ = log.Records(ctx, 1)
	_ = message.Messages()
	_, _ = tail.Read(ctx, 20, 1<<20)
	_, _ = repo.Page(ctx, "owner", "id", 0, 100, 0)
}
func generic[T any](ctx context.Context) { _, _ = view.Messages(ctx) }
`
	set := token.NewFileSet()
	file, err := parser.ParseFile(set, "fixture.go", source, parser.SkipObjectResolution)
	if err != nil {
		t.Fatal(err)
	}
	var keys []string
	for _, read := range findUnboundedReads(set, file, "fixture.go") {
		keys = append(keys, read.key)
	}
	want := []string{"fixture.go#Service.Load#Messages", "fixture.go#Service.Load#Records", "fixture.go#generic#Messages"}
	if strings.Join(keys, ",") != strings.Join(want, ",") {
		t.Fatalf("scanner keys %v, want %v", keys, want)
	}
}
