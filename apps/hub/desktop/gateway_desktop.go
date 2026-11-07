// INPUT: Explicit local Gateway service and one-time generated listener credentials
// OUTPUT: Native cold status and explicit start/stop controls with copyable ephemeral bearer
// POS: [L7 Hub product] Application-level model API settings never execute an Agent or start on observation.
//go:build desktop

package hubdesktop

import (
	"context"
	"github.com/egoist/mygo/ui"
	"strconv"
	"time"
)

func (p *NativePage) gatewayAction(start bool, expectedGeneration uint64) {
	if p.gatewayBusy || p.Service.Gateway == nil {
		return
	}
	p.gatewayBusy = true
	go func() {
		ctx, cancel := context.WithTimeout(p.ctx, 5*time.Second)
		defer cancel()
		credentials := ""
		var generation uint64
		var err error
		if start {
			value, e := p.Service.Gateway.Start(ctx, true)
			err = e
			if err == nil {
				credentials = value.Address + "\nBearer " + value.Token
				generation = value.Generation
			}
		} else {
			err = p.Service.Gateway.StopGeneration(ctx, expectedGeneration)
		}
		p.dispatch(func() {
			p.gatewayBusy = false
			p.observeGateway()
			if err != nil {
				p.problem = err.Error()
				return
			}
			p.problem = ""
			p.gatewayCredentials = credentials
			p.gatewayGeneration = generation
			p.observeGateway()
		})
	}()
}

// One-time credentials belong to the exact listener that returned them, even
// when another observer stops/restarts before this window receives its callback.
func (p *NativePage) gatewayCredentialsCurrent() bool {
	if p.gatewayCredentials == "" || p.Service.Gateway == nil {
		return false
	}
	status := p.gatewayStatus
	return status.Running && !status.Closed && !status.Stopping && status.Generation == p.gatewayGeneration
}

func (p *NativePage) observeGateway() {
	if p.Service.Gateway != nil {
		p.gatewayStatus = p.Service.Gateway.Snapshot()
	}
	if !p.gatewayCredentialsCurrent() {
		p.gatewayCredentials = ""
		p.gatewayGeneration = 0
	}
}

func (p *NativePage) buildGateway(c *ui.Context) {
	ui.Column(c).Grow(1).Gap(8).Children(func() {
		ui.Text(c, "Local model API").Bold()
		if p.Service.Gateway == nil {
			ui.Text(c, "An explicitly configured live model route is required.")
			return
		}
		status := p.gatewayStatus
		ui.Text(c, "This API uses the configured live model account. Each caller can incur model charges. It accepts explicit model requests; it does not run Agent tools or use conversation history.").FontSize(workspaceFontCaption)
		if status.Running {
			ui.Text(c, status.Address)
			if status.Stopping {
				ui.Text(c, "Local API admission is closed. Waiting for active requests to finish; retry Stop to join cleanup.")
			}
			// A press belongs to this listener even if polling replaces the visible controls.
			ui.Button(c.Key("gateway-stop-"+strconv.FormatUint(status.Generation, 10)), "Stop local model API and cancel active requests").Disabled(p.gatewayBusy).OnClick(func() { p.gatewayAction(false, status.Generation) })
		} else if status.Closed {
			ui.Text(c, "The local model API owner is closed.")
		} else {
			ui.Button(c, "Approve model account usage and start local API").Disabled(p.gatewayBusy).OnClick(func() { p.gatewayAction(true, 0) })
		}
		if p.gatewayCredentialsCurrent() {
			ui.Text(c, "Copy these credentials now. They stay in this window only and change after a restart.").FontSize(11)
			ui.TextArea(c.Key("gateway-credentials"), &p.gatewayCredentials).Label("Local API credentials").ReadOnly(true).Height(80)
		}
	})
}
