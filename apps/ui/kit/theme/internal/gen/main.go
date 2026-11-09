// INPUT: tokens.json in the working directory (go generate runs inside apps/ui/kit/theme).
// OUTPUT: tokens_gen.go next to it: Light/Dark Palette, untyped scale constants and narrow-viewport functions.
// POS: [L7 Kit] Deterministic token generator; the Go build never needs Python, tests keep JSON and generated Go in step.
package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"go/format"
	"os"
	"strconv"
	"strings"
)

type token struct {
	Name   string          `json:"name"`
	Type   string          `json:"type"`
	Value  json.RawMessage `json:"value"`
	Dark   json.RawMessage `json:"dark,omitempty"`
	Narrow json.RawMessage `json:"narrow,omitempty"`
}

type document struct {
	Version int     `json:"version"`
	Tokens  []token `json:"tokens"`
}

func main() {
	data, err := os.ReadFile("tokens.json")
	if err != nil {
		fail(err)
	}
	out, err := generate(data)
	if err != nil {
		fail(err)
	}
	if err = os.WriteFile("tokens_gen.go", out, 0o644); err != nil {
		fail(err)
	}
}

func fail(err error) {
	fmt.Fprintln(os.Stderr, "gen:", err)
	os.Exit(1)
}

// ident converts a dotted token name to an exported Go identifier: font.size.body -> FontSizeBody.
func ident(name string) string {
	var b strings.Builder
	for _, part := range strings.Split(name, ".") {
		b.WriteString(strings.ToUpper(part[:1]) + part[1:])
	}
	return b.String()
}

func hexColor(raw json.RawMessage) (string, error) {
	var s string
	if err := json.Unmarshal(raw, &s); err != nil {
		return "", err
	}
	s = strings.TrimPrefix(s, "#")
	if len(s) == 6 {
		s += "FF"
	}
	if len(s) != 8 {
		return "", fmt.Errorf("color %q is not #RRGGBB or #RRGGBBAA", s)
	}
	v, err := strconv.ParseUint(s, 16, 32)
	if err != nil {
		return "", err
	}
	return fmt.Sprintf("Color{0x%02X, 0x%02X, 0x%02X, 0x%02X}", v>>24&255, v>>16&255, v>>8&255, v&255), nil
}

func shadowLiteral(raw json.RawMessage) (string, error) {
	var s struct {
		OffsetX, OffsetY, Blur, Spread float64
		Color                          string
	}
	// The design source spells the keys offsetX/offsetY; encoding/json matches case-insensitively.
	if err := json.Unmarshal(raw, &s); err != nil {
		return "", err
	}
	color, err := hexColor(json.RawMessage(strconv.Quote(s.Color)))
	if err != nil {
		return "", err
	}
	return fmt.Sprintf("Shadow{X: %v, Y: %v, Blur: %v, Spread: %v, Color: %s}", s.OffsetX, s.OffsetY, s.Blur, s.Spread, color), nil
}

func literal(raw json.RawMessage) string { return strings.TrimSpace(string(raw)) }

func generate(data []byte) ([]byte, error) {
	var doc document
	if err := json.Unmarshal(data, &doc); err != nil {
		return nil, err
	}
	if doc.Version != 1 {
		return nil, fmt.Errorf("unsupported token document version %d", doc.Version)
	}
	var colors, shadows []token
	var consts, funcs, strs, bools bytes.Buffer
	for _, t := range doc.Tokens {
		name := ident(t.Name)
		switch t.Type {
		case "color":
			colors = append(colors, t)
		case "shadow":
			shadows = append(shadows, t)
		case "font-family":
			fmt.Fprintf(&strs, "\t%s = %s\n", name, literal(t.Value))
		case "boolean":
			if t.Narrow != nil {
				fmt.Fprintf(&funcs, "// %s reports design token %s; narrow selects the Viewport/Narrow mode.\nfunc %s(narrow bool) bool {\n\tif narrow {\n\t\treturn %s\n\t}\n\treturn %s\n}\n\n", name, t.Name, name, literal(t.Narrow), literal(t.Value))
			} else {
				fmt.Fprintf(&bools, "\t%s = %s\n", name, literal(t.Value))
			}
		case "number", "font-size", "font-weight", "spacing", "border-radius", "stroke-width", "sizing":
			switch {
			case t.Name == "motion.normal" || t.Name == "motion.spinner":
				fmt.Fprintf(&consts, "\t%s = %s * time.Millisecond\n", name, literal(t.Value))
			case t.Narrow != nil:
				fmt.Fprintf(&funcs, "// %s is design token %s; narrow selects the Viewport/Narrow mode.\nfunc %s(narrow bool) float32 {\n\tif narrow {\n\t\treturn %s\n\t}\n\treturn %s\n}\n\n", name, t.Name, name, literal(t.Narrow), literal(t.Value))
			default:
				fmt.Fprintf(&consts, "\t%s = %s\n", name, literal(t.Value))
			}
		default:
			return nil, fmt.Errorf("token %s has unsupported type %q", t.Name, t.Type)
		}
	}
	var out bytes.Buffer
	out.WriteString("// INPUT: tokens.json exported from the design source (token_definitions()).\n")
	out.WriteString("// OUTPUT: Light and Dark palettes, scale constants and narrow-viewport functions. Generated: do not edit.\n")
	out.WriteString("// POS: [L7 Kit] Single Go source of design values; regenerate with go generate ./apps/ui/kit/theme.\n\n")
	out.WriteString("package theme\n\nimport \"time\"\n\n")
	out.WriteString("// Palette is every color role of one appearance, plus its popup shadow.\ntype Palette struct {\n")
	for _, t := range colors {
		fmt.Fprintf(&out, "\t%s Color\n", ident(strings.TrimPrefix(t.Name, "color.")))
	}
	for _, t := range shadows {
		fmt.Fprintf(&out, "\t%s Shadow\n", ident(strings.TrimPrefix(t.Name, "shadow.")))
	}
	out.WriteString("}\n\n")
	for _, mode := range []string{"Light", "Dark"} {
		fmt.Fprintf(&out, "// %s is the %s appearance.\nvar %s = Palette{\n", mode, strings.ToLower(mode), mode)
		pick := func(t token) json.RawMessage {
			if mode == "Dark" && t.Dark != nil {
				return t.Dark
			}
			return t.Value
		}
		for _, t := range colors {
			lit, err := hexColor(pick(t))
			if err != nil {
				return nil, fmt.Errorf("%s: %w", t.Name, err)
			}
			fmt.Fprintf(&out, "\t%s: %s,\n", ident(strings.TrimPrefix(t.Name, "color.")), lit)
		}
		for _, t := range shadows {
			lit, err := shadowLiteral(pick(t))
			if err != nil {
				return nil, fmt.Errorf("%s: %w", t.Name, err)
			}
			fmt.Fprintf(&out, "\t%s: %s,\n", ident(strings.TrimPrefix(t.Name, "shadow.")), lit)
		}
		out.WriteString("}\n\n")
	}
	if strs.Len() > 0 {
		out.WriteString("// Font families.\nconst (\n" + strs.String() + ")\n\n")
	}
	if consts.Len() > 0 {
		out.WriteString("// Scale values: font sizes and weights, line heights, spacing, radii, strokes, control sizes and motion.\nconst (\n" + consts.String() + ")\n\n")
	}
	if bools.Len() > 0 {
		out.WriteString("// Flags.\nconst (\n" + bools.String() + ")\n\n")
	}
	out.Write(funcs.Bytes())
	formatted, err := format.Source(out.Bytes())
	if err != nil {
		return nil, fmt.Errorf("format generated source: %w\n%s", err, out.String())
	}
	return formatted, nil
}
