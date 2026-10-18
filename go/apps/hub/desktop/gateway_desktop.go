// INPUT: Explicit local Gateway service and one-time generated listener credentials
// OUTPUT: Native cold status and explicit start/stop controls with copyable ephemeral bearer
// POS: [L7 Hub product] Application-level model API settings never execute an Agent or start on observation.
//go:build desktop

package hubdesktop

import (
	"context"
	"github.com/egoist/mygo/ui"
	"time"
)

func (p *NativePage) gatewayAction(start bool) {
	if p.gatewayBusy || p.Service.Gateway == nil {
		return
	}
	p.gatewayBusy = true
	go func() {
		ctx, cancel := context.WithTimeout(p.ctx, 5*time.Second)
		defer cancel()
		credentials := ""
		var err error
		if start {
			value, e := p.Service.Gateway.Start(ctx, true)
			err = e
			if err == nil {
				credentials = value.Address + "\nBearer " + value.Token
			}
		} else {
			err = p.Service.Gateway.Stop(ctx)
		}
		p.dispatch(func() {
			p.gatewayBusy = false
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.problem = ""
			p.gatewayCredentials = credentials
		})
	}()
}
func (p *NativePage) buildGateway(c *ui.Context) {
	ui.Column(c).Grow(1).Gap(8).Children(func() {
		ui.Text(c, "Local model API").Bold()
		if p.Service.Gateway == nil {
			ui.Text(c, "An explicitly configured live model route is required.")
			return
		}
		status := p.Service.Gateway.Snapshot()
		ui.Text(c, "This API uses the configured live model account. Each caller can incur model charges. It accepts explicit model requests; it does not run Agent tools or use conversation history.").FontSize(12)
		if status.Running {
			ui.Text(c, status.Address)
			ui.Button(c, "Stop local model API and cancel active requests").Disabled(p.gatewayBusy).OnClick(func() { p.gatewayAction(false) })
		} else {
			ui.Button(c, "Approve model account usage and start local API").Disabled(p.gatewayBusy).OnClick(func() { p.gatewayAction(true) })
		}
		if p.gatewayCredentials != "" {
			ui.Text(c, "Copy these credentials now. They stay in this window only and change after a restart.").FontSize(11)
			ui.TextArea(c.Key("gateway-credentials"), &p.gatewayCredentials).Label("Local API credentials").ReadOnly(true).Height(80)
		}
	})
}
