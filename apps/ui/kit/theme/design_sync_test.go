// INPUT: tokens.json, docs/design/alva/alva-next.smallpen/tokens.css, source/next/token_names.json and (when python3 exists) source/next/build_next.py --check.
// OUTPUT: Proof, with no smallpen CLI, that tokens.json equals the design package's tokens.css (light, dark, Narrow, number and shadow Tokens) and that the package equals its generator.
// POS: [L7 Kit tests] CI guard for Token drift: editing only design_system.py, only tokens.css or only tokens.json fails here; the CLI-based exporter tests are skipped where smallpen is not installed.
package theme

import (
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"regexp"
	"strconv"
	"strings"
	"testing"
)

const designDir = "../../../../docs/design/alva"

type nameRow struct{ Kit, CSS, Type string }

func readNames(t *testing.T) (rows []nameRow, derived map[string]bool) {
	t.Helper()
	data, err := os.ReadFile(filepath.Join(designDir, "source/next/token_names.json"))
	if err != nil {
		t.Fatal(err)
	}
	var doc struct {
		Tokens  []nameRow
		Derived []string
	}
	if err = json.Unmarshal(data, &doc); err != nil {
		t.Fatal(err)
	}
	derived = map[string]bool{}
	for _, d := range doc.Derived {
		derived[d] = true
	}
	return doc.Tokens, derived
}

// cssBlocks returns the variables of every :root block of tokens.css, keyed by selector.
func cssBlocks(t *testing.T) map[string]map[string]string {
	t.Helper()
	data, err := os.ReadFile(filepath.Join(designDir, "alva-next.smallpen/tokens.css"))
	if err != nil {
		t.Fatal(err)
	}
	blocks := map[string]map[string]string{}
	for _, m := range regexp.MustCompile(`(?s)(:root[^{]*)\{(.*?)\}`).FindAllStringSubmatch(string(data), -1) {
		vars := map[string]string{}
		for _, line := range strings.Split(m[2], ";") {
			name, value, ok := strings.Cut(strings.TrimSpace(line), ":")
			if ok && strings.HasPrefix(name, "--") {
				vars[strings.TrimPrefix(name, "--")] = strings.TrimSpace(value)
			}
		}
		blocks[strings.TrimSpace(m[1])] = vars
	}
	return blocks
}

// kitValue converts a CSS value to the form tokens.json decodes to.
func kitValue(t *testing.T, kind, css string) any {
	t.Helper()
	num := func(s string) float64 {
		v, err := strconv.ParseFloat(s, 64)
		if err != nil {
			t.Fatalf("%q is not a number", css)
		}
		return v
	}
	switch kind {
	case "color":
		return css
	case "font-size", "spacing", "border-radius", "stroke-width", "sizing":
		return num(strings.TrimSuffix(css, "px"))
	case "font-weight":
		return num(css)
	case "font-family":
		return strings.Trim(css, `"`)
	case "number":
		return num(strings.Trim(css, `"`))
	case "boolean":
		return css == "true"
	case "shadow":
		f := strings.Fields(css)
		if len(f) != 5 {
			t.Fatalf("shadow %q", css)
		}
		return map[string]any{"offsetX": num(strings.TrimSuffix(f[0], "px")), "offsetY": num(strings.TrimSuffix(f[1], "px")),
			"blur": num(strings.TrimSuffix(f[2], "px")), "spread": num(strings.TrimSuffix(f[3], "px")), "color": f[4]}
	}
	t.Fatalf("unknown token type %q", kind)
	return nil
}

func decode(t *testing.T, raw json.RawMessage) any {
	t.Helper()
	var v any
	if err := json.Unmarshal(raw, &v); err != nil {
		t.Fatal(err)
	}
	return v
}

func TestTokensJSONMatchesTokensCSS(t *testing.T) {
	rows, derived := readNames(t)
	blocks := cssBlocks(t)
	root, dark, narrow := blocks[":root"], blocks[`:root[data-appearance="dark"]`], blocks[`:root[data-viewport="narrow"]`]
	if root == nil || dark == nil || narrow == nil {
		t.Fatalf("tokens.css lacks a :root, dark or narrow block: %v", blocks)
	}
	tokens := readTokens(t)
	if len(tokens) != len(rows) {
		t.Fatalf("tokens.json has %d tokens, token_names.json %d", len(tokens), len(rows))
	}
	inTable := map[string]bool{}
	for i, row := range rows {
		inTable[row.CSS] = true
		tok := tokens[i]
		if tok.Name != row.Kit || tok.Type != row.Type {
			t.Fatalf("token %d is %s/%s in tokens.json, %s/%s in token_names.json", i, tok.Name, tok.Type, row.Kit, row.Type)
		}
		css, ok := root[row.CSS]
		if !ok {
			t.Errorf("tokens.css has no --%s for %s", row.CSS, row.Kit)
			continue
		}
		if got, want := kitValue(t, row.Type, css), decode(t, tok.Value); !reflect.DeepEqual(got, want) {
			t.Errorf("%s: tokens.css gives %v, tokens.json %v", row.Kit, got, want)
		}
		for mode, set := range map[string]struct {
			block map[string]string
			raw   json.RawMessage
		}{"dark": {dark, tok.Dark}, "narrow": {narrow, tok.Narrow}} {
			css, has := set.block[row.CSS]
			switch {
			case has != (set.raw != nil):
				t.Errorf("%s: %s override is %v in tokens.css and %v in tokens.json", row.Kit, mode, has, set.raw != nil)
			case has:
				if got, want := kitValue(t, row.Type, css), decode(t, set.raw); !reflect.DeepEqual(got, want) {
					t.Errorf("%s %s: tokens.css gives %v, tokens.json %v", row.Kit, mode, got, want)
				}
			}
		}
	}
	for name := range root {
		if !inTable[name] && !derived[name] {
			t.Errorf("tokens.css has --%s, which is neither a Kit token nor a listed derived token", name)
		}
	}
	for name := range derived {
		if _, ok := root[name]; !ok {
			t.Errorf("derived token --%s is listed in token_names.json but missing from tokens.css", name)
		}
	}
	for mode, block := range map[string]map[string]string{"dark": dark, "narrow": narrow} {
		for name := range block {
			if !inTable[name] {
				t.Errorf("%s block overrides --%s, which is not a Kit token", mode, name)
			}
		}
	}
}

func TestDesignPackageMatchesItsGenerator(t *testing.T) {
	python, err := exec.LookPath("python3")
	if err != nil {
		t.Skip("python3 not found; the generator is a Python script")
	}
	script, err := filepath.Abs(filepath.Join(designDir, "source/next/build_next.py"))
	if err != nil {
		t.Fatal(err)
	}
	if out, err := exec.Command(python, "-I", script, "--check").CombinedOutput(); err != nil {
		t.Fatalf("alva-next.smallpen is not what build_next.py generates from design_system.py:\n%s", out)
	}
}
