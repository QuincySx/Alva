// INPUT: Shared workspace resource/view services, Foundation controls and page-local observation cancellation.
// OUTPUT: Source-keyed resource rows/path labels, bounded Markdown/raw/integrity views and durable tabs/resize/popout/return.
// POS: [L7 Hub UI] Observes files/tasks/evidence without stopping, restoring or replaying their execution owners.
//go:build desktop

package hubdesktop

import (
	"context"
	"encoding/hex"
	"fmt"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
)

type workspaceResourceSession struct {
	view                    hub.WorkspaceView
	loaded, loading, saving bool
	changed, saved          uint64
	problem                 string
	scroll                  map[string]*ui.ScrollState
	cancel                  context.CancelFunc
}

type workspaceResourceDrag struct{ session, tab string }

type workspaceResourcesState struct {
	session        string
	sessions       map[string]*workspaceResourceSession
	page           product.WorkspaceResourcePage
	loading        bool
	openRequested  bool
	emptyOpen      bool
	suppressed     bool // Explicit advanced navigation hides, but does not close, saved tabs.
	problem, query string
	reveal         string
	popout         string
	epoch          uint64
	cancel         context.CancelFunc
	resize         workspaceSplitterState
	raw, metadata  bool
	find           bool
	findQuery      string
	markdown       markdown.Options
	// pending is a tab a tool jump requested before the view loaded.
	pending *workspacePendingResource
}

func (p *NativePage) workspaceResourceSession() *workspaceResourceSession {
	if p.resources.session != p.selected {
		return nil
	}
	return p.resources.sessions[p.selected]
}

func (p *NativePage) workspaceActiveResource() *hub.WorkspaceTab {
	s := p.workspaceResourceSession()
	if s == nil {
		return nil
	}
	for i := range s.view.Tabs {
		if s.view.Tabs[i].ID == s.view.Active {
			return &s.view.Tabs[i]
		}
	}
	return nil
}

func (p *NativePage) selectWorkspaceResources(id string) {
	p.observeWorkspaceResources()
	p.closeWorkspaceResources()
	if p.resources.session != id {
		if s := p.resources.sessions[p.resources.session]; s != nil && s.cancel != nil {
			s.cancel()
			delete(p.resources.sessions, p.resources.session)
		}
		p.resources.popout = ""
	}
	if p.resources.pending != nil && p.resources.pending.session != id {
		p.resources.pending = nil
	}
	p.resources.session = id
	p.resources.openRequested = false
	p.resources.emptyOpen, p.resources.suppressed = false, false
	if id == "" || p.Service.WorkspaceViews == nil {
		return
	}
	if p.resources.sessions == nil {
		p.resources.sessions = make(map[string]*workspaceResourceSession)
	}
	if s := p.resources.sessions[id]; s != nil && s.loaded {
		if !s.view.Collapsed {
			p.loadWorkspaceResource()
		}
		return
	}
	p.reloadWorkspaceResources()
}

func (p *NativePage) reloadWorkspaceResources() {
	id := p.selected
	if id == "" || p.Service.WorkspaceViews == nil {
		return
	}
	if p.resources.sessions == nil {
		p.resources.sessions = make(map[string]*workspaceResourceSession)
	}
	old := p.resources.sessions[id]
	if old != nil && (old.saving || old.loading) {
		return
	}
	p.closeWorkspaceResources()
	p.resources.session = id
	s := &workspaceResourceSession{view: hub.WorkspaceView{SessionID: id}, loading: true, scroll: make(map[string]*ui.ScrollState)}
	ctx, cancel := context.WithCancel(p.ctx)
	s.cancel = cancel
	p.resources.sessions[id] = s
	go func() {
		value, err := p.Service.WorkspaceViews.Read(ctx, id)
		p.dispatch(func() {
			cancel()
			if p.resources.sessions[id] != s {
				return
			}
			s.cancel = nil
			s.loading = false
			if err != nil {
				s.problem = err.Error()
				// A tab requested before the failed read is not opened later.
				if p.resources.pending != nil && p.resources.pending.session == id {
					p.resources.pending = nil
				}
				return
			}
			s.view, s.loaded = value, true
			if p.selected == id && p.resources.session == id {
				p.resources.reveal = value.Active
			}
			for _, tab := range value.Tabs {
				s.scroll[tab.ID] = &ui.ScrollState{X: tab.ScrollX, Y: tab.ScrollY}
			}
			if p.selected == id && p.resources.session == id && p.resources.openRequested {
				p.resources.openRequested = false
				p.openWorkspaceResources()
				p.openPendingWorkspaceResource(id)
				return
			}
			if p.selected == id && p.resources.session == id && !value.Collapsed {
				p.loadWorkspaceResource()
			}
		})
	}()
}

func (p *NativePage) saveWorkspaceResources(s *workspaceResourceSession) {
	if s == nil || !s.loaded || s.saving || s.problem != "" || s.changed == s.saved || p.Service.WorkspaceViews == nil {
		return
	}
	s.saving = true
	version := s.changed
	view := hub.CloneWorkspaceView(s.view)
	go func() {
		// The service owns this accepted save through completion, even if its
		// initiating observer leaves. Application Close cancels and joins it.
		value, err := p.Service.WorkspaceViews.Save(context.Background(), view, view.Revision)
		p.dispatch(func() {
			s.saving = false
			if err != nil {
				s.problem = err.Error()
				return
			}
			s.view.Revision = value.Revision
			s.saved = version
			p.saveWorkspaceResources(s)
		})
	}()
}

func (p *NativePage) changedWorkspaceResources() {
	s := p.workspaceResourceSession()
	if s == nil {
		return
	}
	s.changed++
	p.saveWorkspaceResources(s)
}

func (p *NativePage) observeWorkspaceResources() {
	if p.resources.loading || p.resources.problem != "" {
		return
	}
	s := p.workspaceResourceSession()
	tab := p.workspaceActiveResource()
	if s == nil || tab == nil {
		return
	}
	if scroll := s.scroll[tab.ID]; scroll != nil && (tab.ScrollX != scroll.X || tab.ScrollY != scroll.Y) {
		tab.ScrollX, tab.ScrollY = scroll.X, scroll.Y
		p.changedWorkspaceResources()
	}
}

func (p *NativePage) prepareWorkspaceResourcesClose() bool {
	p.observeWorkspaceResources()
	for _, s := range p.resources.sessions {
		if s.saving {
			return true
		}
	}
	return false
}

func (p *NativePage) closeWorkspaceResources() {
	if p.resources.cancel != nil {
		p.resources.cancel()
		p.resources.cancel = nil
	}
	p.resources.epoch++
	p.resources.page = product.WorkspaceResourcePage{}
	p.resources.problem, p.resources.query = "", ""
	p.resources.loading = false
	p.resources.raw, p.resources.metadata, p.resources.find = false, false, false
	p.resources.findQuery = ""
	p.resources.markdown = markdown.Options{}
	p.resources.resize.dragging = false
}

func (p *NativePage) openWorkspaceResources() {
	if p.selected == "" || p.Service.WorkspaceViews == nil {
		return
	}
	p.resources.suppressed = false
	if p.resources.session != p.selected {
		p.selectWorkspaceResources(p.selected)
	}
	s := p.workspaceResourceSession()
	if s == nil {
		p.reloadWorkspaceResources()
		s = p.workspaceResourceSession()
	}
	if s == nil || !s.loaded {
		p.resources.openRequested = true
		return
	}
	if s.view.Collapsed || s.view.Revision == 0 && len(s.view.Tabs) == 0 && s.changed == 0 {
		s.view.Collapsed = false
		p.changedWorkspaceResources()
	}
	p.loadWorkspaceResource()
}

func (p *NativePage) toggleWorkspaceResources() {
	if p.selected == "" {
		return
	}
	if p.workspaceResourcesVisible() {
		p.collapseWorkspaceResources()
		return
	}
	p.openWorkspaceResources()
}

func (p *NativePage) collapseWorkspaceResources() {
	p.resources.emptyOpen = false
	p.observeWorkspaceResources()
	s := p.workspaceResourceSession()
	p.resources.openRequested = false
	p.closeWorkspaceResources()
	if s == nil {
		return
	}
	if !s.loaded {
		if s.cancel != nil {
			s.cancel()
		}
		delete(p.resources.sessions, p.selected)
		return
	}
	s.view.Collapsed = true
	p.changedWorkspaceResources()
}

func workspaceResourceSame(a, b hub.WorkspaceTab) bool {
	if a.Kind != b.Kind || a.BranchID != b.BranchID || a.URI != b.URI || a.TeamID != b.TeamID || a.WorkerID != b.WorkerID || a.TaskID != b.TaskID {
		return false
	}
	if a.File == nil || b.File == nil {
		return a.File == nil && b.File == nil
	}
	return *a.File == *b.File
}

func (p *NativePage) openWorkspaceResource(tab hub.WorkspaceTab) {
	s := p.workspaceResourceSession()
	if s == nil || !s.loaded {
		return
	}
	p.resources.suppressed = false
	p.observeWorkspaceResources()
	for _, open := range s.view.Tabs {
		if workspaceResourceSame(open, tab) {
			if s.view.Collapsed {
				s.view.Collapsed = false
				p.changedWorkspaceResources()
			}
			p.activateWorkspaceResource(open.ID)
			return
		}
	}
	if len(s.view.Tabs) >= 12 {
		p.resources.problem = "最多打开 12 个资料标签，请先关闭一个。"
		return
	}
	id, err := hub.NewID("view-")
	if err != nil {
		p.resources.problem = err.Error()
		return
	}
	tab.ID = id
	s.view.Tabs = append(s.view.Tabs, tab)
	s.scroll[id] = &ui.ScrollState{X: tab.ScrollX, Y: tab.ScrollY}
	s.view.Collapsed = false
	p.activateWorkspaceResource(id)
}

func (p *NativePage) activateWorkspaceResource(id string) {
	s := p.workspaceResourceSession()
	if s == nil {
		return
	}
	found := false
	for _, tab := range s.view.Tabs {
		found = found || tab.ID == id
	}
	if !found {
		return
	}
	p.observeWorkspaceResources()
	changed := s.view.Active != id
	s.view.Active = id
	p.resources.reveal = id
	if changed {
		p.changedWorkspaceResources()
	}
	p.loadWorkspaceResource()
}

func (p *NativePage) closeWorkspaceResource(id string) {
	s := p.workspaceResourceSession()
	if s == nil {
		return
	}
	p.observeWorkspaceResources()
	for i, tab := range s.view.Tabs {
		if tab.ID != id {
			continue
		}
		wasActive := s.view.Active == id
		s.view.Tabs = append(s.view.Tabs[:i], s.view.Tabs[i+1:]...)
		delete(s.scroll, id)
		if wasActive {
			p.closeWorkspaceResources()
			s.view.Active = ""
			if len(s.view.Tabs) > 0 {
				s.view.Active = s.view.Tabs[min(i, len(s.view.Tabs)-1)].ID
			}
		}
		if len(s.view.Tabs) == 0 {
			s.view.Focused = false
			s.view.Collapsed = true
		}
		p.changedWorkspaceResources()
		if wasActive && len(s.view.Tabs) > 0 {
			p.loadWorkspaceResource()
		}
		return
	}
}

func (p *NativePage) moveWorkspaceResource(delta int) {
	s := p.workspaceResourceSession()
	if s == nil {
		return
	}
	p.moveWorkspaceResourceTab(s.view.Active, delta)
}

func (p *NativePage) moveWorkspaceResourceTab(id string, delta int) {
	s := p.workspaceResourceSession()
	if s == nil {
		return
	}
	for i, tab := range s.view.Tabs {
		if tab.ID == id && i+delta >= 0 && i+delta < len(s.view.Tabs) {
			p.reorderWorkspaceResource(workspaceResourceDrag{p.selected, tab.ID}, s.view.Tabs[i+delta].ID)
			return
		}
	}
}

func (p *NativePage) reorderWorkspaceResource(source workspaceResourceDrag, target string) {
	s := p.workspaceResourceSession()
	if s == nil || source.session != p.selected || source.tab == target {
		return
	}
	from, to := -1, -1
	for i, tab := range s.view.Tabs {
		if tab.ID == source.tab {
			from = i
		}
		if tab.ID == target {
			to = i
		}
	}
	if from < 0 || to < 0 {
		return
	}
	tab := s.view.Tabs[from]
	if from < to {
		copy(s.view.Tabs[from:to], s.view.Tabs[from+1:to+1])
	} else {
		copy(s.view.Tabs[to+1:from+1], s.view.Tabs[to:from])
	}
	s.view.Tabs[to] = tab
	p.resources.reveal = tab.ID
	p.changedWorkspaceResources()
}

func (p *NativePage) loadWorkspaceResource() {
	tab := p.workspaceActiveResource()
	if tab == nil || p.Service.WorkspaceViews == nil {
		return
	}
	p.closeWorkspaceResources()
	id, location := p.selected, *tab
	ctx, cancel := context.WithCancel(p.ctx)
	p.resources.cancel = cancel
	p.resources.loading = true
	p.resources.query = location.Query
	epoch := p.resources.epoch
	go func() {
		page, err := p.Service.WorkspaceResource(ctx, id, location)
		p.dispatch(func() {
			if p.resources.epoch != epoch || p.selected != id || p.resources.session != id {
				return
			}
			current := p.workspaceActiveResource()
			if current == nil || current.ID != location.ID {
				return
			}
			p.resources.loading = false
			p.resources.cancel = nil
			cancel()
			if err != nil {
				p.resources.problem = err.Error()
				return
			}
			p.resources.page = page
			if current.HighWater == 0 && page.HighWater != 0 {
				current.HighWater = page.HighWater
				p.changedWorkspaceResources()
			}
		})
	}()
}

func (p *NativePage) workspaceResourcesVisible() bool {
	if p.resources.suppressed {
		return false
	}
	if p.selected == "" || p.Service.WorkspaceViews == nil {
		return p.resources.emptyOpen
	}
	s := p.workspaceResourceSession()
	return s != nil && (s.loaded && !s.view.Collapsed && (len(s.view.Tabs) > 0 || s.view.Revision > 0 || s.changed > 0) || p.resources.openRequested && !s.loaded)
}

// At narrow widths the content region becomes a focused resource view. The
// durable user's focus choice is unchanged; widening restores the split.
func (p *NativePage) workspaceResourcesFocused() bool {
	s := p.workspaceResourceSession()
	if s == nil || !p.workspaceResourcesVisible() {
		return false
	}
	return s.view.Focused || p.resources.popout != ""
}

func (p *NativePage) workspaceResourcesFullWidth(width float32) bool {
	minimum := p.workspaceShellHorizontalInset() + p.workspaceSidebarDisplayedWidth(width) + 36 + 24 + workspaceConversationMinWidth + 320
	return p.workspaceResourcesVisible() && (p.workspaceResourcesFocused() || width < minimum)
}

func (p *NativePage) workspaceResourceWidth(preferred, viewport float32) float32 {
	if preferred == 0 {
		preferred = 432 // Foundation ui.size.panel, used by page 22.
	}
	// Preserve usable reading/composer width beside the actual navigation.
	// The viewport limit never replaces the saved preferred width.
	return min(preferred, max(float32(320), min(float32(960), viewport-p.workspaceShellHorizontalInset()-p.workspaceSidebarDisplayedWidth(viewport)-36-24-workspaceConversationMinWidth)))
}

func (p *NativePage) resizeWorkspaceResources(s *workspaceResourceSession, value, viewport float32) {
	if s != p.workspaceResourceSession() || !s.loaded || p.workspaceResourcesFullWidth(viewport) {
		return
	}
	value = p.workspaceResourceWidth(max(float32(320), min(float32(960), value)), viewport)
	if value != s.view.Width {
		s.view.Width = value
		p.changedWorkspaceResources()
	}
}

func (p *NativePage) buildWorkspaceResourceResize(c *ui.Context, s *workspaceResourceSession, viewport float32) {
	workspaceSplitter(c, &p.resources.resize, workspaceSplitterSpec{
		key: "resource-resize", label: "Resize resource workspace",
		tooltip: "拖动调整资料宽度；方向键调整，Home 恢复默认宽度",
		minimum: 320, maximum: p.workspaceResourceWidth(960, viewport), width: p.workspaceResourceWidth(s.view.Width, viewport), direction: -1,
		valid: func() bool {
			return p.workspaceResourceSession() == s && s.loaded && !p.workspaceResourcesFullWidth(viewport)
		},
		resize: func(value float32) { p.resizeWorkspaceResources(s, value, viewport) },
		reset: func() {
			if s.view.Width != 0 {
				s.view.Width = 0
				p.changedWorkspaceResources()
			}
		},
	}).Left(-8)
}

func (p *NativePage) buildWorkspaceResourceMenu(m *ui.Menu) {
	for _, item := range []struct{ kind, title string }{{"files", "项目文件"}, {"artifacts", "原件与产物"}, {"tasks", "协作任务"}, {"inspector", "Inspector"}, {"media", "媒体原件"}} {
		if m.Item(item.title).Disabled(p.detail.Session.ID != p.selected || p.detail.Session.ActiveBranchID == "").Chosen() {
			p.openWorkspaceResource(hub.WorkspaceTab{Kind: item.kind, Title: item.title, BranchID: p.detail.Session.ActiveBranchID})
		}
	}
	m.Separator()
	if s := p.workspaceResourceSession(); s != nil {
		label := "专注资料"
		if s.view.Focused {
			label = "恢复分栏"
		}
		if m.Item(label).Disabled(!s.loaded).Chosen() {
			s.view.Focused = !s.view.Focused
			p.changedWorkspaceResources()
		}
	}
	if m.Item("标签左移").Chosen() {
		p.moveWorkspaceResource(-1)
	}
	if m.Item("标签右移").Chosen() {
		p.moveWorkspaceResource(1)
	}
	if m.Item("从已保存位置重新载入").Chosen() {
		p.reloadWorkspaceResources()
	}
}

func (p *NativePage) buildWorkspaceResources(c *ui.Context, showCollapse bool) {
	if !p.workspaceResourcesVisible() {
		return
	}
	s := p.workspaceResourceSession()
	width, _ := c.Size()
	panel := ui.Column(c.Key("resource-workspace")).Label("Resource workspace").FillHeight().MinWidth(0).Gap(workspaceRegionGap)
	if p.workspaceResourcesFullWidth(width) {
		panel.Grow(1).FillWidth().Transition(workspacePaneTransition())
	} else {
		preferred := float32(0)
		if s != nil {
			preferred = s.view.Width
		}
		panelWidth := p.workspaceResourceWidth(preferred, width)
		panel.Width(panelWidth).Shrink(0).Transition(ui.ElementTransition{
			Duration: workspacePaneMotionDuration, Position: true, Size: true,
			Enter: &ui.Motion{X: panelWidth, Collapse: true, Opacity: 1},
			Exit:  &ui.Motion{X: panelWidth, Collapse: true, Opacity: 0},
		})
	}
	panel.Children(func() {
		if s != nil && !p.workspaceResourcesFullWidth(width) {
			p.buildWorkspaceResourceResize(c, s, width)
		}
		workspaceColumnToolbar(c, "resource-header", "Resource header", workspaceWindowInset, workspaceRightSafeInset(c), func() {
			if s != nil {
				p.buildWorkspaceResourceTabs(c, s)
			} else {
				ui.Spacer(c)
			}
			workspaceIconAction(c, "resource-add", "Open resource tab", "plus").Disabled(s == nil || !s.loaded).Menu(p.buildWorkspaceResourceMenu)
			workspaceIconAction(c, "workspace-more", "More actions", "ellipsis").Menu(p.buildWorkspaceResourceMore)
			if showCollapse {
				ui.Box(c).Size(workspaceIconControlSize, workspaceIconControlSize).Shrink(0) // Fixed shell toggle owns this slot.
			}
		})
		workspaceAuxiliarySurface(c, "resource-surface", "Resource surface").FillWidth().Children(func() {
			if s == nil {
				ui.Column(c).Label("Resource empty state").Grow(1).FillWidth().Padding(16).Gap(12).Children(func() {
					if p.selected != "" && p.Service.WorkspaceViews == nil {
						ui.Text(c, "当前工作台未提供资料服务。").FontSize(workspaceFontBody).TextColor(c.Theme().TextMuted)
						return
					}
					ui.Text(c, "选择任务后查看资料").FontSize(workspaceFontBody).TextColor(c.Theme().TextMuted)
					workspaceControlButton(c, "resource-choose-task", "选择任务", "secondary", false).OnClick(func() {
						p.collapseWorkspaceResources()
						p.openWorkspaceSessionChooser("resources")
					})
				})
				return
			}
			if !s.loaded {
				ui.Column(c).Grow(1).MinHeight(0).Padding(12).Gap(8).Children(func() {
					if s.loading {
						ui.Text(c, "正在读取资料位置…").Role(ui.RoleStatus).FontSize(workspaceFontCaption)
					} else {
						ui.Text(c, s.problem).Role(ui.RoleStatus).FontSize(workspaceFontCaption).TextColor(c.Theme().Danger)
						workspaceAction(c, "resource-retry-load", "重新读取资料位置", "refresh-cw", false).OnClick(p.reloadWorkspaceResources)
					}
				})
				return
			}
			p.buildWorkspaceResourceContents(c, s)
		})
	})
}

func (p *NativePage) buildWorkspaceResourceMore(m *ui.Menu) {
	if p.resources.popout != "" && p.returnResourceWindow != nil {
		if m.Item("收回资料窗口").Chosen() {
			p.returnResourceWindow()
		}
	} else {
		s := p.workspaceResourceSession()
		unavailable := s == nil || !s.loaded || p.openResourceWindow == nil && p.openWindow == nil
		if s != nil {
			unavailable = unavailable || s.saving || s.changed != s.saved || s.problem != "" || p.workspaceActiveResource() == nil
		}
		if m.Item("在新窗口打开资料").Disabled(unavailable).Chosen() {
			var err error
			if p.openResourceWindow != nil {
				err = p.openResourceWindow(p.selected, *p.workspaceActiveResource())
			} else {
				err = p.openWindow(p.selected)
			}
			if err != nil {
				p.resources.problem = err.Error()
			}
		}
	}
	m.Separator()
	p.buildWorkspaceMenu(m)
}

func (p *NativePage) buildWorkspaceResourceTabs(c *ui.Context, s *workspaceResourceSession) {
	ui.ScrollHorizontal(c.Key("resource-tabs")).Label("Resource tabs").Height(workspaceNavigationRowHeight).Grow(1).MinWidth(0).Children(func() {
		ui.Row(c).Height(workspaceNavigationRowHeight).Gap(4).Children(func() {
			// A drop can reorder the slice during this frame. Capture its order
			// so every stable identity is still built exactly once.
			for _, tab := range append([]hub.WorkspaceTab(nil), s.view.Tabs...) {
				tab := tab
				label := tab.Title
				if tab.File != nil {
					label = tab.File.Path
				}
				fill := workspacePalette(c).subtle
				if s.view.Active == tab.ID {
					fill = c.Theme().Surface
				}
				row := ui.Row(c.Key(tab.ID)).MaxWidth(180).Shrink(0).Height(workspaceNavigationRowHeight).Padding(0, 8).Gap(8).Radius(8).
					Background(fill).AlignItems(ui.Center)
				if source, ok := ui.Drop[workspaceResourceDrag](row); ok {
					p.reorderWorkspaceResource(source, tab.ID)
				}
				stroke := fill
				if source, ok := ui.DragOver[workspaceResourceDrag](row); ok && source.session == p.selected && source.tab != tab.ID {
					stroke = c.Theme().Accent
				}
				row.DrawOver(func(painter *ui.Painter, r ui.Rect) { painter.Stroke(r, stroke, 8, 1) })
				row.Children(func() {
					button := ui.ButtonBase(c.Key("activate")).Label("Select resource "+label).Tooltip(label).MinWidth(0).Grow(1).Shrink(1).FillHeight().Padding(0).Gap(8).
						AlignItems(ui.Center).Radius(8).Selected(s.view.Active == tab.ID).Background(fill).TextColor(c.Theme().Text).Drag(workspaceResourceDrag{p.selected, tab.ID}).OnClick(func() { p.activateWorkspaceResource(tab.ID) }).
						OnShortcut(ui.Alt|ui.Shift, ui.KeyLeft, func() { p.moveWorkspaceResourceTab(tab.ID, -1) }).
						OnShortcut(ui.Alt|ui.Shift, ui.KeyRight, func() { p.moveWorkspaceResourceTab(tab.ID, 1) })
					button.Children(func() {
						icon := "file-text"
						if tab.Kind == "tasks" || tab.Kind == "task" {
							icon = "list"
						}
						ui.Icon(c, workspaceIcon(icon)).Size(16, 16).Shrink(0)
						ui.Text(c, tab.Title).TextColor(c.Theme().Text).FontSize(workspaceFontBody).FontWeight(400).LineHeight(1.5).MinWidth(0).SingleLine()
					})
					if p.resources.reveal == tab.ID {
						row.ScrollIntoView()
						p.resources.reveal = ""
					}
					close := ui.ButtonBase(c.Key("close")).Label("Close resource "+label).Tooltip("关闭 "+label).Size(16, workspaceNavigationRowHeight).Padding(0).Radius(8).AlignItems(ui.Center).Justify(ui.Center).
						TextColor(c.Theme().TextMuted).OnClick(func() { p.closeWorkspaceResource(tab.ID) })
					if close.Hovered() || close.FocusVisible() {
						close.TextColor(c.Theme().Text)
					}
					close.Children(func() { ui.Icon(c, workspaceIcon("x")).Size(16, 16) })
				})
			}
		})
	})
}

func (p *NativePage) buildWorkspaceResourceContents(c *ui.Context, s *workspaceResourceSession) {
	tab := p.workspaceActiveResource()
	if tab == nil {
		ui.Text(c, "选择资料标签，或通过添加按钮打开资料。").Role(ui.RoleStatus).FontSize(workspaceFontBody).Padding(16)
		return
	}
	ui.Column(c).Grow(1).MinHeight(0).FillWidth().Gap(0).Children(func() {
		if s.problem != "" {
			ui.Text(c, "资料位置尚未保存："+s.problem).FontSize(workspaceFontCaption).TextColor(c.Theme().Danger)
			ui.Row(c).Gap(8).Children(func() {
				workspaceAction(c, "resource-reload-view", "重新载入已保存位置", "refresh-cw", false).OnClick(p.reloadWorkspaceResources)
			})
		}
		pathHeight := float32(32)
		if tab.Kind == "files" || tab.Kind == "inspector" {
			pathHeight = 36 // The shared search control keeps its own size.
		}
		ui.Row(c).Label("Resource path and find").FillWidth().Height(pathHeight).Shrink(0).Padding(0, 24).Gap(8).AlignItems(ui.Center).Children(func() {
			if tab.Kind == "files" || tab.Kind == "inspector" {
				input := workspaceControlInput(c, "resource-query", &p.resources.query).Label("Search resource names or evidence").Placeholder("搜索").Grow(1).MinWidth(0)
				if input.Submitted() {
					tab.Query = p.resources.query
					tab.Cursor, tab.HighWater = 0, 0
					tab.After = ""
					p.changedWorkspaceResources()
					p.loadWorkspaceResource()
				}
			} else {
				path := tab.Title
				if tab.File != nil {
					path = tab.File.Path
				}
				ui.Text(c, path).Label("Resource path").Tooltip(path).FontSize(workspaceFontCaption).TextColor(c.Theme().TextMuted).SingleLine().MinWidth(0).Grow(1)
			}
			if p.resources.page.Body.Title != "" {
				workspaceIconAction(c, "resource-find", "Find in loaded resource content", "search").Tooltip("查找当前载入内容").OnClick(func() {
					p.resources.find = !p.resources.find
				})
			}
			workspaceIconAction(c, "resource-reading-options", "Resource reading options", "ellipsis").Menu(func(m *ui.Menu) {
				if m.Item("刷新当前资料").Chosen() {
					tab.Offset = 0
					tab.Cursor, tab.HighWater = 0, 0
					tab.After = ""
					tab.ScrollX, tab.ScrollY = 0, 0
					s.scroll[tab.ID] = &ui.ScrollState{}
					p.changedWorkspaceResources()
					p.loadWorkspaceResource()
				}
				if p.resources.page.Body.Title != "" {
					m.Separator()
					label := "查看原始内容"
					if p.resources.raw {
						label = "返回阅读视图"
					}
					if m.Item(label).Chosen() {
						p.resources.raw = !p.resources.raw
					}
					label = "查看校验信息"
					if p.resources.metadata {
						label = "收起校验信息"
					}
					if m.Item(label).Chosen() {
						p.resources.metadata = !p.resources.metadata
					}
				}
			})
		})
		if p.resources.find {
			ui.Row(c).FillWidth().Padding(8, 24).Gap(8).AlignItems(ui.Center).Children(func() {
				workspaceControlInput(c, "resource-find-query", &p.resources.findQuery).Label("Find in currently loaded content").Placeholder("查找当前载入内容").Grow(1).MinWidth(0)
				workspaceIconAction(c, "resource-find-close", "Close resource find", "x").OnClick(func() { p.resources.find = false })
			})
		}
		if p.resources.loading {
			ui.Text(c, "正在读取资料…").Role(ui.RoleStatus).FontSize(workspaceFontBody).Padding(16)
			ui.Spacer(c).Grow(1)
			return
		}
		if p.resources.problem != "" {
			ui.Column(c).Padding(16).Gap(12).Children(func() {
				ui.Text(c, p.resources.problem).Role(ui.RoleStatus).TextColor(c.Theme().Danger).FontSize(workspaceFontBody)
				workspaceAction(c, "resource-retry-read", "重新读取资料", "refresh-cw", false).OnClick(p.loadWorkspaceResource)
			})
			ui.Spacer(c).Grow(1)
			return
		}
		scroll := s.scroll[tab.ID]
		if scroll == nil {
			scroll = &ui.ScrollState{}
			s.scroll[tab.ID] = scroll
		}
		ui.Scroll(c.Key(tab.ID)).Label("Resource contents").TrackScroll(scroll).Grow(1).MinHeight(0).FillWidth().Padding(16).Gap(16).Children(func() {
			page := p.resources.page
			if page.Processing != "" && (page.Body.Title == "" || p.resources.metadata) {
				ui.Text(c, page.Processing).FontSize(workspaceFontCaption).TextColor(c.Theme().TextMuted)
			}
			for _, entry := range page.Entries {
				entry := entry
				location := entry.Location
				key := strings.Join([]string{location.Kind, location.BranchID, location.URI, location.TeamID, location.WorkerID, location.TaskID}, "\x00")
				label := location.Title
				if location.File != nil {
					key += "\x00" + location.File.ID
					label = location.File.Path
				}
				ui.Column(c.Key(key)).FillWidth().MinWidth(0).Gap(4).Children(func() {
					workspaceAction(c, "open", location.Title, "file-text", false).Label("Open resource " + label).OnClick(func() { p.openWorkspaceResource(location) })
					ui.Text(c, entry.Detail).FontSize(workspaceFontCaption).TextColor(c.Theme().TextMuted)
				})
			}
			body := page.Body
			if body.Title != "" {
				p.buildWorkspaceResourceBody(c, body)
			} else if !p.resources.loading && p.resources.problem == "" && len(page.Entries) == 0 {
				ui.Text(c, "当前范围没有已提交资料。").FontSize(workspaceFontCaption).TextColor(c.Theme().TextMuted)
			}
		})
		page := p.resources.page
		if !page.HasMore && !page.Body.HasMore && tab.Offset == 0 && tab.Cursor == 0 && tab.After == "" {
			return
		}
		ui.Row(c).FillWidth().Padding(0, 24, 12, 24).Gap(8).Shrink(0).Children(func() {
			workspaceControlIconButton(c, "resource-start", "起始位置", "chevron-left", "ghost", p.resources.loading || tab.Offset == 0 && tab.Cursor == 0 && tab.After == "").Grow(1).MinWidth(0).Height(32).OnClick(func() {
				tab.Offset = 0
				tab.Cursor = 0
				tab.After = ""
				tab.ScrollX, tab.ScrollY = 0, 0
				s.scroll[tab.ID] = &ui.ScrollState{}
				p.changedWorkspaceResources()
				p.loadWorkspaceResource()
			})
			workspaceControlIconButton(c, "resource-next", "下一范围", "chevron-right", "ghost", p.resources.loading || !page.HasMore && !page.Body.HasMore).Grow(1).MinWidth(0).Height(32).OnClick(func() {
				if page.Body.Title != "" {
					tab.Offset = page.Body.NextOffset
				} else {
					tab.Cursor = page.Next
					tab.After = page.After
				}
				tab.ScrollX, tab.ScrollY = 0, 0
				s.scroll[tab.ID] = &ui.ScrollState{}
				p.changedWorkspaceResources()
				p.loadWorkspaceResource()
			})
		})
	})
}

func (p *NativePage) buildWorkspaceResourceBody(c *ui.Context, body product.WorkspaceResourceBody) {
	if p.resources.metadata {
		ui.Column(c).Label("Resource verification details").FillWidth().Gap(8).Children(func() {
			ui.Text(c, fmt.Sprintf("%d–%d / %d bytes", body.Offset, body.NextOffset, body.TotalBytes)).FontSize(workspaceFontCaption).TextColor(c.Theme().TextMuted)
			if body.MediaType != "" {
				ui.Text(c, body.MediaType).FontSize(workspaceFontCaption)
			}
			if body.SHA256 != "" {
				ui.Text(c, "SHA-256 "+body.SHA256).FontSize(workspaceFontCaption).FillWidth().Selectable()
			}
			if body.Processing != "" {
				ui.Text(c, body.Processing).FontSize(workspaceFontCaption).TextColor(c.Theme().TextMuted)
			}
		})
	}
	text := string(body.Data)
	binary := !utf8.Valid(body.Data) || strings.ContainsRune(text, 0)
	if binary {
		text = "Binary range (hex)\n" + hex.Dump(body.Data)
	}
	if text == "" {
		ui.Text(c, "此范围没有内容。").Role(ui.RoleStatus).FontSize(workspaceFontBody).TextColor(c.Theme().TextMuted)
		return
	}
	if p.resources.find && p.resources.findQuery != "" {
		query := p.resources.findQuery
		count := strings.Count(text, query)
		label := fmt.Sprintf("当前载入内容：%d 处匹配", count)
		if count > 128 {
			label += "；高亮前 128 处"
		}
		ui.Text(c, label).Role(ui.RoleStatus).FontSize(workspaceFontCaption).TextColor(c.Theme().TextMuted)
		// At most 128 highlighted matches; neither search nor rendering loads
		// another range. Every original byte remains in the displayed text.
		parts := strings.SplitN(text, query, 129)
		spans := make([]ui.Span, 0, len(parts)*2)
		for i, part := range parts {
			if i > 0 {
				spans = append(spans, ui.Span{Text: query, Background: workspacePalette(c).selection})
			}
			spans = append(spans, ui.Span{Text: part})
		}
		ui.Column(c).Label("Actual resource bytes").FillWidth().MinWidth(0).Selectable().Children(func() {
			ui.RichText(c, spans...).FontSize(c.Theme().FontSize).LineHeight(1.75).FillWidth().MinWidth(0)
		})
		return
	}
	name := strings.ToLower(body.Title)
	if !p.resources.raw && !binary && (body.MediaType == "text/markdown" || strings.HasSuffix(name, ".md") || strings.HasSuffix(name, ".markdown")) {
		theme := markdown.Theme{FontSize: c.Theme().FontSize, LineHeight: 1.75, BlockSpacing: 24.0 / 14}
		for i := range theme.Headings {
			size := float32(16.0 / 14)
			if i == 0 {
				size = 24.0 / 14
			}
			theme.Headings[i] = markdown.HeadingStyle{Size: size, Weight: 600, Top: 24.0 / 14, Bottom: 8.0 / 14}
		}
		theme.Headings[0].Bottom = 24.0 / 14
		p.resources.markdown.Theme = &theme
		p.resources.markdown.WrapCode = true
		// Render this complete bounded source once: reference definitions,
		// footnotes and heading anchors share the same parser context. MyGo
		// currently inherits the prose line box for headings; do not slice
		// source to compensate and silently change document semantics.
		ui.Column(c).Label("Resource document").FillWidth().MinWidth(0).Children(func() {
			markdown.View(c, text, &p.resources.markdown)
		})
		return
	}
	ui.Column(c).Label("Actual resource bytes").FillWidth().MinWidth(0).Selectable().Children(func() {
		ui.Text(c, text).FontSize(c.Theme().FontSize).LineHeight(1.75).FillWidth().MinWidth(0)
	})
}
