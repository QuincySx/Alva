// INPUT: The generated Light/Dark palettes and the MyGo Context/Theme.
// OUTPUT: theme.Of(c) for components, theme.Apply(c, Options) for views, theme.UI for color conversion, ReducedMotion.
// POS: [L7 Kit] The only place design colors become ui.Theme values; components read Palette and never write color literals.
//go:build desktop

package theme

import (
	"fmt"
	"sync"

	"github.com/egoist/mygo/ui"
)

// Appearance selects light, dark or the window's own setting.
type Appearance uint8

const (
	// System follows the window's appearance.
	System Appearance = iota
	// ForceLight and ForceDark pin the appearance regardless of the window.
	ForceLight
	ForceDark
)

// Options configure Apply. The zero value follows the system and its motion preference.
type Options struct {
	Appearance Appearance
	// ReduceMotion is the application's own preference; the system's is always honored too.
	ReduceMotion bool
}

// UI converts a design color to a MyGo color.
func UI(c Color) ui.Color { return ui.Color{R: c.R, G: c.G, B: c.B, A: c.A} }

// Of returns the palette of the window's current appearance.
func Of(c *ui.Context) Palette { return For(c.Theme().Dark) }

// Apply maps the design tokens onto the window's ui.Theme and, for reduced motion, disables
// animations for the frame so that kit loaders and MyGo transitions agree (see ReducedMotion).
func Apply(c *ui.Context, o Options) {
	dark := c.Theme().Dark
	switch o.Appearance {
	case ForceLight:
		dark = false
	case ForceDark:
		dark = true
	}
	t := ui.LightTheme()
	if dark {
		t = ui.DarkTheme()
	}
	p := For(dark)
	t.Background, t.Surface = UI(p.Canvas), UI(p.Surface)
	t.SurfaceHover, t.SurfacePressed = UI(p.Subtle), UI(p.Hover)
	t.Text, t.TextMuted, t.Border = UI(p.Text), UI(p.Secondary), UI(p.Border)
	t.Accent, t.AccentText = UI(p.Accent), UI(p.OnAccent)
	t.AccentHover, t.AccentPressed = t.Accent.Mix(t.Text, .08), t.Accent.Mix(t.Text, .16)
	t.Danger, t.Warning, t.Success = UI(p.Danger), UI(p.Warning), UI(p.Success)
	t.Focus = t.Accent
	t.Inverse, t.InverseText = t.Surface, t.Text
	t.Selection = t.Accent.Alpha(.2)
	t.Radius = RadiusCtrl
	t.Spacing = Space1
	t.Font = FontFamily
	t.FontSize = FontSizeBody
	c.SetTheme(t)
	if o.ReduceMotion || c.Preferences().ReduceMotion {
		c.DisableAnimations()
	}
}

// ReducedMotion reports whether decorative motion should stand still in this frame: the system
// preference, or an application preference applied through Apply (which disables animations).
func ReducedMotion(c *ui.Context) bool { return c.Preferences().ReduceMotion || c.Instant() }

var registerFonts = sync.OnceValue(func() error {
	for _, f := range []struct{ file, family string }{
		{"AlvaUISansSC-Regular.ttf", FontFamily}, {"AlvaUISansSC-SemiBold.ttf", FontFamily}, {"JetBrainsMono-Regular.ttf", FontMono},
	} {
		data, err := FontFiles.ReadFile("fonts/" + f.file)
		if err != nil {
			return err
		}
		if err = ui.RegisterFont(data, f.family); err != nil {
			return fmt.Errorf("register %s: %w", f.file, err)
		}
	}
	return nil
})

// RegisterFonts registers the embedded design fonts once; call it before the first frame (the gallery and its tests do).
func RegisterFonts() error { return registerFonts() }
