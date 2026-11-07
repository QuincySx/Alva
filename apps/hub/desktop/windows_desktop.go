// INPUT: One shared HubService, MyGo UI-thread window actions and page-local view state
// OUTPUT: Bounded native observers and guarded resource popout/return within the same application
// POS: [L7 Hub product] Window ownership and origin cleanup never stop or duplicate a shared product Turn.
//go:build desktop

package hubdesktop

import (
	"context"
	"errors"
	"fmt"
	"github.com/QuincySx/Alva/apps/debug"
	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"io"
	"runtime"
	"time"
)

// All fields are confined to the MyGo UI thread, just like NativePage.
type nativeWindows struct {
	service         *product.HubService
	output          io.Writer
	pages           map[*NativePage]struct{}
	resourceOrigins map[*NativePage]*NativePage
	resourceReturns map[*NativePage]string
	diagnostics     *debug.Server
	diagnosticNext  uint64
}

func (m *nativeWindows) open(sessionID string) error {
	if len(m.pages) >= 8 {
		return errors.New("at most eight Hub windows; close another observer before opening one")
	}
	p := NewNativePage(m.service)
	if m.diagnostics != nil && !m.diagnostics.Closed() {
		p.diagnosticLog = m.diagnostics.Logger()
	}
	p.openWindow = m.open
	p.openResourceWindow = func(id string, tab hub.WorkspaceTab) error { return m.openResourceFrom(p, id, tab) }
	ctx, detach := context.WithCancel(context.Background())
	p.ctx = ctx
	options := workspaceWindowOptions(runtime.GOOS)
	options.Content = ui.View(p.Build)
	p.window = mygo.NewWindow(options)
	m.pages[p] = struct{}{}
	p.window.OnClosed(func() {
		p.referenceOwner.close()
		p.closeWorkspaceResources()
		target := m.forgetPage(p)
		p.gatewayCredentials = ""
		detach()
		if target != nil && target.window != nil {
			target.window.Focus()
			mygo.App.Focus()
		}
	})
	go func() {
		ticker := time.NewTicker(250 * time.Millisecond)
		defer ticker.Stop()
		observePage(ctx, p.window.Update, p.poll, ticker.C)
	}()
	p.window.OnClose(func(event *mygo.CloseEvent) {
		if p.guardWorkspaceDraftClose(event) {
			p.window.Invalidate()
			return
		}
		if p.guardWorkspaceVoiceClose(event) {
			p.window.Invalidate()
			return
		}
		if p.prepareWorkspaceResourcesClose() {
			event.PreventDefault()
			p.window.Invalidate()
			return
		}
		if p.guardConnectionsClose(event) {
			p.window.Invalidate()
			return
		}
		if p.guardPreferencesClose(event) {
			p.window.Invalidate()
			return
		}
		if p.guardWorkspaceLayoutClose(event) {
			p.window.Invalidate()
			return
		}
		// Another window can keep observing the actual runtime. The final
		// window still requires explicit Stop while execution is active.
		if p.busy || p.goal.saving || p.composer.modelPicker.saving || p.companions.busy || p.companions.running || p.playerBusy || p.gatewayBusy || p.inspectExportBusy || len(m.pages) == 1 && m.service.Runtime.HasActive() {
			if p.diagnosticChanged != nil {
				p.diagnosticChanged()
			}
			event.PreventDefault()
			p.problem = "Work or local playback is active. Stop its actual owner and wait for cleanup before closing the last observer."
			p.window.Invalidate()
		}
		if len(m.pages) == 1 && m.service.Developer != nil && m.service.Developer.Snapshot().State == "running" {
			event.PreventDefault()
			p.openDeveloper()
			p.developer.problem = "Stop the developer run and wait for its actual request to finish before closing the last window."
			p.window.Invalidate()
		}
	})
	if m.diagnostics != nil && !m.diagnostics.Closed() {
		m.diagnosticNext++
		id := fmt.Sprintf("hub-window-%d", m.diagnosticNext)
		owner, methods, err := p.Diagnostic(ctx, p.window.Update, p.window.CapturePage, func() *debug.Bounds {
			width, height := p.window.ContentSize()
			return &debug.Bounds{Width: float64(width), Height: float64(height)}
		})
		if err == nil {
			p.diagnosticFrame, p.diagnosticChanged = owner.TrackRendering(p.window.Invalidate)
			var unregister func()
			unregister, err = m.diagnostics.Register(id, "AlvaHubWindow", methods, owner)
			if err == nil {
				p.window.OnClosed(unregister)
				m.diagnostics.Logger().Info("native diagnostic view registered", "view", id)
			}
		}
		if err != nil {
			p.problem = "Native diagnostic view registration failed"
			p.window.Invalidate()
		}
	}
	p.loadWorkspaceDraft()
	p.refreshSessions()
	p.refreshWorkspaceProjects()
	if sessionID != "" {
		p.selectSession(sessionID)
	}
	p.window.Focus()
	mygo.App.Focus()
	if m.output != nil {
		fmt.Fprintf(m.output, "native_qa: window created visible=%v bounds=%+v observers=%d\n", p.window.IsVisible(), p.window.Bounds(), len(m.pages))
	}
	return nil
}

// Resource popouts reuse this application and its original data owners. An
// already open resource window receives focus rather than another duplicate.
func (m *nativeWindows) openResourceFrom(origin *NativePage, sessionID string, tab hub.WorkspaceTab) error {
	key := sessionID + "\x00" + tab.ID
	if origin != nil && origin.resources.popout != "" {
		origin = m.resourceReturnTarget(origin)
	}
	if m.resourceOrigins == nil {
		m.resourceOrigins = make(map[*NativePage]*NativePage)
	}
	before := make(map[*NativePage]bool, len(m.pages))
	for page := range m.pages {
		if page.resources.popout == key {
			if origin != nil && origin != page {
				m.resourceOrigins[page] = origin
			}
			page.openWorkspaceResource(tab)
			page.window.Focus()
			return nil
		}
		before[page] = true
	}
	if err := m.open(sessionID); err != nil {
		return err
	}
	for page := range m.pages {
		if !before[page] {
			page.resources.popout = key
			page.resources.openRequested = true
			m.resourceOrigins[page] = origin
			page.returnResourceWindow = func() {
				if m.prepareResourceReturn(page) {
					// Use normal close guards. A pending save or cleanup keeps
					// this observer and its return request alive until retried.
					page.window.Close()
				} else {
					page.window.Invalidate()
				}
			}
			break
		}
	}
	return nil
}

// Resolve only live workbench windows. Returning a resource never navigates
// another observer away from its current session or unsaved settings form.
func (m *nativeWindows) resourceReturnTarget(popup *NativePage) *NativePage {
	if origin := m.resourceOrigins[popup]; origin != nil && origin != popup {
		if _, alive := m.pages[origin]; alive && origin.resources.popout == "" {
			return origin
		}
	}
	var fallback *NativePage
	for page := range m.pages {
		if page == popup || page.resources.popout != "" {
			continue
		}
		if page.selected == popup.selected {
			return page
		}
		if fallback == nil {
			fallback = page
		}
	}
	return fallback
}

func (m *nativeWindows) prepareResourceReturn(popup *NativePage) bool {
	if _, alive := m.pages[popup]; !alive || popup.resources.popout == "" {
		return false
	}
	if m.resourceReturnTarget(popup) == nil {
		// Its workbench was closed. Keep the final observer in this window
		// instead of closing the application or creating a duplicate client.
		popup.resources.popout = ""
		popup.returnResourceWindow = nil
		delete(m.resourceOrigins, popup)
		delete(m.resourceReturns, popup)
		if state := popup.workspaceResourceSession(); state != nil && state.view.Focused {
			state.view.Focused = false
			popup.changedWorkspaceResources()
		}
		return false
	}
	if m.resourceReturns == nil {
		m.resourceReturns = make(map[*NativePage]string)
	}
	m.resourceReturns[popup] = popup.resources.popout
	return true
}

// Drop both sides of the relation when a window really closes. A long-lived
// popout must not retain a closed workbench's messages, drafts or view caches.
func (m *nativeWindows) forgetPage(page *NativePage) *NativePage {
	var target *NativePage
	if key := m.resourceReturns[page]; key != "" && key == page.resources.popout {
		target = m.resourceReturnTarget(page)
	}
	delete(m.pages, page)
	delete(m.resourceOrigins, page)
	delete(m.resourceReturns, page)
	for popup, origin := range m.resourceOrigins {
		if origin == page {
			delete(m.resourceOrigins, popup)
		}
	}
	return target
}
