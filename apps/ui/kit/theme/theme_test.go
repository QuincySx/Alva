// INPUT: tokens.json, the generated Palette, the design source exporter (when python3 exists) and testdata/design-components.json.
// OUTPUT: Proof JSON equals the design source, Palette covers every design color in both appearances, and narrow values resolve.
// POS: [L7 Kit tests] Pure Go except the optional exporter comparison; no MyGo or build tag.
package theme

import (
	"bytes"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
)

type tokenJSON struct {
	Name   string          `json:"name"`
	Type   string          `json:"type"`
	Value  json.RawMessage `json:"value"`
	Dark   json.RawMessage `json:"dark"`
	Narrow json.RawMessage `json:"narrow"`
}

func readTokens(t *testing.T) []tokenJSON {
	t.Helper()
	data, err := os.ReadFile("tokens.json")
	if err != nil {
		t.Fatal(err)
	}
	var doc struct{ Tokens []tokenJSON }
	if err = json.Unmarshal(data, &doc); err != nil {
		t.Fatal(err)
	}
	return doc.Tokens
}

func hexOf(c Color) string {
	if c.A == 0xFF {
		return fmt.Sprintf("#%02X%02X%02X", c.R, c.G, c.B)
	}
	return fmt.Sprintf("#%02X%02X%02X%02X", c.R, c.G, c.B, c.A)
}

func unquote(raw json.RawMessage) string {
	var s string
	_ = json.Unmarshal(raw, &s)
	return s
}

func TestPaletteCoversDesignColors(t *testing.T) {
	colors := 0
	for _, tok := range readTokens(t) {
		if tok.Type != "color" {
			continue
		}
		colors++
		field := strings.ToUpper(tok.Name[len("color."):][:1]) + tok.Name[len("color.")+1:]
		for name, palette := range map[string]Palette{"light": Light, "dark": Dark} {
			got := reflect.ValueOf(palette).FieldByName(field)
			if !got.IsValid() {
				t.Fatalf("Palette has no field %s for %s", field, tok.Name)
			}
			want := unquote(tok.Value)
			if name == "dark" && tok.Dark != nil {
				want = unquote(tok.Dark)
			}
			if gotHex := hexOf(got.Interface().(Color)); !strings.EqualFold(gotHex, want) {
				t.Errorf("%s %s: %s want %s", tok.Name, name, gotHex, want)
			}
		}
	}
	if fields := reflect.TypeOf(Palette{}).NumField(); fields != colors+1 {
		t.Fatalf("Palette has %d fields for %d design colors plus the popup shadow", fields, colors)
	}
	if colors != 20 {
		t.Fatalf("design source color count changed to %d: review every component before accepting", colors)
	}
}

func TestNarrowViewportValuesAndScale(t *testing.T) {
	if SizeSidebar(false) != 220 || SizeSidebar(true) != 72 || SizePanel(true) != 320 || SizeNavigation(true) != 184 || SizeViewportWidth(true) != 1024 {
		t.Fatal("narrow sizes drifted from the design tokens")
	}
	if ViewSidebarLabels(true) || !ViewSidebarLabels(false) {
		t.Fatal("sidebar labels flag drifted")
	}
	if ControlHeight != 32 || ControlIcon != 28 || FontSizeBody != 13 || RadiusCtrl != 8 || BorderFocus != 2 {
		t.Fatal("normal-density control scale drifted (button 36px and 12/14/16/24/32 type are retired)")
	}
	if For(true) != Dark || For(false) != Light {
		t.Fatal("For selected the wrong palette")
	}
	if Light.Popup.Color.A != 0x1A || Dark.Popup.Color.A != 0x66 || Light.Popup.Spread != -6 {
		t.Fatal("popup shadow drifted")
	}
}

func TestTokensJSONMatchesDesignSource(t *testing.T) {
	python, err := exec.LookPath("python3")
	if err != nil {
		t.Skip("python3 not found; CI's Linux runner has it and compares the exporter output with the committed JSON")
	}
	script, err := filepath.Abs("../../../../docs/design/alva/source/export_tokens.py")
	if err != nil {
		t.Fatal(err)
	}
	out := t.TempDir()
	cmd := exec.Command(python, "-I", script, out)
	if combined, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("exporter failed: %v\n%s", err, combined)
	}
	for exported, committed := range map[string]string{
		filepath.Join(out, "theme", "tokens.json"):               "tokens.json",
		filepath.Join(out, "testdata", "design-components.json"): "../testdata/design-components.json",
	} {
		got, err := os.ReadFile(exported)
		if err != nil {
			t.Fatal(err)
		}
		want, err := os.ReadFile(committed)
		if err != nil {
			t.Fatal(err)
		}
		if !bytes.Equal(got, want) {
			t.Errorf("%s differs from the design source: run python3 -I docs/design/alva/source/export_tokens.py, go generate ./apps/ui/kit/theme and commit both", committed)
		}
	}
}

func TestEmbeddedFontsAreTheDesignFonts(t *testing.T) {
	for _, name := range []string{"AlvaUISansSC-Regular.ttf", "AlvaUISansSC-SemiBold.ttf", "JetBrainsMono-Regular.ttf", "OFL.txt", "JetBrainsMono-OFL.txt", "UI-NOTICE.json", "JetBrainsMono-NOTICE.json"} {
		ours, err := FontFiles.ReadFile("fonts/" + name)
		if err != nil {
			t.Fatal(err)
		}
		design, err := os.ReadFile(filepath.Join("../../../../docs/design/alva/fonts", name))
		if err != nil || !bytes.Equal(ours, design) {
			t.Errorf("fonts/%s differs from docs/design/alva/fonts/%s", name, name)
		}
	}
}
