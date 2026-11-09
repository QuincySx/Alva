// INPUT: icons.json in the working directory (a copy of docs/design/alva/fonts/icons.json); go generate runs inside apps/ui/kit/icon.
// OUTPUT: names_gen.go: the Name constants in design order and their Lucide file names.
// POS: [L7 Kit] Deterministic icon-name generator; the Go build never needs Python, tests tie the table to the manifest and embed.
package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"go/format"
	"os"
)

func main() {
	data, err := os.ReadFile("icons.json")
	if err != nil {
		fail(err)
	}
	out, err := generate(data)
	if err != nil {
		fail(err)
	}
	if err = os.WriteFile("names_gen.go", out, 0o644); err != nil {
		fail(err)
	}
}

func fail(err error) {
	fmt.Fprintln(os.Stderr, "gen:", err)
	os.Exit(1)
}

func generate(data []byte) ([]byte, error) {
	var manifest struct {
		Provenance string
		Glyphs     []struct{ Name, Lucide string }
	}
	if err := json.Unmarshal(data, &manifest); err != nil {
		return nil, err
	}
	if len(manifest.Glyphs) == 0 {
		return nil, fmt.Errorf("icons.json has no glyphs")
	}
	var b bytes.Buffer
	b.WriteString("// INPUT: icons.json, the design interface icon manifest (Lucide 1.61.0 subset).\n")
	b.WriteString("// OUTPUT: Name constants in design order and the Lucide file behind each. Generated: do not edit.\n")
	b.WriteString("// POS: [L7 Kit] Single Go source of icon names; regenerate with go generate ./apps/ui/kit/icon.\n\n")
	b.WriteString("package icon\n\n")
	b.WriteString("// Design icon names (Alva Icon.Name axis). PanelLeft is the one derived name: Panel mirrored horizontally.\nconst (\n")
	seen := map[string]bool{}
	for _, g := range manifest.Glyphs {
		if g.Name == "" || g.Lucide == "" || seen[g.Name] {
			return nil, fmt.Errorf("invalid or duplicate glyph %q", g.Name)
		}
		seen[g.Name] = true
		fmt.Fprintf(&b, "\t%s Name = %q\n", g.Name, g.Name)
	}
	b.WriteString("\tPanelLeft Name = \"PanelLeft\"\n)\n\n")
	b.WriteString("// lucideFile maps a design name to its file under lucide/ (without extension).\nvar lucideFile = map[Name]string{\n")
	for _, g := range manifest.Glyphs {
		fmt.Fprintf(&b, "\t%s: %q,\n", g.Name, g.Lucide)
	}
	b.WriteString("\tPanelLeft: \"panel-right\",\n}\n\n")
	b.WriteString("// designOrder lists the manifest names in design order, PanelLeft last.\nvar designOrder = []Name{\n")
	for _, g := range manifest.Glyphs {
		fmt.Fprintf(&b, "\t%s,\n", g.Name)
	}
	b.WriteString("\tPanelLeft,\n}\n")
	return format.Source(b.Bytes())
}
