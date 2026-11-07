// INPUT: Foundation sidebar dimensions, page-local layout intent and native MyGo splitter input.
// OUTPUT: Shared pointer/keyboard splitter, bounded sidebar width and explicit collapsed navigation intent.
// POS: [L7 Hub UI] Changes presentation only; persistence and execution remain with their existing owners.
//go:build desktop

package hubdesktop

import (
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/egoist/mygo/ui"
)

const (
	workspaceConversationMinWidth float32 = 432
	workspaceWindowInset          float32 = 8
	workspaceRegionGap            float32 = 8
	workspacePaneRadius           float32 = 16 // Foundation ui.radius.card, shared by all three panes.
	workspaceShellWidth                   = 2*workspaceWindowInset + workspaceRegionGap
	workspacePaneMotionDuration           = 160 * time.Millisecond
)

func workspacePaneTransition() ui.ElementTransition {
	return ui.ElementTransition{Duration: workspacePaneMotionDuration, Position: true, Size: true}
}

func workspaceShell(c *ui.Context) ui.Element {
	return ui.Row(c).Fill().Padding(workspaceTopInset(c), workspaceWindowInset, workspaceWindowInset, workspaceWindowInset).
		Gap(workspaceRegionGap).AlignItems(ui.Stretch).Background(c.Theme().Background)
}

func workspaceMainSurface(c *ui.Context, key, label string) ui.Element {
	return ui.Column(c.Key(key)).Label(label).Grow(1).MinWidth(0).FillHeight().Gap(0).
		Background(c.Theme().Surface).Radius(workspacePaneRadius)
}

func workspaceAuxiliarySurface(c *ui.Context, key, label string) ui.Element {
	return ui.Column(c.Key(key)).Label(label).Grow(1).MinWidth(0).FillHeight().Gap(0).
		Background(workspacePalette(c).subtle).Radius(workspacePaneRadius).Border(1, c.Theme().Border)
}

type workspaceLayoutState struct {
	sidebarWidth                            float32 // Zero restores Foundation size.sidebar.
	sidebarCollapsed                        bool
	changed, widthChanged, collapsedChanged uint64
	sidebarResize                           workspaceSplitterState
}

type workspaceSplitterState struct {
	handle             ui.Handle
	dragging           bool
	startX, startWidth float32
}

type workspaceSplitterSpec struct {
	key, label, tooltip                string
	minimum, maximum, width, direction float32
	valid                              func() bool
	resize                             func(float32)
	reset                              func()
}

// Both pane edges use window coordinates: their own bounds move while dragging.
// Keeping the initial absolute pointer position avoids accumulating that motion.
func workspaceSplitter(c *ui.Context, state *workspaceSplitterState, spec workspaceSplitterSpec) ui.Element {
	edge := ui.Box(c.Key(spec.key)).Absolute().Top(0).Width(8).FillHeight().Cursor(ui.CursorResizeEW).
		Focusable().Role(ui.RoleSplitter).Label(spec.label).Tooltip(spec.tooltip).
		Range(float64(spec.minimum), float64(spec.maximum), float64(spec.width)).Disabled(!spec.valid()).Bind(&state.handle)
	edge.HandleInput(func(ev ui.InputEvent) bool {
		if !spec.valid() {
			state.dragging = false
			return false
		}
		switch ev.Kind {
		case ui.InputPointerDown:
			if ev.Button != 0 {
				return false
			}
			state.handle.Focus(c)
			state.dragging = true
			state.startX = state.handle.Bounds(c).X + ev.X
			state.startWidth = spec.width
			return true
		case ui.InputPointerMove:
			if ev.Button == 0 && state.dragging {
				spec.resize(state.startWidth + spec.direction*(state.handle.Bounds(c).X+ev.X-state.startX))
				return true
			}
		case ui.InputPointerUp:
			if ev.Button == 0 && state.dragging {
				state.dragging = false
				return true
			}
		case ui.InputKeyDown:
			if ev.Mods != 0 {
				return false
			}
			switch ev.Key {
			case ui.KeyLeft:
				spec.resize(spec.width - spec.direction*16)
			case ui.KeyRight:
				spec.resize(spec.width + spec.direction*16)
			case ui.KeyHome:
				spec.reset()
			case ui.KeyEnd:
				spec.resize(spec.maximum)
			default:
				return false
			}
			return true
		}
		return false
	})
	if edge.Hovered() || edge.Focused() || state.dragging {
		color := c.Theme().Accent
		edge.Draw(func(painter *ui.Painter, r ui.Rect) {
			painter.Fill(ui.Rect{X: r.X + r.W/2, Y: r.Y, W: 1, H: r.H}, color, 0)
		})
	}
	return edge
}

func (p *NativePage) workspaceSidebarDisplayedWidth(viewport float32) float32 {
	if p.layout.sidebarCollapsed {
		return 0
	}
	width := p.layout.sidebarWidth
	if width == 0 {
		width = hub.DefaultSidebarWidth
	}
	// A smaller viewport changes only the display, never the saved preference.
	return min(max(float32(hub.MinSidebarWidth), width), p.workspaceSidebarMaximumWidth(viewport))
}

func (p *NativePage) workspaceShellHorizontalInset() float32 {
	if p.layout.sidebarCollapsed {
		return 2 * workspaceWindowInset
	}
	return workspaceShellWidth
}

func (p *NativePage) workspaceSidebarMaximumWidth(viewport float32) float32 {
	return max(float32(hub.MinSidebarWidth), min(float32(hub.MaxSidebarWidth), viewport-workspaceShellWidth-48-workspaceConversationMinWidth))
}

func (p *NativePage) changeWorkspaceSidebar(width float32, collapsed bool) {
	if width != 0 {
		width = max(float32(hub.MinSidebarWidth), min(float32(hub.MaxSidebarWidth), width))
	}
	if p.layout.sidebarWidth == width && p.layout.sidebarCollapsed == collapsed {
		return
	}
	p.layout.changed++
	if p.layout.sidebarWidth != width {
		p.layout.widthChanged = p.layout.changed
	}
	if p.layout.sidebarCollapsed != collapsed {
		p.layout.sidebarResize.dragging = false
		p.layout.collapsedChanged = p.layout.changed
	}
	p.layout.sidebarWidth, p.layout.sidebarCollapsed = width, collapsed
}

func (p *NativePage) toggleWorkspaceSidebar() {
	p.changeWorkspaceSidebar(p.layout.sidebarWidth, !p.layout.sidebarCollapsed)
}

func (p *NativePage) buildWorkspaceSidebarResize(c *ui.Context, viewport float32) {
	width := p.workspaceSidebarDisplayedWidth(viewport)
	workspaceSplitter(c, &p.layout.sidebarResize, workspaceSplitterSpec{
		key: "sidebar-resize", label: "Resize session navigation",
		tooltip: "拖动调整导航宽度；方向键调整，Home 恢复默认宽度",
		minimum: hub.MinSidebarWidth, maximum: p.workspaceSidebarMaximumWidth(viewport), width: width, direction: 1,
		valid: func() bool { return !p.layout.sidebarCollapsed && !p.isWorkspaceSettings() },
		resize: func(value float32) {
			p.changeWorkspaceSidebar(min(value, p.workspaceSidebarMaximumWidth(viewport)), false)
		},
		reset: func() { p.changeWorkspaceSidebar(0, false) },
	}).Left(width - 8)
}
