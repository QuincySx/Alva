// INPUT: A ui.Tester and kit views.
// OUTPUT: Pixel readers (PixelAt, Near, Over, Ink), a themed Centered harness, Screenshot saving and the accessible-name audit.
// POS: [L7 Kit tests] Test helper shared by kit and gallery tests; pixel helpers follow the lobe-mygo testkit shape (MIT, API shape only, no code copied).
//go:build desktop

package kittest

import (
	"image"
	"image/png"
	"os"
	"path/filepath"
	"testing"

	"github.com/QuincySx/Alva/apps/ui/kit/internal/audit"
	"github.com/QuincySx/Alva/apps/ui/kit/theme"
	"github.com/egoist/mygo/ui"
)

// PixelAt reads the opaque color at a point in window coordinates; width is the tester's window width.
func PixelAt(tt *ui.Tester, width int, x, y float32) ui.Color {
	img := tt.Image()
	scale := float32(img.Bounds().Dx()) / float32(width)
	px := img.RGBAAt(int(x*scale), int(y*scale))
	return ui.RGB(px.R, px.G, px.B)
}

// Over composites src on dst, as the compositor does for translucent paint.
func Over(dst, src ui.Color) ui.Color {
	a := float32(src.A) / 255
	ch := func(d, s uint8) uint8 { return uint8(float32(s)*a + float32(d)*(1-a) + 0.5) }
	return ui.RGB(ch(dst.R, src.R), ch(dst.G, src.G), ch(dst.B, src.B))
}

func diff(a, b uint8) int { return max(int(a)-int(b), int(b)-int(a)) }

// Near reports whether two colors differ by at most 2 per channel (anti-aliasing slack).
func Near(a, b ui.Color) bool {
	return diff(a.R, b.R) <= 2 && diff(a.G, b.G) <= 2 && diff(a.B, b.B) <= 2
}

// Within reports whether two colors differ by at most tol per channel.
func Within(a, b ui.Color, tol int) bool {
	return diff(a.R, b.R) <= tol && diff(a.G, b.G) <= tol && diff(a.B, b.B) <= tol
}

// Ink returns the pixel in r that differs most from bg: the glyph or icon color at full coverage.
func Ink(tt *ui.Tester, width int, r ui.Rect, bg ui.Color) ui.Color {
	img := tt.Image()
	scale := float32(img.Bounds().Dx()) / float32(width)
	best, far := bg, -1
	for y := int(r.Y * scale); y < int((r.Y+r.H)*scale); y++ {
		for x := int(r.X * scale); x < int((r.X+r.W)*scale); x++ {
			px := img.RGBAAt(x, y)
			d := diff(px.R, bg.R) + diff(px.G, bg.G) + diff(px.B, bg.B)
			if d > far {
				best, far = ui.RGB(px.R, px.G, px.B), d
			}
		}
	}
	return best
}

// Centered themes the window with the design tokens and centers a view in a padded column on the canvas color.
func Centered(view func(c *ui.Context)) func(c *ui.Context) {
	return func(c *ui.Context) {
		theme.Apply(c, theme.Options{})
		ui.Column(c).Fill().Center().Padding(theme.Space6).Background(theme.UI(theme.Of(c).Surface)).Children(func() { view(c) })
	}
}

// Row is Centered with a horizontal gap, for side-by-side comparisons.
func Row(view func(c *ui.Context)) func(c *ui.Context) {
	return func(c *ui.Context) {
		theme.Apply(c, theme.Options{})
		ui.Row(c).Fill().Center().Gap(theme.Space4).Background(theme.UI(theme.Of(c).Surface)).Children(func() { view(c) })
	}
}

// Names starts collecting the accessible names interactive kit constructors report, returning a
// checker that fails the test for any nameless control.
func Names(t testing.TB) (assertNamed func(), seen func() []audit.Entry) {
	t.Helper()
	entries, restore := audit.Capture()
	t.Cleanup(restore)
	return func() {
		t.Helper()
		for _, e := range entries() {
			if e.Name == "" {
				t.Errorf("%s has no accessible name", e.Kind)
			}
		}
	}, entries
}

// Screenshot writes the tester's last frame under $ALVA_KIT_SHOTS/name.png and does nothing when the variable is unset.
func Screenshot(t testing.TB, tt *ui.Tester, name string) {
	t.Helper()
	dir := os.Getenv("ALVA_KIT_SHOTS")
	if dir == "" {
		return
	}
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	SaveImage(t, tt.Image(), filepath.Join(dir, name+".png"))
}

// SaveImage encodes img to path.
func SaveImage(t testing.TB, img image.Image, path string) {
	t.Helper()
	f, err := os.Create(path)
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	if err = png.Encode(f, img); err != nil {
		t.Fatal(err)
	}
}
