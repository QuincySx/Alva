// INPUT: icons.json, the embedded SVGs, the generated names and the design fonts directory in the repository.
// OUTPUT: Proof the manifest copy, embed, Name table and design SVGs agree, and that every icon is a Lucide 1.61.0 24-unit outline.
// POS: [L7 Kit tests] Pure Go: no MyGo, so the icon family is checked on every platform.
package icon

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

const designFonts = "../../../../docs/design/alva/fonts"

func TestManifestIsTheDesignManifest(t *testing.T) {
	ours, err := os.ReadFile("icons.json")
	if err != nil {
		t.Fatal(err)
	}
	design, err := os.ReadFile(filepath.Join(designFonts, "icons.json"))
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(ours, design) {
		t.Fatal("icons.json differs from docs/design/alva/fonts/icons.json: copy it and run go generate ./apps/ui/kit/icon")
	}
}

func TestEmbeddedFilesMatchManifestAndDesignSVGs(t *testing.T) {
	var manifest struct {
		Glyphs []struct{ Name, Lucide string }
	}
	data, _ := os.ReadFile("icons.json")
	if err := json.Unmarshal(data, &manifest); err != nil {
		t.Fatal(err)
	}
	want := map[string]bool{"LICENSE": true}
	for _, g := range manifest.Glyphs {
		want[g.Lucide+".svg"] = true
		svg, ok := Data(Name(g.Name))
		if !ok {
			t.Errorf("%s has no embedded SVG %s", g.Name, g.Lucide)
			continue
		}
		original, err := os.ReadFile(filepath.Join(designFonts, "lucide", g.Lucide+".svg"))
		if err != nil || !bytes.Equal(svg, original) {
			t.Errorf("%s: embedded SVG differs from the design's lucide/%s.svg", g.Name, g.Lucide)
		}
		if !bytes.Contains(svg, []byte(`viewBox="0 0 24 24"`)) || !bytes.Contains(svg, []byte("lucide-static v1.61.0")) {
			t.Errorf("%s is not a Lucide 1.61.0 24-unit SVG", g.Name)
		}
	}
	for _, f := range Files() {
		if !want[f] {
			t.Errorf("embedded %s is not in the design manifest: no second icon family", f)
		}
	}
	if len(Files()) != len(want) {
		t.Errorf("%d embedded files, manifest wants %d", len(Files()), len(want))
	}
}

func TestNamesCoverDesignIconAxis(t *testing.T) {
	var catalog struct {
		Components []struct {
			Name string
			Axes []struct {
				Name   string
				Values []string
			}
		}
	}
	data, err := os.ReadFile("../testdata/design-components.json")
	if err != nil {
		t.Fatal(err)
	}
	if err = json.Unmarshal(data, &catalog); err != nil {
		t.Fatal(err)
	}
	for _, c := range catalog.Components {
		if c.Name != "Icon" {
			continue
		}
		names := Names()
		if len(names) != len(c.Axes[0].Values) {
			t.Fatalf("%d names, design Icon axis has %d", len(names), len(c.Axes[0].Values))
		}
		for i, v := range c.Axes[0].Values {
			if string(names[i]) != v {
				t.Errorf("name %d is %s, design says %s", i, names[i], v)
			}
		}
		if !FlipX(PanelLeft) || FlipX(Panel) || strings.TrimSpace(string(Plus)) == "" {
			t.Error("PanelLeft is the only mirrored icon")
		}
		return
	}
	t.Fatal("design catalog has no Icon")
}

func TestUnknownNameHasNoData(t *testing.T) {
	if _, ok := Data(""); ok {
		t.Fatal("zero Name must mean no icon")
	}
	if _, ok := Data("Nope"); ok {
		t.Fatal("unknown name resolved")
	}
}
