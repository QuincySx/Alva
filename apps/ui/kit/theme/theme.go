// INPUT: The generated tokens (tokens_gen.go) and nothing else; no MyGo, no build tag.
// OUTPUT: Color/Shadow value types shared by the generated Palette and the desktop mapping.
// POS: [L7 Kit] Pure design data so token tests run on every platform; apply_desktop.go maps it to ui.Theme.
package theme

import "embed"

//go:generate go run ./internal/gen

// Color is an 8-bit RGBA value.
type Color struct{ R, G, B, A uint8 }

// Shadow is a drop shadow in CSS order: offset, blur and spread in points.
type Shadow struct {
	X, Y, Blur, Spread float32
	Color              Color
}

// For returns the palette of an appearance.
func For(dark bool) Palette {
	if dark {
		return Dark
	}
	return Light
}

// FontFiles are the design fonts: Alva UI Sans SC at 400 and 600 (OFL) and JetBrains Mono (OFL), copied
// from docs/design/alva/fonts and compared with it by a test.
//
//go:embed fonts/AlvaUISansSC-Regular.ttf fonts/AlvaUISansSC-SemiBold.ttf fonts/JetBrainsMono-Regular.ttf fonts/OFL.txt fonts/JetBrainsMono-OFL.txt fonts/UI-NOTICE.json fonts/JetBrainsMono-NOTICE.json
var FontFiles embed.FS
