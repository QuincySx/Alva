// INPUT: SettingsService MCP views, explicit probe/authorization actions and the system browser opener
// OUTPUT: MCP server list, editor, connection status, tool list and OAuth authorize/revoke controls
// POS: [L7 Hub UI] Viewing never starts a server; checks and authorization are explicit and bound to this window's form
//go:build desktop

package hubdesktop

import (
	"context"
	"fmt"
	"net/url"
	"strconv"
	"strings"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

type settingsMCPState struct {
	probing     string
	authorizing string
	cancel      context.CancelFunc
	expanded    map[string]bool
	credentials map[string]product.MCPCredential
	loadedCreds bool
	problem     map[string]string
}

var mcpTransportLabels = []string{"本地命令", "远程 HTTP"}
var mcpAuthLabels = []string{"无", "OAuth"}

func initMCPEditor(p *NativePage) {
	p.settingsForm.editor = nil
	p.loadMCPCredentials()
}

// loadMCPCredentials reads only whether an authorization exists; token values
// never reach the window.
func (p *NativePage) loadMCPCredentials() {
	f := &p.settingsForm
	if p.Service.Settings == nil || f.mcp.loadedCreds {
		return
	}
	f.mcp.loadedCreds = true
	var ids []string
	for _, server := range f.view.Capabilities.MCP {
		if server.Remote() && server.Auth == hub.MCPAuthOAuth {
			ids = append(ids, server.ID)
		}
	}
	if len(ids) == 0 {
		return
	}
	epoch, pane, scope := f.epoch, p.pane, f.scope
	go func() {
		result := map[string]product.MCPCredential{}
		for _, id := range ids {
			credential, err := p.Service.Settings.MCPCredential(p.ctx, scope, id)
			if err != nil {
				credential = product.MCPCredential{State: product.MCPCredentialNone, Problem: err.Error()}
			}
			result[id] = credential
		}
		p.dispatch(func() {
			if f.epoch != epoch || p.pane != pane {
				return
			}
			f.mcp.credentials = result
		})
	}()
}

func (p *NativePage) editMCP(index int) {
	f := &p.settingsForm
	f.errors, f.notice = nil, ""
	e := newSettingsEditor(hub.CapabilityMCP, index)
	server := hub.MCPServer{}
	if index >= 0 {
		server = f.view.Capabilities.MCP[index]
	}
	*e.str("id"), *e.str("name"), *e.str("command") = server.ID, server.Name, server.Command
	e.setList("args", server.Args)
	e.setEnv("env", server.Env)
	*e.str("url"), *e.str("client") = server.URL, server.OAuthClientID
	e.setEnv("headers", server.Headers)
	*e.str("transport") = mcpTransportLabels[0]
	if server.Remote() {
		*e.str("transport") = mcpTransportLabels[1]
	}
	*e.str("auth") = mcpAuthLabels[0]
	if server.Auth == hub.MCPAuthOAuth {
		*e.str("auth") = mcpAuthLabels[1]
	}
	f.editor = e
}

func (p *NativePage) saveMCPServer() {
	f := &p.settingsForm
	e := f.editor
	var problems product.FieldErrors
	id := strings.TrimSpace(*e.str("id"))
	field := "mcp." + id
	server := hub.MCPServer{ID: id, Name: strings.TrimSpace(*e.str("name"))}
	if *e.str("transport") == mcpTransportLabels[1] {
		server.Transport = hub.MCPTransportHTTP
		server.URL = strings.TrimSpace(*e.str("url"))
		server.Headers = e.environment("headers", field+".headers", &problems)
		if *e.str("auth") == mcpAuthLabels[1] {
			server.Auth = hub.MCPAuthOAuth
			server.OAuthClientID = strings.TrimSpace(*e.str("client"))
		}
	} else {
		server.Command = strings.TrimSpace(*e.str("command"))
		server.Args = e.values("args")
		server.Env = e.environment("env", field+".env", &problems)
	}
	if len(problems) > 0 {
		f.errors = problems
		return
	}
	desired := f.view.Capabilities.Clone()
	if e.index >= 0 {
		desired.MCP[e.index] = server
	} else {
		desired.MCP = append(desired.MCP, server)
	}
	f.mcp.loadedCreds = false
	p.saveSettingsSection(product.SectionMCP, desired, false)
}

func (p *NativePage) deleteMCP(index int, label string) {
	f := &p.settingsForm
	f.confirm = &settingsConfirm{key: "delete-mcp-" + strconv.Itoa(index), message: "删除 MCP 服务器“" + label + "”？已保存的 OAuth 授权需要另外撤销。", action: "删除", danger: true, run: func() {
		desired := f.view.Capabilities.Clone()
		desired.MCP = append(desired.MCP[:index:index], desired.MCP[index+1:]...)
		p.saveSettingsSection(product.SectionMCP, desired, false)
	}}
}

func (p *NativePage) probeMCP(serverID string) {
	f := &p.settingsForm
	if f.mcp.probing != "" || p.Service.Settings == nil {
		return
	}
	f.mcp.probing = serverID
	if f.mcp.problem == nil {
		f.mcp.problem = map[string]string{}
	}
	delete(f.mcp.problem, serverID)
	epoch, pane, scope := f.epoch, p.pane, f.scope
	go func() {
		ctx, cancel := context.WithTimeout(p.ctx, 90*time.Second)
		defer cancel()
		_, err := p.Service.Settings.ProbeMCP(ctx, scope, serverID)
		p.dispatch(func() {
			if f.epoch != epoch || p.pane != pane {
				return
			}
			f.mcp.probing = ""
			if err != nil {
				f.mcp.problem[serverID] = err.Error()
				return
			}
			if f.mcp.expanded == nil {
				f.mcp.expanded = map[string]bool{}
			}
			f.mcp.expanded[serverID] = true
		})
	}()
}

func (p *NativePage) authorizeMCP(serverID string, revoke bool) {
	f := &p.settingsForm
	if f.mcp.authorizing != "" || p.Service.Settings == nil {
		return
	}
	if f.mcp.problem == nil {
		f.mcp.problem = map[string]string{}
	}
	delete(f.mcp.problem, serverID)
	ctx, cancel := context.WithTimeout(p.ctx, 6*time.Minute)
	f.mcp.authorizing, f.mcp.cancel = serverID, cancel
	epoch, pane, scope := f.epoch, p.pane, f.scope
	go func() {
		defer cancel()
		var credential product.MCPCredential
		var err error
		if revoke {
			credential, err = p.Service.Settings.RevokeMCP(ctx, scope, serverID)
		} else {
			credential, err = p.Service.Settings.AuthorizeMCP(ctx, scope, serverID, func(address string) error { return mygo.Shell.OpenExternal(address) })
		}
		p.dispatch(func() {
			if f.epoch != epoch || p.pane != pane {
				return
			}
			f.mcp.authorizing, f.mcp.cancel = "", nil
			if f.mcp.credentials == nil {
				f.mcp.credentials = map[string]product.MCPCredential{}
			}
			if credential.State != "" {
				f.mcp.credentials[serverID] = credential
			}
			if err != nil {
				f.mcp.problem[serverID] = err.Error()
			}
		})
	}()
}

func mcpPlaintext(server hub.MCPServer) bool {
	address, err := url.Parse(server.URL)
	return server.Remote() && err == nil && address.Scheme == "http" && !hub.LoopbackHost(address.Hostname())
}

func (p *NativePage) buildMCPServerRow(c *ui.Context, index int, server hub.MCPServer) {
	f := &p.settingsForm
	t := c.Theme()
	title := server.Name
	if title == "" {
		title = server.ID
	}
	detail := "本地 · " + server.Command
	if server.Remote() {
		detail = "远程 · " + server.URL
	}
	ui.Column(c.Key("mcp-row-"+server.ID)).Label("MCP server "+server.ID).FillWidth().Gap(4).Padding(12).Radius(8).Border(1, t.Border).Children(func() {
		p.settingsSwitchRow(c, hub.CapabilityMCP, server.ID, title, detail)
		if mcpPlaintext(server) {
			ui.Row(c).Gap(6).AlignItems(ui.Center).Children(func() {
				ui.Icon(c, workspaceIcon("triangle-alert")).Size(workspaceIconSmall, workspaceIconSmall).TextColor(t.Warning)
				ui.Text(c, "明文传输：地址使用 http，请求头和数据可能被同一网络中的人看到。").FontSize(workspaceFontCaption).TextColor(t.Warning).Wrap()
			})
		}
		probe, probed := p.Service.Settings.Probe(f.scope, server.ID)
		status := "未检测"
		color := t.TextMuted
		switch {
		case f.mcp.probing == server.ID:
			status = "正在连接…"
		case probed && probe.State == product.MCPProbeConnected:
			status = fmt.Sprintf("已连接 · %d 个工具", probe.TotalTools)
			if probe.ServerName != "" {
				status += " · " + probe.ServerName + " " + probe.ServerVersion
			}
			color = t.Success
		case probed && probe.State == product.MCPProbeNeedsAuthorization:
			status, color = "需要授权："+probe.Problem, t.Warning
		case probed:
			status, color = "连接失败："+probe.Problem, t.Danger
		}
		ui.Text(c, status).Label("MCP status " + server.ID).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(color).Wrap()
		if server.Remote() && server.Auth == hub.MCPAuthOAuth {
			p.buildMCPCredential(c, server)
		}
		if problem := f.mcp.problem[server.ID]; problem != "" {
			ui.Text(c, problem).Role(ui.RoleStatus).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(t.Danger).Wrap()
		}
		ui.Row(c).Gap(8).AlignItems(ui.Center).Children(func() {
			item, _ := f.view.Item(hub.CapabilityMCP, server.ID)
			canProbe := item.Approved && f.view.SessionID != "" && f.mcp.probing == ""
			workspaceControlIconButton(c, "mcp-probe-"+server.ID, "测试连接", "refresh-cw", "secondary", !canProbe || f.saving).Label("Test MCP connection " + server.ID).
				Tooltip("在当前任务的工作区启动一次，列出工具后关闭").OnClick(func() { p.probeMCP(server.ID) })
			if probed && probe.State == product.MCPProbeConnected && len(probe.Tools) > 0 {
				label := "显示工具"
				if f.mcp.expanded[server.ID] {
					label = "收起工具"
				}
				workspaceControlButton(c, "mcp-tools-"+server.ID, label, "ghost").Label("Toggle MCP tools " + server.ID).OnClick(func() {
					if f.mcp.expanded == nil {
						f.mcp.expanded = map[string]bool{}
					}
					f.mcp.expanded[server.ID] = !f.mcp.expanded[server.ID]
				})
			}
			workspaceControlIconButton(c, "mcp-edit-"+server.ID, "编辑", "pencil", "ghost", f.saving).Label("Edit MCP server " + server.ID).OnClick(func() { p.editMCP(index) })
			workspaceControlIconButton(c, "mcp-delete-"+server.ID, "删除", "trash-2", "danger", f.saving).Label("Delete MCP server " + server.ID).OnClick(func() { p.deleteMCP(index, title) })
		})
		if state := mcpItem(f.view, server.ID); !state.Approved && f.view.SessionID == "" && state.Enabled {
			ui.Text(c, "选择任务后才能测试连接。").FontSize(workspaceFontCaption).TextColor(t.TextMuted)
		}
		if probed && probe.State == product.MCPProbeConnected && f.mcp.expanded[server.ID] {
			ui.Column(c.Key("mcp-tools-list-"+server.ID)).Label("MCP tools "+server.ID).FillWidth().Gap(2).Padding(4, 0).Children(func() {
				for _, tool := range probe.Tools {
					line := tool.Name
					if tool.Description != "" {
						line += " — " + tool.Description
					}
					ui.Text(c, line).FontSize(workspaceFontCaption).LineHeight(1.5).Wrap()
				}
				if probe.TotalTools > len(probe.Tools) {
					ui.Text(c, fmt.Sprintf("另有 %d 个工具未列出。", probe.TotalTools-len(probe.Tools))).FontSize(workspaceFontCaption).TextColor(t.TextMuted)
				}
			})
		}
	})
}

func mcpItem(view product.SettingsView, serverID string) product.SettingsItem {
	value, _ := view.Item(hub.CapabilityMCP, serverID)
	return value
}

func (p *NativePage) buildMCPCredential(c *ui.Context, server hub.MCPServer) {
	f := &p.settingsForm
	t := c.Theme()
	credential, known := f.mcp.credentials[server.ID]
	line, color := "授权状态读取中…", t.TextMuted
	if known {
		switch credential.State {
		case product.MCPCredentialAuthorized:
			line, color = "已授权", t.Success
			if credential.Issuer != "" {
				line += " · " + credential.Issuer
			}
			if credential.ExpiresAt > 0 {
				line += " · 令牌到期 " + time.UnixMilli(credential.ExpiresAt).Format("01-02 15:04")
			}
		case product.MCPCredentialExpired:
			line, color = "授权已过期，请重新授权", t.Warning
		case product.MCPCredentialUnsupported:
			line, color = "此平台不能安全保存令牌："+credential.Problem, t.Danger
		default:
			line = "未授权"
			if credential.Problem != "" {
				line += "：" + credential.Problem
			}
		}
	}
	ui.Row(c.Key("mcp-oauth-" + server.ID)).Gap(8).AlignItems(ui.Center).Children(func() {
		ui.Icon(c, workspaceIcon("key-round")).Size(workspaceIconSmall, workspaceIconSmall).TextColor(color)
		ui.Text(c, line).Label("MCP authorization " + server.ID).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(color).Grow(1).MinWidth(0).Wrap()
		if f.mcp.authorizing == server.ID {
			ui.Text(c, "等待浏览器中完成授权…").FontSize(workspaceFontCaption).TextColor(t.TextMuted)
			workspaceControlButton(c, "mcp-oauth-cancel-"+server.ID, "取消", "secondary").Label("Cancel MCP authorization " + server.ID).OnClick(func() {
				if f.mcp.cancel != nil {
					f.mcp.cancel()
				}
			})
			return
		}
		busy := f.mcp.authorizing != "" || f.saving
		label := "授权"
		if known && credential.State != product.MCPCredentialNone && credential.State != product.MCPCredentialUnsupported {
			label = "重新授权"
		}
		workspaceControlIconButton(c, "mcp-oauth-start-"+server.ID, label, "external-link", "secondary", busy || known && credential.State == product.MCPCredentialUnsupported).
			Label("Authorize MCP server " + server.ID).Tooltip("在系统浏览器中打开授权页面").OnClick(func() { p.authorizeMCP(server.ID, false) })
		if known && (credential.State == product.MCPCredentialAuthorized || credential.State == product.MCPCredentialExpired) {
			workspaceControlButton(c, "mcp-oauth-revoke-"+server.ID, "撤销授权", "danger", busy).Label("Revoke MCP authorization " + server.ID).OnClick(func() { p.authorizeMCP(server.ID, true) })
		}
	})
}

func (p *NativePage) buildSettings(c *ui.Context) {
	p.ensureSettingsForm(initMCPEditor)
	f := &p.settingsForm
	settingsPageTitle(c, "MCP 服务器", "为 Alva 接入本地命令或远程 HTTP 的 MCP 服务器。保存不会启动服务器；对话中按需连接，每次调用仍需审批。")
	p.buildSettingsScope(c)
	p.buildSettingsStatus(c)
	if !f.loaded {
		return
	}
	ui.Scroll(c).Grow(1).FillWidth().MinHeight(0).Children(func() {
		ui.Column(c).FillWidth().Gap(12).Children(func() {
			if len(f.view.Capabilities.MCP) == 0 {
				ui.Text(c, "还没有 MCP 服务器。").FontSize(workspaceFontBody).TextColor(c.Theme().TextMuted)
			}
			for i, server := range f.view.Capabilities.MCP {
				p.buildMCPServerRow(c, i, server)
			}
			workspaceControlIconButton(c, "mcp-add", "添加服务器", "plus", "secondary", f.saving || len(f.view.Capabilities.MCP) >= 32).Label("Add MCP server").OnClick(func() { p.editMCP(-1) })
			if e := f.editor; e != nil && e.kind == hub.CapabilityMCP {
				p.buildMCPEditor(c, e)
			}
			ui.Row(c).FillWidth().Justify(ui.End).Children(func() {
				workspaceControlButton(c, "mcp-reset", "恢复默认", "ghost", f.saving).Label("Reset MCP servers to defaults").OnClick(func() {
					p.resetSettingsSection(product.SectionMCP, "停用全部 MCP 服务器？配置和授权会保留。")
				})
			})
		})
	})
}

func (p *NativePage) buildMCPEditor(c *ui.Context, e *settingsEditor) {
	f := &p.settingsForm
	t := c.Theme()
	id := *e.str("id")
	field := "mcp." + id
	title := "添加 MCP 服务器"
	if e.index >= 0 {
		title = "编辑 MCP 服务器"
	}
	settingsGroup(c, "mcp-editor", title, "改动连接配置后，需要在每个工作区重新批准。", func() {
		if e.index < 0 {
			p.settingsTextField(c, "mcp-id", "标识", field+".id", "例如 github，只能用字母、数字和 . _ : -", e.str("id"))
		}
		p.settingsTextField(c, "mcp-name", "显示名称（可选）", field+".name", "", e.str("name"))
		ui.Text(c, "类型").FontSize(workspaceFontBody)
		workspaceControlSelect(c, "mcp-transport", e.str("transport"), mcpTransportLabels).Label("MCP transport").FillWidth().Disabled(f.saving)
		if *e.str("transport") == mcpTransportLabels[1] {
			p.settingsTextField(c, "mcp-url", "地址", field+".url", "https://example.com/mcp", e.str("url"))
			if address, err := url.Parse(strings.TrimSpace(*e.str("url"))); err == nil && address.Scheme == "http" && !hub.LoopbackHost(address.Hostname()) {
				ui.Text(c, "明文传输：http 地址上的数据和请求头没有加密。").FontSize(workspaceFontCaption).TextColor(t.Warning).Wrap()
			}
			p.settingsEnvField(c, "mcp-headers", "请求头", field+".headers", "例如 X-Api-Key", e.env("headers"))
			ui.Text(c, "认证").FontSize(workspaceFontBody)
			workspaceControlSelect(c, "mcp-auth", e.str("auth"), mcpAuthLabels).Label("MCP authorization").FillWidth().Disabled(f.saving)
			p.settingsFieldError(c, field+".auth")
			if *e.str("auth") == mcpAuthLabels[1] {
				ui.Text(c, "保存后在列表里点“授权”，会用系统浏览器打开授权页面。令牌保存在系统钥匙串。").FontSize(workspaceFontCaption).TextColor(t.TextMuted).Wrap()
				p.settingsTextField(c, "mcp-client", "Client ID（可选）", field+".oauth_client_id", "服务器不支持自动注册时填写", e.str("client"))
			}
		} else {
			p.settingsPathField(c, "mcp-command", "命令", field+".command", "例如 npx 或 /usr/local/bin/server", e.str("command"), false)
			p.settingsListField(c, "mcp-args", "参数", field+".args", "", e.list("args"))
			p.settingsEnvField(c, "mcp-env", "环境变量", field+".env", "名称", e.env("env"))
		}
		ui.Row(c).Gap(8).Justify(ui.End).FillWidth().Children(func() {
			workspaceControlButton(c, "mcp-cancel", "取消", "secondary", f.saving).Label("Cancel MCP edit").OnClick(func() { f.editor, f.errors = nil, nil })
			p.workspaceControlAction(c, "mcp-save", "保存", "", "primary", f.saving, p.saveMCPServer).Label("Save MCP server")
		})
	})
}
