// INPUT: One shared HubService, MyGo UI-thread window actions and page-local view state
// OUTPUT: Bounded independent native windows observing the same durable execution owners
// POS: [L7 Hub product] Closing an observer window never stops or duplicates a shared product Turn.
//go:build desktop

package hubdesktop

import (
	"context"
	"errors"
	"fmt"
	"github.com/QuincySx/Alva/go/apps/debug"
	"github.com/QuincySx/Alva/go/apps/hub/product"
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"io"
	"time"
)

// All fields are confined to the MyGo UI thread, just like NativePage.
type nativeWindows struct {
	service        *product.HubService
	output         io.Writer
	pages          map[*NativePage]struct{}
	diagnostics    *debug.Server
	diagnosticNext uint64
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
	ctx, detach := context.WithCancel(context.Background())
	p.ctx = ctx
	p.window = mygo.NewWindow(mygo.WindowOptions{Title: "ALVA Hub · Go native", Width: 1240, Height: 850, MinWidth: 960, MinHeight: 680, Content: ui.View(p.Build)})
	m.pages[p] = struct{}{}
	p.window.OnClosed(func() { delete(m.pages, p); p.gatewayCredentials = ""; detach() })
	go func() {
		ticker := time.NewTicker(250 * time.Millisecond)
		defer ticker.Stop()
		observePage(ctx, p.window.Update, p.poll, ticker.C)
	}()
	p.window.OnClose(func(event *mygo.CloseEvent) {
		// Another window can keep observing the actual runtime. The final
		// window still requires explicit Stop while execution is active.
		if p.busy || p.playerBusy || p.gatewayBusy || p.inspectExportBusy || len(m.pages) == 1 && m.service.Runtime.HasActive() {
			if p.diagnosticChanged != nil {
				p.diagnosticChanged()
			}
			event.PreventDefault()
			p.problem = "Work or local playback is active. Stop its actual owner and wait for cleanup before closing the last observer."
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
	p.refreshSessions()
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
