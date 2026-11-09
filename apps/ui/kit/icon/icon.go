// INPUT: The embedded design Lucide SVGs (lucide/) and the generated name table.
// OUTPUT: icon.Name, Data (raw SVG bytes), Names and FlipX; the empty Name means no icon.
// POS: [L7 Kit] Pure data, no MyGo import: the kit package parses and paints; one Lucide 1.61.0 family only.
package icon

import (
	"embed"
	"io/fs"
)

//go:generate go run ./internal/gen

//go:embed lucide/*.svg lucide/LICENSE
var files embed.FS

// Name is a design icon name. The zero value means "no icon".
type Name string

// Data returns the SVG bytes of a design icon.
func Data(n Name) ([]byte, bool) {
	file, ok := lucideFile[n]
	if !ok {
		return nil, false
	}
	data, err := files.ReadFile("lucide/" + file + ".svg")
	return data, err == nil
}

// FlipX reports whether the icon is drawn mirrored horizontally (PanelLeft).
func FlipX(n Name) bool { return n == PanelLeft }

// Names lists every design icon name in design order.
func Names() []Name { return append([]Name(nil), designOrder...) }

// Files lists the embedded SVG file names, for tests.
func Files() []string {
	entries, _ := fs.ReadDir(files, "lucide")
	var out []string
	for _, e := range entries {
		out = append(out, e.Name())
	}
	return out
}
