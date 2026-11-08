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
	t := c.Theme()
	ui.Column(c).Grow(1).FillWidth().Gap(16).Children(func() {
		settingsPageTitle(c, "Gateway", "在本机开放一个模型 API，供其他程序使用当前配置的模型账户。它只转发模型请求，不运行 Agent 工具，也不读取会话历史。")
		if p.Service.Gateway == nil {
			ui.Text(c, "需要先配置一个可用的模型连接。").FontSize(workspaceFontBody).TextColor(t.TextMuted)
			return
		}
		status := p.gatewayStatus
		ui.Text(c, "每个调用方的请求都会计入模型账户的费用。").FontSize(workspaceFontCaption).TextColor(t.Warning).Wrap()
		ui.Row(c.Key("gateway-status")).FillWidth().AlignItems(ui.Center).Gap(12).Padding(8, 0).Children(func() {
			ui.Column(c).Grow(1).MinWidth(0).Gap(2).Children(func() {
				switch {
				case status.Running:
					ui.Text(c, "运行中").FontSize(workspaceFontBody).TextColor(t.Success)
					ui.Text(c, status.Address).Label("Gateway address").FontSize(workspaceFontCaption).TextColor(t.TextMuted)
					if status.Stopping {
						ui.Text(c, "已停止接收新请求，正在等待进行中的请求结束；可以再次点击停止。").FontSize(workspaceFontCaption).TextColor(t.Warning).Wrap()
					}
				case status.Closed:
					ui.Text(c, "本机模型 API 已关闭，需要重启应用才能再次启动。").FontSize(workspaceFontBody).TextColor(t.TextMuted)
				default:
					ui.Text(c, "未运行").FontSize(workspaceFontBody).TextColor(t.TextMuted)
				}
			})
			switch {
			case status.Running:
				// A press belongs to this listener even if polling replaces the visible controls.
				workspaceControlButton(c, "gateway-stop-"+strconv.FormatUint(status.Generation, 10), "停止并取消进行中的请求", "danger", p.gatewayBusy).
					Label("Stop local model API and cancel active requests").OnClick(func() { p.gatewayAction(false, status.Generation) })
			case !status.Closed:
				workspaceControlButton(c, "gateway-start", "同意使用模型账户并启动", "primary", p.gatewayBusy).
					Label("Approve model account usage and start local API").OnClick(func() { p.gatewayAction(true, 0) })
			}
		})
		if p.gatewayCredentialsCurrent() {
			ui.Text(c, "现在复制这些凭据：它们只在这个窗口显示，重启后会更换。").FontSize(workspaceFontCaption).TextColor(t.Warning).Wrap()
			workspaceControlTextArea(c, "gateway-credentials", &p.gatewayCredentials).Label("Local API credentials").ReadOnly(true).Height(80).FillWidth()
		}
	})
}
