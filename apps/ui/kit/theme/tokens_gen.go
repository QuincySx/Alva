// INPUT: tokens.json exported from the design source (token_definitions()).
// OUTPUT: Light and Dark palettes, scale constants and narrow-viewport functions. Generated: do not edit.
// POS: [L7 Kit] Single Go source of design values; regenerate with go generate ./apps/ui/kit/theme.

package theme

import "time"

// Palette is every color role of one appearance, plus its popup shadow.
type Palette struct {
	Canvas        Color
	Surface       Color
	Subtle        Color
	Hover         Color
	Sidebar       Color
	Text          Color
	Secondary     Color
	Border        Color
	Accent        Color
	OnAccent      Color
	AccentSoft    Color
	ControlBorder Color
	Success       Color
	SuccessBg     Color
	Warning       Color
	WarningBg     Color
	Danger        Color
	DangerBg      Color
	Scrim         Color
	Transparent   Color
	Popup         Shadow
}

// Light is the light appearance.
var Light = Palette{
	Canvas:        Color{0xF3, 0xF4, 0xF7, 0xFF},
	Surface:       Color{0xFF, 0xFF, 0xFF, 0xFF},
	Subtle:        Color{0xF5, 0xF6, 0xF8, 0xFF},
	Hover:         Color{0xE9, 0xEB, 0xF0, 0xFF},
	Sidebar:       Color{0xF3, 0xF4, 0xF7, 0xFF},
	Text:          Color{0x27, 0x2A, 0x34, 0xFF},
	Secondary:     Color{0x62, 0x6B, 0x7A, 0xFF},
	Border:        Color{0xE2, 0xE5, 0xEB, 0xFF},
	Accent:        Color{0x65, 0x55, 0xCD, 0xFF},
	OnAccent:      Color{0xFF, 0xFF, 0xFF, 0xFF},
	AccentSoft:    Color{0xEF, 0xED, 0xF9, 0xFF},
	ControlBorder: Color{0x92, 0x9A, 0xA9, 0xFF},
	Success:       Color{0x32, 0x70, 0x4F, 0xFF},
	SuccessBg:     Color{0xED, 0xF4, 0xEE, 0xFF},
	Warning:       Color{0x85, 0x60, 0x29, 0xFF},
	WarningBg:     Color{0xF8, 0xF1, 0xE5, 0xFF},
	Danger:        Color{0xAC, 0x40, 0x50, 0xFF},
	DangerBg:      Color{0xF8, 0xEC, 0xEE, 0xFF},
	Scrim:         Color{0xE3, 0xE6, 0xED, 0xFF},
	Transparent:   Color{0xFF, 0xFF, 0xFF, 0x00},
	Popup:         Shadow{X: 0, Y: 8, Blur: 30, Spread: -6, Color: Color{0x20, 0x26, 0x35, 0x1A}},
}

// Dark is the dark appearance.
var Dark = Palette{
	Canvas:        Color{0x14, 0x16, 0x1B, 0xFF},
	Surface:       Color{0x1C, 0x1F, 0x26, 0xFF},
	Subtle:        Color{0x25, 0x29, 0x32, 0xFF},
	Hover:         Color{0x2C, 0x31, 0x3C, 0xFF},
	Sidebar:       Color{0x18, 0x1B, 0x21, 0xFF},
	Text:          Color{0xEA, 0xEC, 0xF3, 0xFF},
	Secondary:     Color{0xA6, 0xAD, 0xBA, 0xFF},
	Border:        Color{0x30, 0x36, 0x40, 0xFF},
	Accent:        Color{0xAA, 0xA1, 0xFF, 0xFF},
	OnAccent:      Color{0x23, 0x1F, 0x46, 0xFF},
	AccentSoft:    Color{0x2C, 0x29, 0x45, 0xFF},
	ControlBorder: Color{0x7F, 0x89, 0x99, 0xFF},
	Success:       Color{0xA0, 0xCD, 0xB3, 0xFF},
	SuccessBg:     Color{0x29, 0x39, 0x2F, 0xFF},
	Warning:       Color{0xE4, 0xC5, 0x96, 0xFF},
	WarningBg:     Color{0x3C, 0x34, 0x28, 0xFF},
	Danger:        Color{0xE6, 0xA6, 0xB2, 0xFF},
	DangerBg:      Color{0x42, 0x2E, 0x33, 0xFF},
	Scrim:         Color{0x10, 0x12, 0x16, 0xFF},
	Transparent:   Color{0xFF, 0xFF, 0xFF, 0x00},
	Popup:         Shadow{X: 0, Y: 8, Blur: 30, Spread: -6, Color: Color{0x00, 0x00, 0x00, 0x66}},
}

// Font families.
const (
	FontFamily = "Alva UI Sans SC"
	FontIcons  = "Alva Interface Icons"
	FontMono   = "JetBrains Mono"
)

// Scale values: font sizes and weights, line heights, spacing, radii, strokes, control sizes and motion.
const (
	FontSizeCaption    = 11
	FontSizeBody       = 13
	FontSizeSubtitle   = 14
	FontSizeHeading    = 20
	FontSizeDisplay    = 28
	FontSizeIcon       = 16
	FontSizeIconSmall  = 14
	FontWeightRegular  = 400
	FontWeightStrong   = 600
	LineNormal         = 1.5
	LineReading        = 1.6
	Space1             = 4
	Space2             = 8
	Space3             = 12
	Space4             = 16
	Space6             = 24
	Space8             = 32
	RadiusSmall        = 4
	RadiusCtrl         = 8
	RadiusCard         = 16
	RadiusPill         = 99
	BorderFocus        = 2
	ControlHeight      = 32
	ControlIcon        = 28
	ControlRow         = 28
	IconSize           = 16
	IconSmall          = 14
	SizeSidebar        = 220
	SizeNavigation     = 208
	SizeViewportHeight = 960
	SizeHeader         = 36
	SizePanelHeader    = 52
	MotionNormal       = 160 * time.Millisecond
	MotionSpinner      = 840 * time.Millisecond
)

// Flags.
const (
	MotionReduced = false
)

// SizePanel is design token size.panel; narrow selects the Viewport/Narrow mode.
func SizePanel(narrow bool) float32 {
	if narrow {
		return 320
	}
	return 432
}

// SizeViewportWidth is design token size.viewport.width; narrow selects the Viewport/Narrow mode.
func SizeViewportWidth(narrow bool) float32 {
	if narrow {
		return 1024
	}
	return 1440
}
