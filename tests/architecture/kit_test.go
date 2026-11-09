// INPUT: go/ast, go/parser, strconv, filesystem and non-test sources under apps/ui/kit
// OUTPUT: Literal rejection (colors, radii, sizes must come from theme), product-vocabulary rejection, framework-free data-file proof and scanner regressions
// POS: Keeps Alva Kit (ADR 0014) a pure token-driven skin: no color or size literal in a component, no tool or engine name, theme data files free of MyGo.
package architecture_test

import (
	"go/ast"
	"go/parser"
	"go/token"
	"io/fs"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
)

// kitSources returns the parsed non-test Go files under apps/ui/kit, keyed by slash path relative to the repository.
func kitSources(t *testing.T) map[string]*ast.File {
	t.Helper()
	out := map[string]*ast.File{}
	root := filepath.Join(goRoot(), sharedKitRoot)
	err := filepath.WalkDir(root, func(path string, entry fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if entry.IsDir() || !strings.HasSuffix(path, ".go") || strings.HasSuffix(path, "_test.go") {
			return nil
		}
		file, err := parser.ParseFile(token.NewFileSet(), path, nil, parser.SkipObjectResolution)
		if err != nil {
			return err
		}
		rel, _ := filepath.Rel(goRoot(), path)
		out[filepath.ToSlash(rel)] = file
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	if len(out) == 0 {
		t.Fatal("no kit sources found")
	}
	return out
}

// colorConstructors build colors from literals; components take colors from theme.Palette only.
var colorConstructors = map[string]bool{"Hex": true, "RGB": true, "RGBA": true, "Oklch": true}

// sizeMethods take pixel, opacity, ratio or offset values; a numeric literal other than 0 or 1 (a hairline or a
// flex factor) must be a theme token or a named constant.
var sizeMethods = map[string]bool{
	"Radius": true, "Height": true, "Width": true, "Padding": true, "PaddingX": true, "PaddingY": true, "PaddingStart": true, "PaddingEnd": true,
	"Gap": true, "GapX": true, "GapY": true, "FontSize": true, "FontWeight": true, "LineHeight": true, "LetterSpacing": true, "Size": true,
	"MinHeight": true, "MinWidth": true, "MaxWidth": true, "MaxHeight": true, "Margin": true, "MarginX": true, "MarginY": true,
	"MarginStart": true, "MarginEnd": true, "Border": true, "Stroke": true, "Opacity": true, "Shadow": true, "InsetShadow": true, "Blur": true,
	"Top": true, "Right": true, "Bottom": true, "Left": true, "Scale": true, "Grow": true, "Shrink": true, "Basis": true, "Rotate": true, "Translate": true,
}

// findNumeric returns a numeric literal anywhere in an argument expression: bare, signed, inside arithmetic or inside a conversion such as float32(8).
func findNumeric(e ast.Expr) (string, bool) {
	switch v := e.(type) {
	case *ast.BasicLit:
		if v.Kind == token.INT || v.Kind == token.FLOAT {
			return v.Value, true
		}
	case *ast.UnaryExpr:
		lit, ok := findNumeric(v.X)
		if ok && v.Op == token.SUB {
			lit = "-" + lit
		}
		return lit, ok
	case *ast.ParenExpr:
		return findNumeric(v.X)
	case *ast.BinaryExpr:
		if lit, ok := findNumeric(v.X); ok {
			return lit, true
		}
		return findNumeric(v.Y)
	case *ast.CallExpr:
		if _, conversion := v.Fun.(*ast.Ident); conversion && len(v.Args) == 1 {
			return findNumeric(v.Args[0])
		}
	}
	return "", false
}

// literalViolations reports color constructors, Color literals, ui.White/ui.Black and magic size literals in one file.
func literalViolations(name string, file *ast.File) []string {
	var out []string
	themePkg := under(name, sharedKitRoot+"/theme")
	ast.Inspect(file, func(n ast.Node) bool {
		switch v := n.(type) {
		case *ast.CompositeLit:
			if sel, ok := v.Type.(*ast.SelectorExpr); ok && !themePkg {
				if pkg, ok := sel.X.(*ast.Ident); ok && pkg.Name == "ui" && sel.Sel.Name == "Color" {
					out = append(out, name+": ui.Color{} literal builds a color; read theme.Palette")
				}
			}
		case *ast.SelectorExpr:
			if pkg, ok := v.X.(*ast.Ident); ok && pkg.Name == "ui" && (v.Sel.Name == "White" || v.Sel.Name == "Black") {
				out = append(out, name+": ui."+v.Sel.Name+" is a color literal; read theme.Palette")
			}
		case *ast.CallExpr:
			sel, ok := v.Fun.(*ast.SelectorExpr)
			if !ok {
				return true
			}
			if pkg, ok := sel.X.(*ast.Ident); ok && pkg.Name == "ui" && colorConstructors[sel.Sel.Name] && !under(name, sharedKitRoot+"/kittest") {
				// kittest rebuilds colors from sampled pixel bytes, not from design values.
				out = append(out, name+": ui."+sel.Sel.Name+" builds a color from a literal; read theme.Palette")
			}
			if sizeMethods[sel.Sel.Name] {
				for _, arg := range v.Args {
					if lit, ok := findNumeric(arg); ok {
						if value, err := strconv.ParseFloat(lit, 64); err == nil && value != 0 && value != 1 {
							out = append(out, name+": "+sel.Sel.Name+"(... "+lit+" ...) is a literal; use a theme token or a named constant")
						}
					}
				}
			}
		}
		return true
	})
	return out
}

func TestKitHasNoLiterals(t *testing.T) {
	for name, file := range kitSources(t) {
		if under(name, sharedKitRoot+"/theme") && strings.HasSuffix(name, "_gen.go") {
			continue // the generated tokens are the one place design values are written
		}
		for _, violation := range literalViolations(name, file) {
			t.Error(violation)
		}
	}
}

func TestKitLiteralScannerCatchesColorsAndSizes(t *testing.T) {
	src := `package kit
import "github.com/egoist/mygo/ui"
func f(e ui.Element) {
	e.Background(ui.Hex("#fff"))
	e.Foreground(ui.RGB(1, 2, 3))
	e.Radius(8)
	e.Height(32).Padding(0, 12)
	e.Gap(1)
	e.Padding(theme.Space2)
	e.Opacity(.96)
	e.Width(theme.ControlHeight * 2)
	e.Width(float32(40))
	e.Margin(-6)
	e.Scale(-1, 1)
	_ = ui.Color{R: 1}
	e.TextColor(ui.White)
	e.Shadow(0, 8, 30, -6, c)
}`
	file, err := parser.ParseFile(token.NewFileSet(), "x.go", src, 0)
	if err != nil {
		t.Fatal(err)
	}
	got := strings.Join(literalViolations("x.go", file), "\n")
	for _, want := range []string{"ui.Hex", "ui.RGB", "Radius(... 8", "Height(... 32", "Padding(... 12", "Opacity(... .96", "Width(... 2", "Width(... 40", "Margin(... -6", "Scale(... -1", "ui.Color{}", "ui.White", "Shadow(... 8"} {
		if !strings.Contains(got, want) {
			t.Errorf("scanner missed %q in\n%s", want, got)
		}
	}
	if strings.Contains(got, "Gap(") || strings.Contains(got, "Padding(... 2") {
		t.Errorf("scanner flagged an allowed call:\n%s", got)
	}
}

// kitVocabularyExempt names kit files whose string literals may equal a tool or engine name, with the reason.
var kitVocabularyExempt = map[string]string{
	sharedKitRoot + "/icon/names_gen.go":        "design glyph names (Edit, Task, Agent) are icon identifiers, not tool or engine names",
	sharedKitRoot + "/display_desktop.go#Agent": "AvatarKind.String returns the design's Kind axis value; Agent names an avatar shape, not an engine",
}

func TestKitHasNoProductVocabulary(t *testing.T) {
	for name, file := range kitSources(t) {
		if _, exempt := kitVocabularyExempt[name]; exempt {
			continue
		}
		ast.Inspect(file, func(n ast.Node) bool {
			lit, ok := n.(*ast.BasicLit)
			if !ok || lit.Kind != token.STRING {
				return true
			}
			if value, err := strconv.Unquote(lit.Value); err == nil && toolNameLiterals[value] {
				if _, exempt := kitVocabularyExempt[name+"#"+value]; exempt {
					return true
				}
				t.Errorf("%s: string literal %q is a tool or engine name; the kit renders kinds and props, never names (ADR 0013 section 3)", name, value)
			}
			return true
		})
	}
	for name := range kitVocabularyExempt {
		if _, found := kitSources(t)[strings.SplitN(name, "#", 2)[0]]; !found {
			t.Errorf("stale vocabulary exemption %s", name)
		}
	}
}

func TestKitThemeDataFilesAreFrameworkFree(t *testing.T) {
	for name, file := range kitSources(t) {
		if !under(name, sharedKitRoot+"/theme") && !under(name, sharedKitRoot+"/icon") {
			continue
		}
		desktopOnly := strings.HasSuffix(name, "_desktop.go")
		for _, spec := range file.Imports {
			path, _ := strconv.Unquote(spec.Path.Value)
			if under(path, "github.com/egoist/mygo") && !(desktopOnly && under(name, sharedKitRoot+"/theme")) {
				t.Errorf("%s imports %s: only theme's *_desktop.go mapping may touch MyGo; token and icon data must build without it", name, path)
			}
		}
	}
}

// shadowViolations reports every Shadow or InsetShadow call outside the receiver-less popupShadow function of
// kit_desktop.go, and a popupShadow that does not read the Popup token.
func shadowViolations(name string, file *ast.File) []string {
	var out []string
	for _, decl := range file.Decls {
		fn, isFunc := decl.(*ast.FuncDecl)
		allowed := isFunc && fn.Name.Name == "popupShadow" && fn.Recv == nil && name == sharedKitRoot+"/kit_desktop.go"
		usesToken, calls := false, 0
		ast.Inspect(decl, func(n ast.Node) bool {
			switch v := n.(type) {
			case *ast.SelectorExpr:
				usesToken = usesToken || v.Sel.Name == "Popup"
			case *ast.CallExpr:
				if sel, ok := v.Fun.(*ast.SelectorExpr); ok && (sel.Sel.Name == "Shadow" || sel.Sel.Name == "InsetShadow") {
					calls++
					if !allowed {
						out = append(out, name+": "+sel.Sel.Name+" called outside popupShadow; use popupShadow/popupSurface so shadow.popup is the only source")
					}
				}
			}
			return true
		})
		if allowed && (calls != 1 || !usesToken) {
			out = append(out, name+": popupShadow must make exactly one Shadow call fed by the Palette.Popup token")
		}
	}
	return out
}

func TestKitShadowComesOnlyFromThePopupToken(t *testing.T) {
	seen := false
	for name, file := range kitSources(t) {
		for _, violation := range shadowViolations(name, file) {
			t.Error(violation)
		}
		seen = seen || name == sharedKitRoot+"/kit_desktop.go"
	}
	if !seen {
		t.Fatal("kit_desktop.go not scanned")
	}
}

func TestKitShadowScannerCatchesBypasses(t *testing.T) {
	for name, src := range map[string]string{
		"inset":     `package kit; func f(e E) { e.InsetShadow(0, 1, 2, 0, c) }`,
		"method":    `package kit; func (x T) popupShadow(e E) { e.Shadow(0, 1, 2, 0, c) }`,
		"elsewhere": `package kit; func other(e E) { e.Shadow(0, 1, 2, 0, c) }`,
	} {
		file, err := parser.ParseFile(token.NewFileSet(), "x.go", src, 0)
		if err != nil {
			t.Fatal(err)
		}
		if len(shadowViolations(sharedKitRoot+"/kit_desktop.go", file)) == 0 {
			t.Errorf("%s: bypass accepted", name)
		}
	}
	realShape := `package kit; func popupShadow(e E, p P) E { s := p.Popup; return e.Shadow(s.X, s.Y, s.Blur, s.Spread, c) }`
	file, _ := parser.ParseFile(token2(), "x.go", realShape, 0)
	if v := shadowViolations(sharedKitRoot+"/kit_desktop.go", file); len(v) != 0 {
		t.Errorf("the real shape was rejected: %v", v)
	}
	hardCoded := `package kit; func popupShadow(e E, p P) E { return e.Shadow(0, 8, 30, -6, c) }`
	file, _ = parser.ParseFile(token2(), "x.go", hardCoded, 0)
	if len(shadowViolations(sharedKitRoot+"/kit_desktop.go", file)) == 0 {
		t.Error("a popupShadow that ignores the token was accepted")
	}
}

func token2() *token.FileSet { return token.NewFileSet() }
