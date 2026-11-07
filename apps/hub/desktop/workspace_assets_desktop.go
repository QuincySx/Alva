// INPUT: Embedded licensed Alva UI font subset and pinned Lucide SVGs from the canonical design.
// OUTPUT: One shared workspace icon family, font registration and semantic surface palette.
// POS: [L7 Hub UI] Preserves source geometry and font metrics without per-label or per-icon offsets.
//go:build desktop

package hubdesktop

import (
	"embed"
	"fmt"
	"io/fs"
	"path"
	"strings"
	"sync"

	"github.com/egoist/mygo/ui"
)

//go:embed assets/lucide/*.svg assets/lucide/LICENSE assets/fonts/* assets/provenance.json
var workspaceAssets embed.FS

var registerWorkspaceAssets = sync.OnceValue(func() error {
	for _, file := range []string{"AlvaUISansSC-Regular.ttf", "AlvaUISansSC-SemiBold.ttf"} {
		data, err := workspaceAssets.ReadFile("assets/fonts/" + file)
		if err != nil {
			return err
		}
		if err = ui.RegisterFont(data, "Alva UI Sans SC"); err != nil {
			return fmt.Errorf("register workspace font: %w", err)
		}
	}
	return nil
})

var workspaceIcons = sync.OnceValue(func() map[string]*ui.SVG {
	files, err := fs.Glob(workspaceAssets, "assets/lucide/*.svg")
	if err != nil {
		panic(err)
	}
	icons := make(map[string]*ui.SVG, len(files))
	for _, file := range files {
		data, err := workspaceAssets.ReadFile(file)
		if err != nil {
			panic(err)
		}
		icons[strings.TrimSuffix(path.Base(file), ".svg")] = ui.MustParseSVG(data)
	}
	return icons
})

func workspaceIcon(name string) *ui.SVG {
	icon, ok := workspaceIcons()[name]
	if !ok {
		panic("unknown workspace icon: " + name)
	}
	return icon
}

type workspaceColors struct{ sidebar, subtle, hover, selection, controlBorder, dangerBg, scrim ui.Color }

// Alva Foundation ui.color.*, revision 843480b8 (2026-11-04).
// This is the shared UI palette; the older prototype color.* roles are separate.
func workspacePalette(c *ui.Context) workspaceColors {
	if c.Theme().Dark {
		return workspaceColors{ui.Hex("#14161B"), ui.Hex("#252932"), ui.Hex("#2C313C"), ui.Hex("#2C2945"), ui.Hex("#7F8999"), ui.Hex("#422E33"), ui.Hex("#101216")}
	}
	return workspaceColors{ui.Hex("#F3F4F7"), ui.Hex("#F5F6F8"), ui.Hex("#E9EBF0"), ui.Hex("#EFEDF9"), ui.Hex("#929AA9"), ui.Hex("#F8ECEE"), ui.Hex("#E3E6ED")}
}
