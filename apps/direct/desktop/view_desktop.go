// INPUT: Shared chat presentation, MyGo UI and product-owned diagnostic hooks
// OUTPUT: directView with direct-product framing and private diagnostic wiring
// POS: [L6 application] Direct window copy and diagnostics stay outside the shared chat controls.
//go:build desktop

package desktop

import (
	"log/slog"

	"github.com/QuincySx/Alva/apps/ui/chat"
	"github.com/egoist/mygo/ui"
)

type directView struct {
	*chat.NativeView
	diagnosticLog                      *slog.Logger
	diagnosticFrame, diagnosticChanged func()
}

func newDirectView(model *chat.Model) *directView {
	return &directView{NativeView: chat.NewNativeView(model)}
}

func (v *directView) Build(c *ui.Context) {
	if v.diagnosticFrame != nil {
		defer v.diagnosticFrame()
	}
	m := v.Model
	ui.Column(c).Fill().Padding(16).Gap(10).Children(func() {
		ui.Row(c).Gap(12).Children(func() {
			ui.Text(c, "ALVA · Go native foundation").FontSize(20).Bold().Grow(1)
			mode := "LIVE · explicit provider"
			if m.RouteLabel != "" {
				mode = "LIVE · " + m.RouteLabel
			}
			if m.Demo {
				mode = "DEMO · local fixture, no model service"
			}
			ui.Text(c, mode).FontSize(12).TextColor(c.Theme().TextMuted)
		})
		ui.Text(c, "Self-drawn MyGo · no WebView, React, JavaScript or frontend server").FontSize(12).TextColor(c.Theme().TextMuted)
		ui.Text(c, "In-process conversation only. Closing this app loses this session; restarting does not restore old Rust data.").FontSize(12)
		ui.Text(c, "This volatile mode sends text only. Use --data-dir for durable direct sessions, file references, approvals and Inspector; Hub owns multi-engine branches and teams.").FontSize(12).TextColor(c.Theme().TextMuted)
		v.NativeView.Build(c)
	})
}
