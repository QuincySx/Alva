// INPUT: go/ast, go/parser, strconv, filesystem and non-test sources under apps/hub/product and apps/hub/desktop
// OUTPUT: Tool-name/engine-ID string literal rejection with a reasoned, stale-checked allowlist and scanner regressions
// POS: Keeps tool presentation in engine adapters (ADR 0013 §3): the Hub product and desktop render ToolDisplay kinds, never tool or engine names.
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
	"strconv"
	"strings"
	"testing"
)

var toolNameRoots = []string{"apps/hub/product", "apps/hub/desktop"}

// toolNameLiterals are the tool names the engine adapters classify plus the
// engine IDs. A string literal in product/desktop code equal to one of them
// means presentation or policy branches on a name instead of a capability.
var toolNameLiterals = map[string]bool{
	// Alva built-in tools
	"execute_shell": true, "read_file": true, "write_file": true, "edit_file": true, "search_files": true, "agent": true, "todo_write": true,
	// Codex item types (app-server and exec)
	"commandExecution": true, "command_execution": true, "fileChange": true, "file_change": true, "webSearch": true, "imageView": true, "collabToolCall": true,
	// Claude Code tools
	"Bash": true, "Read": true, "Write": true, "Edit": true, "MultiEdit": true, "Grep": true, "Glob": true, "WebSearch": true, "Task": true, "Agent": true, "TodoWrite": true,
	// Pi tools
	"bash": true, "read": true, "write": true, "edit": true, "grep": true, "find": true,
	// engine IDs
	"alva": true, "codex": true, "claude": true, "pi": true,
}

// Reasons shared by the pre-existing engine-ID gates found when this test was
// added. They are capability checks spelled as engine IDs; ADR 0013 §6
// replaces them with Engine Descriptor capability queries.
const (
	alvaOnlyGate    = "pre-existing Alva-only capability gate by engine ID (native store/approvals/waits); ADR 0013 §6 moves it to a capability query"
	engineLabel     = "pre-existing engine-ID to display-name/default mapping for author and engine picker labels, not tool presentation"
	connectionError = "connection storage operation word in an error, not a tool name"
	agentWord       = "UI copy/control key for the product's Agent picker, not the agent tool"
	productAction   = "action argument of the product-owned hub_history/hub_team tool it implements, not another tool's name"
)

// toolNameAllowlist holds reviewed literals, keyed path#literal#count, each
// with a one-line reason. The count is the exact number of occurrences of that
// literal in the file, so a further comparison in an already listed file is a
// violation. When occurrences drop, lower the count (or delete the entry at
// zero): a count above the actual number fails as stale. Counts only decrease.
var toolNameAllowlist = map[string]string{
	"apps/hub/desktop/media_desktop.go#alva#1":                         alvaOnlyGate,
	"apps/hub/desktop/tools_desktop.go#alva#1":                         alvaOnlyGate,
	"apps/hub/desktop/workspace_actions_desktop.go#alva#3":             alvaOnlyGate,
	"apps/hub/desktop/workspace_attachments_desktop.go#alva#2":         alvaOnlyGate,
	"apps/hub/desktop/workspace_composer_desktop.go#alva#5":            engineLabel,
	"apps/hub/desktop/workspace_engine_interactions_desktop.go#alva#1": alvaOnlyGate,
	"apps/hub/desktop/workspace_goals_desktop.go#alva#1":               alvaOnlyGate,
	"apps/hub/desktop/workspace_model_options_desktop.go#Agent#1":      agentWord,
	"apps/hub/desktop/workspace_models_desktop.go#agent#1":             agentWord,
	"apps/hub/desktop/workspace_permissions_desktop.go#alva#1":         alvaOnlyGate,
	"apps/hub/desktop/workspace_plan_desktop.go#alva#2":                alvaOnlyGate,
	"apps/hub/desktop/workspace_prepared_desktop.go#alva#1":            alvaOnlyGate,
	"apps/hub/desktop/workspace_shell_desktop.go#alva#1":               alvaOnlyGate,
	"apps/hub/desktop/workspace_transcript_desktop.go#alva#1":          engineLabel,
	"apps/hub/desktop/workspace_transcript_desktop.go#codex#1":         engineLabel,
	"apps/hub/desktop/workspace_transcript_desktop.go#claude#1":        engineLabel,
	"apps/hub/desktop/workspace_transcript_desktop.go#pi#1":            engineLabel,
	"apps/hub/product/approvals.go#alva#1":                             alvaOnlyGate,
	"apps/hub/product/companions.go#alva#2":                            alvaOnlyGate,
	"apps/hub/product/composer_commands.go#alva#3":                     alvaOnlyGate,
	"apps/hub/product/connection_catalog.go#read#1":                    connectionError,
	"apps/hub/product/connections.go#read#3":                           connectionError,
	"apps/hub/product/connections.go#alva#1":                           alvaOnlyGate,
	"apps/hub/product/engine_interactions.go#alva#2":                   alvaOnlyGate,
	"apps/hub/product/goals.go#alva#1":                                 alvaOnlyGate,
	"apps/hub/product/history.go#read#1":                               productAction,
	"apps/hub/product/plan_mode.go#alva#1":                             alvaOnlyGate,
	"apps/hub/product/questions.go#alva#1":                             alvaOnlyGate,
	"apps/hub/product/service.go#alva#4":                               alvaOnlyGate,
	"apps/hub/product/shell.go#alva#3":                                 alvaOnlyGate,
	"apps/hub/product/team_tool.go#read#2":                             productAction,
	"apps/hub/product/team_tool.go#edit#1":                             productAction,
	"apps/hub/product/wait.go#alva#1":                                  alvaOnlyGate,
	"apps/hub/product/workspace_references.go#alva#1":                  alvaOnlyGate,
	"apps/hub/product/workspace_views_resources.go#alva#1":             alvaOnlyGate,
}

type toolNameHit struct{ key, position string }

// findToolNameLiterals reports string literals whose value is exactly a
// forbidden name; substrings of larger literals (schemas, labels) do not count.
func findToolNameLiterals(set *token.FileSet, file *ast.File, rel string) []toolNameHit {
	var hits []toolNameHit
	ast.Inspect(file, func(node ast.Node) bool {
		literal, ok := node.(*ast.BasicLit)
		if !ok || literal.Kind != token.STRING {
			return true
		}
		value, err := strconv.Unquote(literal.Value)
		if err == nil && toolNameLiterals[value] {
			hits = append(hits, toolNameHit{key: rel + "#" + value, position: set.Position(literal.Pos()).String()})
		}
		return true
	})
	return hits
}

func TestHubProductAndDesktopNameNoToolsOrEngines(t *testing.T) {
	root := goRoot()
	counts := map[string]int{}
	positions := map[string][]string{}
	var violations []string
	for _, base := range toolNameRoots {
		err := filepath.WalkDir(filepath.Join(root, base), func(path string, entry fs.DirEntry, err error) error {
			if err != nil {
				return err
			}
			if entry.IsDir() {
				if entry.Name() == "testdata" || entry.Name() == "assets" {
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
			for _, hit := range findToolNameLiterals(set, file, rel) {
				counts[hit.key]++
				positions[hit.key] = append(positions[hit.key], hit.position)
			}
			return nil
		})
		if err != nil && !os.IsNotExist(err) {
			t.Fatal(err)
		}
	}
	allowed := parseToolNameAllowlist(t, toolNameAllowlist)
	for key, count := range counts {
		if count > allowed[key] {
			violations = append(violations, fmt.Sprintf("%s occurs %d times, allowlist permits %d (%s)", key, count, allowed[key], strings.Join(positions[key], ", ")))
		}
	}
	sort.Strings(violations)
	for _, violation := range violations {
		t.Errorf("tool or engine name literal %s: classify tools in the engine adapter's ToolDisplay and query engine capabilities, or add a reviewed allowlist reason", violation)
	}
	for key, permitted := range allowed {
		if counts[key] < permitted {
			t.Errorf("stale tool-name allowlist entry %s#%d: only %d occurrence(s) remain, lower the count or remove the entry", key, permitted, counts[key])
		}
	}
}

// parseToolNameAllowlist turns path#literal#count keys into path#literal ->
// count and rejects malformed or zero counts.
func parseToolNameAllowlist(t *testing.T, list map[string]string) map[string]int {
	t.Helper()
	result := make(map[string]int, len(list))
	for key, reason := range list {
		cut := strings.LastIndex(key, "#")
		count, err := strconv.Atoi(key[cut+1:])
		if cut < 0 || err != nil || count < 1 || reason == "" {
			t.Fatalf("allowlist key %q must be path#literal#count with count >= 1 and a reason", key)
		}
		if _, dup := result[key[:cut]]; dup {
			t.Fatalf("allowlist lists %q more than once", key[:cut])
		}
		result[key[:cut]] = count
	}
	return result
}

func TestToolNameScannerRegressions(t *testing.T) {
	source := "package fixture\n" +
		"var kinds = map[string]int{\"read_file\": 1, \"Bash\": 2}\n" +
		"func engine(id string) bool { return id == \"codex\" }\n" +
		"var raw = `edit`\n" +
		"var schema = `{\"enum\":[\"read\",\"write\"]}`\n" +
		"var label = \"读取 read_file\"\n" +
		"var other = \"reader\"\n"
	set := token.NewFileSet()
	file, err := parser.ParseFile(set, "fixture.go", source, parser.SkipObjectResolution)
	if err != nil {
		t.Fatal(err)
	}
	var keys []string
	for _, hit := range findToolNameLiterals(set, file, "fixture.go") {
		keys = append(keys, hit.key)
	}
	want := []string{"fixture.go#read_file", "fixture.go#Bash", "fixture.go#codex", "fixture.go#edit"}
	if strings.Join(keys, ",") != strings.Join(want, ",") {
		t.Fatalf("scanner keys %v, want %v", keys, want)
	}
}

func TestToolNameAllowlistIsCountedAndMonotonic(t *testing.T) {
	good := map[string]string{"apps/hub/product/a.go#alva#2": "reason"}
	if parseToolNameAllowlist(t, good)["apps/hub/product/a.go#alva"] != 2 {
		t.Fatal("count not parsed")
	}
	source := "package fixture\nfunc f(a, b, c string) bool { return a == \"alva\" || b == \"alva\" || c == \"alva\" }\n"
	set := token.NewFileSet()
	file, err := parser.ParseFile(set, "a.go", source, parser.SkipObjectResolution)
	if err != nil {
		t.Fatal(err)
	}
	counts := map[string]int{}
	for _, hit := range findToolNameLiterals(set, file, "apps/hub/product/a.go") {
		counts[hit.key]++
	}
	allowed := parseToolNameAllowlist(t, good)
	if counts["apps/hub/product/a.go#alva"] <= allowed["apps/hub/product/a.go#alva"] {
		t.Fatal("a third comparison in an already allowlisted file must exceed its registered count of two")
	}
}
