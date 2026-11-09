// INPUT: go/ast, go/parser, filesystem and non-test sources under apps/hub/desktop and apps/direct/desktop
// OUTPUT: Shrink-only counts of color literals, shadows, native buttons, private workspaceControl definitions and hand-made popover/dialog shells
// POS: Keeps desktop code from growing new controls outside Alva Kit (ADR 0014 section 6); K4 migrates the existing ones and drives every count to zero.
package architecture_test

import (
	"fmt"
	"go/ast"
	"go/parser"
	"go/token"
	"io/fs"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"testing"
)

var kitAdoptionRoots = []string{hubDesktopRoot, directDesktopRoot}

const (
	adoptColor   = "ui color constructor (Hex|RGB|RGBA|Oklch)"
	adoptShadow  = ".Shadow"
	adoptButton  = "ui.ButtonBase|ui.Button"
	adoptControl = "workspaceControl*"
	adoptShell   = "ui.Popover*|ui.Dialog*|ui.AlertDialog*"
	adoptSelect  = "ui.Select*"
	adoptInput   = "ui.TextInput*|ui.TextArea*"
)

// uiImportName is the local name under which a file imports MyGo's ui package ("" when it does not),
// so aliased imports (import mu "github.com/egoist/mygo/ui") are still counted.
func uiImportName(file *ast.File) string {
	for _, spec := range file.Imports {
		if path, _ := strconv.Unquote(spec.Path.Value); path != "github.com/egoist/mygo/ui" {
			continue
		}
		if spec.Name != nil {
			return spec.Name.Name
		}
		return "ui"
	}
	return ""
}

// kitAdoptionCategory names the construct a node is, or "".
func kitAdoptionCategory(n ast.Node, uiName string) string {
	switch v := n.(type) {
	case *ast.FuncDecl:
		if strings.HasPrefix(v.Name.Name, "workspaceControl") {
			return adoptControl
		}
	case *ast.CallExpr:
		fun := v.Fun
		switch g := fun.(type) { // explicit generic instantiation: ui.SelectBase[int](...)
		case *ast.IndexExpr:
			fun = g.X
		case *ast.IndexListExpr:
			fun = g.X
		}
		sel, ok := fun.(*ast.SelectorExpr)
		if !ok {
			return ""
		}
		if pkg, ok := sel.X.(*ast.Ident); ok && uiName != "" && pkg.Name == uiName {
			name := sel.Sel.Name
			switch {
			case colorConstructors[name]:
				return adoptColor
			case name == "ButtonBase" || name == "Button":
				return adoptButton
			case strings.HasPrefix(name, "Popover") || strings.HasPrefix(name, "Dialog") || strings.HasPrefix(name, "AlertDialog"):
				return adoptShell
			case strings.HasPrefix(name, "Select"):
				return adoptSelect
			case strings.HasPrefix(name, "TextInput") || strings.HasPrefix(name, "TextArea"):
				return adoptInput
			}
			return ""
		}
		if sel.Sel.Name == "Shadow" {
			return adoptShadow
		}
	}
	return ""
}

// kitAdoptionCounts counts constructs per "path#category" in non-test desktop sources.
func kitAdoptionCounts(t *testing.T, roots []string) map[string]int {
	t.Helper()
	counts := map[string]int{}
	for _, root := range roots {
		err := filepath.WalkDir(filepath.Join(goRoot(), root), func(path string, entry fs.DirEntry, err error) error {
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
			uiName := uiImportName(file)
			ast.Inspect(file, func(n ast.Node) bool {
				if category := kitAdoptionCategory(n, uiName); category != "" {
					counts[filepath.ToSlash(rel)+"#"+category]++
				}
				return true
			})
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
	}
	return counts
}

// kitAdoptionAllowance is the reviewed count per file and construct that exists before the kit. Counts
// only decrease: a lower real count fails as stale (lower the number or delete the entry), a higher one
// is a new control outside the kit. K4 migrates these call sites and deletes every entry.
var kitAdoptionAllowance = map[string]int{
	"apps/direct/desktop/persistent_view_desktop.go#ui.ButtonBase|ui.Button":                            20,
	"apps/direct/desktop/persistent_view_desktop.go#ui.TextInput*|ui.TextArea*":                         5,
	"apps/hub/desktop/app_desktop.go#ui.ButtonBase|ui.Button":                                           2,
	"apps/hub/desktop/automation_desktop.go#ui.ButtonBase|ui.Button":                                    8,
	"apps/hub/desktop/automation_desktop.go#ui.TextInput*|ui.TextArea*":                                 3,
	"apps/hub/desktop/handoff_desktop.go#ui.ButtonBase|ui.Button":                                       1,
	"apps/hub/desktop/media_desktop.go#ui.ButtonBase|ui.Button":                                         14,
	"apps/hub/desktop/panes_desktop.go#ui.ButtonBase|ui.Button":                                         10,
	"apps/hub/desktop/panes_desktop.go#ui.TextInput*|ui.TextArea*":                                      3,
	"apps/hub/desktop/playback_desktop.go#ui.ButtonBase|ui.Button":                                      4,
	"apps/hub/desktop/playback_desktop.go#ui.TextInput*|ui.TextArea*":                                   2,
	"apps/hub/desktop/preferences_desktop.go#ui color constructor (Hex|RGB|RGBA|Oklch)":                 24,
	"apps/hub/desktop/team_desktop.go#ui.ButtonBase|ui.Button":                                          17,
	"apps/hub/desktop/team_desktop.go#ui.TextInput*|ui.TextArea*":                                       3,
	"apps/hub/desktop/tools_desktop.go#ui.ButtonBase|ui.Button":                                         6,
	"apps/hub/desktop/tools_desktop.go#ui.TextInput*|ui.TextArea*":                                      1,
	"apps/hub/desktop/wait_desktop.go#ui.ButtonBase|ui.Button":                                          4,
	"apps/hub/desktop/wait_desktop.go#ui.TextInput*|ui.TextArea*":                                       5,
	"apps/hub/desktop/workspace_assets_desktop.go#ui color constructor (Hex|RGB|RGBA|Oklch)":            14,
	"apps/hub/desktop/workspace_automation_index_desktop.go#ui.ButtonBase|ui.Button":                    1,
	"apps/hub/desktop/workspace_chart_blocks_desktop.go#ui color constructor (Hex|RGB|RGBA|Oklch)":      16,
	"apps/hub/desktop/workspace_chart_blocks_desktop.go#ui.ButtonBase|ui.Button":                        1,
	"apps/hub/desktop/workspace_composer_desktop.go#.Shadow":                                            1,
	"apps/hub/desktop/workspace_composer_desktop.go#ui color constructor (Hex|RGB|RGBA|Oklch)":          1,
	"apps/hub/desktop/workspace_composer_desktop.go#ui.ButtonBase|ui.Button":                            4,
	"apps/hub/desktop/workspace_composer_desktop.go#ui.Popover*|ui.Dialog*|ui.AlertDialog*":             1,
	"apps/hub/desktop/workspace_composer_desktop.go#ui.TextInput*|ui.TextArea*":                         1,
	"apps/hub/desktop/workspace_connections_desktop.go#ui.ButtonBase|ui.Button":                         2,
	"apps/hub/desktop/workspace_context_meter_desktop.go#.Shadow":                                       1,
	"apps/hub/desktop/workspace_context_meter_desktop.go#ui color constructor (Hex|RGB|RGBA|Oklch)":     1,
	"apps/hub/desktop/workspace_context_meter_desktop.go#ui.ButtonBase|ui.Button":                       1,
	"apps/hub/desktop/workspace_context_meter_desktop.go#ui.Popover*|ui.Dialog*|ui.AlertDialog*":        1,
	"apps/hub/desktop/workspace_controls_desktop.go#ui.ButtonBase|ui.Button":                            2,
	"apps/hub/desktop/workspace_controls_desktop.go#ui.Select*":                                         1,
	"apps/hub/desktop/workspace_controls_desktop.go#ui.TextInput*|ui.TextArea*":                         3,
	"apps/hub/desktop/workspace_controls_desktop.go#workspaceControl*":                                  16,
	"apps/hub/desktop/workspace_drafts_desktop.go#ui.ButtonBase|ui.Button":                              2,
	"apps/hub/desktop/workspace_goals_desktop.go#ui color constructor (Hex|RGB|RGBA|Oklch)":             1,
	"apps/hub/desktop/workspace_goals_desktop.go#ui.ButtonBase|ui.Button":                               1,
	"apps/hub/desktop/workspace_goals_desktop.go#ui.Popover*|ui.Dialog*|ui.AlertDialog*":                1,
	"apps/hub/desktop/workspace_interactions_desktop.go#ui.ButtonBase|ui.Button":                        2,
	"apps/hub/desktop/workspace_model_options_desktop.go#ui.ButtonBase|ui.Button":                       1,
	"apps/hub/desktop/workspace_model_options_desktop.go#ui.Popover*|ui.Dialog*|ui.AlertDialog*":        1,
	"apps/hub/desktop/workspace_model_options_desktop.go#ui.Select*":                                    1,
	"apps/hub/desktop/workspace_model_preferences_desktop.go#ui color constructor (Hex|RGB|RGBA|Oklch)": 1,
	"apps/hub/desktop/workspace_model_preferences_desktop.go#ui.Popover*|ui.Dialog*|ui.AlertDialog*":    1,
	"apps/hub/desktop/workspace_models_desktop.go#.Shadow":                                              1,
	"apps/hub/desktop/workspace_models_desktop.go#ui color constructor (Hex|RGB|RGBA|Oklch)":            1,
	"apps/hub/desktop/workspace_models_desktop.go#ui.ButtonBase|ui.Button":                              4,
	"apps/hub/desktop/workspace_models_desktop.go#ui.Popover*|ui.Dialog*|ui.AlertDialog*":               1,
	"apps/hub/desktop/workspace_models_desktop.go#ui.TextInput*|ui.TextArea*":                           1,
	"apps/hub/desktop/workspace_picker_desktop.go#ui.Popover*|ui.Dialog*|ui.AlertDialog*":               1,
	"apps/hub/desktop/workspace_picker_desktop.go#ui.TextInput*|ui.TextArea*":                           1,
	"apps/hub/desktop/workspace_resources_desktop.go#ui.ButtonBase|ui.Button":                           2,
	"apps/hub/desktop/workspace_routing_desktop.go#ui.ButtonBase|ui.Button":                             3,
	"apps/hub/desktop/workspace_search_desktop.go#ui color constructor (Hex|RGB|RGBA|Oklch)":            1,
	"apps/hub/desktop/workspace_search_desktop.go#ui.ButtonBase|ui.Button":                              1,
	"apps/hub/desktop/workspace_search_desktop.go#ui.Popover*|ui.Dialog*|ui.AlertDialog*":               1,
	"apps/hub/desktop/workspace_settings_desktop.go#ui.ButtonBase|ui.Button":                            1,
	"apps/hub/desktop/workspace_shell_desktop.go#ui.ButtonBase|ui.Button":                               4,
	"apps/hub/desktop/workspace_tool_blocks_desktop.go#ui.ButtonBase|ui.Button":                         7,
	"apps/hub/desktop/workspace_trace_desktop.go#ui.ButtonBase|ui.Button":                               5,
	"apps/hub/desktop/workspace_voice_desktop.go#ui.ButtonBase|ui.Button":                               1,
	"apps/hub/desktop/worktree_desktop.go#ui.ButtonBase|ui.Button":                                      10,
}

func TestDesktopControlsDoNotGrowOutsideTheKit(t *testing.T) {
	counts := kitAdoptionCounts(t, kitAdoptionRoots)
	var failures []string
	for key, got := range counts {
		if allowed := kitAdoptionAllowance[key]; got > allowed {
			failures = append(failures, fmt.Sprintf("%s: %d uses, allowance %d: build it with apps/ui/kit instead of adding another", key, got, allowed))
		}
	}
	for key, allowed := range kitAdoptionAllowance {
		if got := counts[key]; got < allowed {
			failures = append(failures, fmt.Sprintf("%s: allowance %d is stale, only %d remain: lower it", key, allowed, got))
		}
	}
	sort.Strings(failures)
	for _, failure := range failures {
		t.Error(failure)
	}
}

func TestAdoptionScannerClassifiesConstructs(t *testing.T) {
	src := `package p
import mu "github.com/egoist/mygo/ui"
func workspaceControlFoo() {}
func other(e mu.Element) {
	mu.Hex("#fff")
	mu.RGBA(1, 2, 3, .5)
	mu.Oklch(1, 2, 3)
	mu.ButtonBase(c)
	mu.Button(c, "x")
	mu.PopoverBase(c, a, o, f)
	mu.Popover(c, a, o, f)
	mu.DialogBase(c, o, f)
	mu.Select(c, v)
	mu.SelectBase(c, v)
	mu.SelectBase[int](c, v)
	mu.TextInputBase(c, v)
	mu.TextAreaBase(c, v)
	e.Shadow(0, 1, 2, 0, col)
	mu.Text(c, "plain")
}`
	file, err := parser.ParseFile(token.NewFileSet(), "x.go", src, 0)
	if err != nil {
		t.Fatal(err)
	}
	name := uiImportName(file)
	if name != "mu" {
		t.Fatal("alias import not resolved", name)
	}
	got := map[string]int{}
	ast.Inspect(file, func(n ast.Node) bool {
		if category := kitAdoptionCategory(n, name); category != "" {
			got[category]++
		}
		return true
	})
	want := map[string]int{adoptControl: 1, adoptColor: 3, adoptButton: 2, adoptShell: 3, adoptSelect: 3, adoptInput: 2, adoptShadow: 1}
	if fmt.Sprint(got) != fmt.Sprint(want) {
		t.Fatalf("classified %v, want %v", got, want)
	}
	// Without the ui import nothing counts, so an unrelated package named ui is not flagged.
	if kitAdoptionCategory(file.Decls[2].(*ast.FuncDecl).Body.List[0].(*ast.ExprStmt).X, "") != "" {
		t.Fatal("counted a call without a ui import")
	}
}
