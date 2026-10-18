// INPUT:  context, MyGo window/update, host.Controller
// OUTPUT: Run and RunWithDebug native desktop entry points with optional separately owned diagnostics
// POS: [L6 application]    Binds runtime-owned execution to UI-thread-confined native presentation
//go:build desktop

package desktop

import (
	"context"
	"github.com/QuincySx/Alva/go/apps/debug"

	"github.com/QuincySx/Alva/go/platform/host"
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

// Run creates native UI only. The caller owns controller shutdown after Run exits.
func Run(controller *host.Controller, demo bool, routeLabel string) error {
	return RunWithDebug(controller, demo, routeLabel, nil)
}

func RunWithDebug(controller *host.Controller, demo bool, routeLabel string, diagnostics *debug.Server) error {
	m := NewModel(demo)
	m.RouteLabel = routeLabel
	v := NewNativeView(m)
	if diagnostics != nil && !diagnostics.Closed() {
		v.diagnosticLog = diagnostics.Logger()
	}
	mygo.App.WhenReady(func() {
		window := mygo.NewWindow(mygo.WindowOptions{
			Title: "ALVA · Go native foundation", Width: 1040, Height: 800,
			MinWidth: 720, MinHeight: 600,
			Content: ui.View(v.Build),
		})
		observerCtx, detach := context.WithCancel(context.Background())
		window.OnClosed(detach)
		window.OnClose(func(event *mygo.CloseEvent) {
			if m.ActiveRunID != "" {
				if v.diagnosticChanged != nil {
					v.diagnosticChanged()
				}
				event.PreventDefault()
				m.Error = "A run is active. Use Stop, wait for acknowledgement, then close. Closing an observer is not Stop."
				window.Invalidate()
			}
		})
		v.Send = func() {
			v.traceUI("send")
			prompt, err := m.Prompt()
			if err != nil {
				m.FailAdmission(err)
				return
			}
			id, err := controller.StartPrompt(context.Background(), prompt)
			if err != nil {
				m.FailAdmission(err)
				return
			}
			m.Admitted(id, prompt)
			v.list.ScrollToEnd()
			go observe(observerCtx, controller, id, func(snapshot host.RunSnapshot) {
				applied := make(chan struct{})
				window.Update(func() {
					if v.diagnosticChanged != nil {
						v.diagnosticChanged()
					}
					defer close(applied)
					if observerCtx.Err() == nil {
						m.Observe(snapshot.ID, snapshot.HighWater, snapshot.Text, Status(snapshot.Status), snapshot.Error)
					}
				})
				// Window.Update is asynchronous. Bound this observer to one queued
				// UI closure; slow UI causes bounded replay expiry and resnapshot,
				// not an unbounded queue of captured transcript strings.
				select {
				case <-applied:
				case <-observerCtx.Done():
				}
			}, func(err error) {
				window.Update(func() {
					if v.diagnosticChanged != nil {
						v.diagnosticChanged()
					}
					if m.ActiveRunID == id {
						m.Error = "Observation failed; execution was not retried: " + err.Error()
					}
				})
			})
		}
		v.Stop = func() {
			v.traceUI("stop")
			if id := m.RequestStop(); id != "" {
				if err := controller.Stop(id); err != nil {
					m.Status, m.Error = Running, "Stop request failed: "+err.Error()
				}
			}
		}
		if diagnostics != nil && !diagnostics.Closed() {
			owner, methods, err := v.Diagnostic(observerCtx, window.Update, window.CapturePage, func() *debug.Bounds {
				width, height := window.ContentSize()
				return &debug.Bounds{Width: float64(width), Height: float64(height)}
			})
			if err == nil {
				v.diagnosticFrame, v.diagnosticChanged = owner.TrackRendering(window.Invalidate)
				var unregister func()
				unregister, err = diagnostics.Register("direct-window", "AlvaDirectWindow", methods, owner)
				if err == nil {
					window.OnClosed(unregister)
					diagnostics.Logger().Info("native diagnostic view registered", "view", "direct-window")
				}
			}
			if err != nil {
				m.Error = "Native diagnostic view registration failed"
				window.Invalidate()
			}
		}
	})
	return mygo.App.Run()
}
