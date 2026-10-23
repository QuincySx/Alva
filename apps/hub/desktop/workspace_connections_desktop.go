// INPUT: Shared connection service, redacted committed DTOs and native MyGo controls.
// OUTPUT: Explicit connection CRUD, masked credential drafts and inline manual model selection.
// POS: [L7 Hub UI] Owns page-local drafts and navigation; product service owns policy, secrets and routes.
//go:build desktop

package hubdesktop

import (
	"errors"
	"fmt"
	"strings"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

type connectionHeaderDraft struct{ name, value string }

type modelConnectionsPane struct {
	entries                         []hub.ModelConnection
	selected, mode, source, query   string
	draft                           hub.ConnectionUpdate
	headers                         []connectionHeaderDraft
	modelID, modelName              string
	details, advanced, deleteReview bool
	loading, saving, readFailed     bool
	problem                         string
	focus                           ui.Handle
}

func (p *NativePage) openConnections() {
	p.setPane("connections")
	if p.connections.mode == "" {
		p.reloadConnections()
	}
}

func (p *NativePage) observeConnections() {
	if p.Service.Connections == nil {
		return
	}
	v := &p.connections
	if !v.loading && !v.saving {
		v.entries = p.Service.Connections.Snapshot()
		found := false
		for _, entry := range v.entries {
			found = found || entry.ID == v.selected
		}
		if !found && len(v.entries) > 0 {
			v.selected = v.entries[0].ID
		}
	}
	p.targets = p.Service.AvailableTargets()
}

func (p *NativePage) reloadConnections() {
	v := &p.connections
	if p.Service.Connections == nil || v.loading || v.saving {
		return
	}
	v.loading = true
	go func() {
		entries, err := p.Service.Connections.Read(p.ctx)
		p.dispatch(func() {
			v.loading = false
			if err != nil {
				v.readFailed = true
				v.problem = "无法读取连接。草稿已保留，请重试刷新。"
				return
			}
			v.entries, v.readFailed, v.problem = entries, false, ""
			if v.mode == "form" && v.draft.ID != "" {
				for _, entry := range entries {
					if entry.ID == v.draft.ID {
						p.editConnection(entry)
						break
					}
				}
			}
			p.observeConnections()
		})
	}()
}

func (p *NativePage) selectedConnection() (hub.ModelConnection, bool) {
	for _, entry := range p.connections.entries {
		if entry.ID == p.connections.selected {
			return entry, true
		}
	}
	return hub.ModelConnection{}, false
}

func (p *NativePage) chooseConnectionSource(source string) {
	v := &p.connections
	if v.saving || v.loading {
		return
	}
	// Returning through the source chooser keeps the same source's draft.
	if v.source != source {
		v.draft = hub.ConnectionUpdate{Auth: "api_key"}
		v.headers, v.modelID, v.modelName = nil, "", ""
		v.details, v.advanced = false, false
		switch source {
		case "OpenAI":
			v.draft.Name, v.draft.Protocol, v.draft.Endpoint = "OpenAI", "openai_responses", "https://api.openai.com/v1/responses"
		case "Anthropic":
			v.draft.Name, v.draft.Protocol, v.draft.Endpoint = "Anthropic", "anthropic", "https://api.anthropic.com/v1/messages"
		case "Gemini":
			v.draft.Name, v.draft.Protocol, v.draft.Endpoint = "Google Gemini", "gemini", "https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent"
		case "Local":
			v.draft.Name, v.draft.Protocol, v.draft.Auth = "本地模型服务", "openai_chat", "none"
		default:
			v.draft.Protocol = "openai_responses"
		}
	}
	v.source, v.mode, v.problem = source, "form", ""
	v.focus.Focus()
}

func (p *NativePage) editConnection(entry hub.ModelConnection) {
	v := &p.connections
	v.selected, v.source, v.mode = entry.ID, "Custom", "form"
	v.draft = hub.ConnectionUpdate{ID: entry.ID, ExpectedRevision: entry.Revision, Name: entry.Name,
		Protocol: entry.Protocol, Endpoint: entry.Endpoint, Auth: entry.Auth, Enabled: entry.Enabled,
		Models: append([]hub.ConnectionModel(nil), entry.Models...)}
	v.headers, v.modelID, v.modelName = nil, "", ""
	v.details, v.advanced, v.deleteReview, v.problem = true, false, false, ""
	v.focus.Focus()
}

func (p *NativePage) cancelConnectionDraft() {
	v := &p.connections
	if v.saving {
		return
	}
	v.mode, v.source, v.problem = "", "", ""
	v.draft, v.headers = hub.ConnectionUpdate{}, nil
	v.modelID, v.modelName = "", ""
}

func (p *NativePage) addConnectionModel() {
	v := &p.connections
	id := v.modelID
	if strings.TrimSpace(id) == "" || v.saving {
		return
	}
	for _, model := range v.draft.Models {
		if model.ID == id {
			return
		}
	}
	v.draft.Models = append(v.draft.Models, hub.ConnectionModel{ID: id, Name: v.modelName, Enabled: true})
	v.modelID, v.modelName = "", ""
}

func (p *NativePage) saveConnectionDraft() {
	v := &p.connections
	if p.Service.Connections == nil || v.saving || v.loading || v.readFailed {
		return
	}
	update := v.draft
	update.Models = append([]hub.ConnectionModel(nil), update.Models...)
	if update.ReplaceHeaders {
		update.Headers = make(map[string]string, len(v.headers))
		for _, header := range v.headers {
			if header.name == "" {
				v.problem = "请填写请求头名称，或移除空行。"
				return
			}
			if _, duplicate := update.Headers[header.name]; duplicate {
				v.problem = "请求头名称重复，请保留一行。"
				return
			}
			update.Headers[header.name] = header.value
		}
	}
	v.saving, v.problem = true, ""
	go func() {
		entry, err := p.Service.Connections.Save(p.ctx, update)
		p.dispatch(func() {
			v.saving = false
			if err != nil {
				v.problem = connectionOperationError(err)
				return
			}
			p.cancelConnectionDraft()
			v.selected = entry.ID
			p.observeConnections()
		})
	}()
}

func connectionOperationError(err error) string {
	if errors.Is(err, hub.ErrConnectionConflict) {
		return "连接已在其他窗口更改。草稿已保留；刷新后重新编辑。"
	}
	return "保存失败，草稿已保留。" + err.Error()
}

func (p *NativePage) changeConnectionEnabled(entry hub.ModelConnection) {
	v := &p.connections
	if v.saving || v.loading || v.readFailed {
		return
	}
	v.saving, v.problem = true, ""
	go func() {
		_, err := p.Service.Connections.SetEnabled(p.ctx, entry.ID, entry.Revision, !entry.Enabled)
		p.dispatch(func() {
			v.saving = false
			if err != nil {
				v.problem = connectionOperationError(err)
				return
			}
			p.observeConnections()
		})
	}()
}

func (p *NativePage) deleteConnection(entry hub.ModelConnection) {
	v := &p.connections
	if !v.deleteReview || v.saving || v.loading || v.readFailed {
		return
	}
	v.saving, v.problem = true, ""
	go func() {
		err := p.Service.Connections.Delete(p.ctx, entry.ID, entry.Revision)
		p.dispatch(func() {
			v.saving = false
			if err != nil {
				v.problem = connectionOperationError(err)
				return
			}
			v.selected, v.deleteReview = "", false
			p.observeConnections()
		})
	}()
}

func (p *NativePage) guardConnectionsClose(event *mygo.CloseEvent) bool {
	if !p.connections.saving {
		return false
	}
	event.PreventDefault()
	p.setPane("connections")
	p.connections.problem = "正在保存连接，请等待存储完成后关闭窗口。"
	return true
}

func connectionButton(c *ui.Context, key, text, icon string) ui.Element {
	t := c.Theme()
	b := ui.ButtonBase(c.Key(key)).Label(text).Height(36).Padding(8, 12).Gap(8).
		AlignItems(ui.Center).AlignSelf(ui.Start).Radius(8).TextColor(t.Text)
	if b.Hovered() {
		b.Background(t.SurfaceHover)
	}
	b.Children(func() {
		ui.Icon(c, workspaceIcon(icon)).Size(16, 16).Shrink(0)
		ui.Text(c, text).FontSize(14).SingleLine()
	})
	return b
}

func (p *NativePage) buildConnections(c *ui.Context) {
	v := &p.connections
	if p.Service.Connections == nil {
		ui.Text(c, "模型连接服务不可用").Role(ui.RoleStatus)
		return
	}
	ui.Row(c).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
		ui.Text(c, "模型与连接").FontSize(24).FontWeight(600).Grow(1)
		if v.mode == "" {
			connectionButton(c, "connections-reload", "刷新连接", "refresh-cw").Disabled(v.loading || v.saving).OnClick(p.reloadConnections)
			connectionButton(c, "connections-add", "添加连接", "plus").Disabled(v.loading || v.saving || v.readFailed).OnClick(func() { v.mode, v.deleteReview, v.problem = "sources", false, "" })
		}
	})
	if v.problem != "" {
		ui.Text(c, v.problem).Role(ui.RoleStatus).TextColor(c.Theme().Danger)
	}
	if v.loading || v.saving {
		status := "正在读取…"
		if v.saving {
			status = "正在保存…"
		}
		ui.Text(c, status).Role(ui.RoleStatus).TextColor(c.Theme().TextMuted)
	}
	switch v.mode {
	case "sources":
		p.buildConnectionSources(c)
	case "form":
		p.buildConnectionForm(c)
	default:
		p.buildConnectionManager(c)
	}
}

func (p *NativePage) buildConnectionSources(c *ui.Context) {
	v := &p.connections
	c.OnShortcut(0, ui.KeyEscape, p.cancelConnectionDraft)
	ui.Row(c).Children(func() {
		connectionButton(c, "connections-back-manager", "返回连接", "chevron-left").OnClick(p.cancelConnectionDraft)
	})
	ui.Column(c).FillWidth().MaxWidth(760).Gap(8).Children(func() {
		ui.Text(c, "选择供应商").FontSize(18).FontWeight(600)
		ui.TextInput(c.Key("connections-source-search"), &v.query).Label("Search connection sources").Placeholder("搜索供应商")
		for _, source := range []struct{ id, name, icon string }{{"OpenAI", "OpenAI", "cloud"}, {"Anthropic", "Anthropic", "cloud"}, {"Gemini", "Google Gemini", "cloud"}, {"Custom", "自定义端点", "plug"}, {"Local", "本地 HTTP 服务", "cpu"}} {
			if v.query != "" && !strings.Contains(strings.ToLower(source.name+source.id), strings.ToLower(v.query)) {
				continue
			}
			workspaceAction(c, "connection-source-"+source.id, source.name, source.icon, false).
				Label("Add " + source.id + " connection").OnClick(func() { p.chooseConnectionSource(source.id) })
		}
	})
}

func (p *NativePage) buildConnectionManager(c *ui.Context) {
	v := &p.connections
	ui.Row(c).Fill().Gap(24).Children(func() {
		ui.Scroll(c.Key("connections-list")).Width(224).FillHeight().Shrink(0).Children(func() {
			ui.Column(c).FillWidth().Gap(4).Children(func() {
				for _, entry := range v.entries {
					workspaceAction(c, entry.ID, entry.Name, "plug", entry.ID == v.selected).Label("Connection " + entry.Name).
						Disabled(v.saving).OnClick(func() { v.selected, v.deleteReview, v.problem = entry.ID, false, "" })
				}
				if len(v.entries) == 0 && !v.loading {
					ui.Text(c, "尚未添加连接").TextColor(c.Theme().TextMuted)
				}
			})
		})
		ui.Column(c).Grow(1).MinWidth(0).FillHeight().Gap(12).Children(func() {
			entry, ok := p.selectedConnection()
			if !ok {
				return
			}
			ui.Row(c).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
				ui.Text(c, entry.Name).FontSize(18).FontWeight(600).Grow(1)
				connectionButton(c, "connection-edit", "连接设置", "pencil").Label("Edit connection").Disabled(v.saving || v.loading || v.readFailed).OnClick(func() { p.editConnection(entry) })
				workspaceIconAction(c, "connection-delete", "Delete connection", "trash-2").Disabled(v.saving || v.loading || v.readFailed).OnClick(func() { v.deleteReview = true })
			})
			ui.Text(c, connectionProtocolLabel(entry.Protocol)).TextColor(c.Theme().TextMuted)
			ui.Row(c).Gap(8).AlignItems(ui.Center).Children(func() {
				label, status := "启用连接", "已禁用"
				if entry.Enabled {
					label, status = "禁用连接", "已启用"
				}
				ui.Text(c, status).Grow(1)
				ui.Button(c, label).Label("Toggle connection enabled").Disabled(v.saving || v.loading || v.readFailed).OnClick(func() { p.changeConnectionEnabled(entry) })
			})
			if v.deleteReview {
				ui.Text(c, "删除连接将移除其模型和凭据，之后无法再选择；现有会话记录保留。").TextColor(c.Theme().Danger)
				ui.Row(c).Gap(8).Children(func() {
					ui.Button(c, "取消删除").Label("Cancel delete connection").Disabled(v.saving).OnClick(func() { v.deleteReview = false })
					ui.Button(c, "确认删除").Label("Confirm delete connection").Disabled(v.saving).OnClick(func() { p.deleteConnection(entry) })
				})
			}
			ui.Divider(c)
			ui.Text(c, "模型").FontWeight(600)
			ui.Scroll(c.Key("connection-model-list")).Grow(1).MinHeight(0).Children(func() {
				ui.Column(c).FillWidth().Gap(8).Children(func() {
					if len(entry.Models) == 0 {
						ui.Text(c, "尚未添加模型").TextColor(c.Theme().TextMuted)
					}
					for _, model := range entry.Models {
						ui.Row(c.Key(model.ID)).FillWidth().Gap(12).Padding(8, 0).Children(func() {
							name := model.Name
							if name == "" {
								name = model.ID
							}
							ui.Column(c).Grow(1).MinWidth(0).Children(func() {
								ui.Text(c, name)
								if name != model.ID {
									ui.Text(c, model.ID).TextColor(c.Theme().TextMuted).FontSize(12)
								}
							})
							state := "未启用"
							if model.Enabled {
								state = "已启用"
							}
							ui.Text(c, state).TextColor(c.Theme().TextMuted)
						})
					}
				})
			})
		})
	})
}

func connectionTextField(c *ui.Context, key, title string, value *string, disabled bool) ui.Element {
	ui.Text(c, title).FontSize(12).TextColor(c.Theme().TextMuted)
	return ui.TextInput(c.Key(key), value).Label("Edit " + title).FillWidth().Height(36).Disabled(disabled)
}

var connectionProtocols = []struct{ id, label string }{
	{"openai_responses", "OpenAI Responses"}, {"openai_chat", "OpenAI Chat Completions"},
	{"anthropic", "Anthropic Messages"}, {"gemini", "Gemini"},
}

func connectionProtocolLabel(id string) string {
	for _, protocol := range connectionProtocols {
		if protocol.id == id {
			return protocol.label
		}
	}
	return id
}

func (p *NativePage) buildConnectionForm(c *ui.Context) {
	v := &p.connections
	busy := v.saving || v.loading
	back := func() {
		if busy {
			return
		}
		if v.draft.ID != "" {
			p.cancelConnectionDraft()
		} else {
			v.mode = "sources"
		}
	}
	ui.Row(c).FillWidth().MaxWidth(760).Gap(8).Children(func() {
		backLabel := "返回来源"
		if v.draft.ID != "" {
			backLabel = "返回连接"
		}
		connectionButton(c, "connection-form-back", backLabel, "chevron-left").Label("Back from connection form").Disabled(busy).OnClick(back)
		ui.Box(c).Grow(1)
		if v.draft.ID != "" {
			connectionButton(c, "connection-form-reload", "重新载入", "refresh-cw").Label("Reload connection draft").Disabled(busy).OnClick(p.reloadConnections)
		}
	})
	ui.Scroll(c.Key("connection-form-scroll")).Grow(1).MinHeight(0).Children(func() {
		ui.Column(c).FillWidth().MaxWidth(760).Gap(8).Padding(0, 0, 16).Children(func() {
			title := v.source
			if title == "Custom" {
				title = "自定义连接"
			}
			if title == "Local" {
				title = "本地 HTTP 服务"
			}
			if v.draft.ID != "" {
				title = v.draft.Name
			}
			ui.Text(c, title).FontSize(18).FontWeight(600)
			custom := v.source == "Custom" || v.source == "Local"
			if !custom {
				connectionButton(c, "connection-details", "连接设置", "chevron-down").Label("Connection details").Disabled(busy).OnClick(func() { v.details = !v.details })
			}
			if custom || v.details {
				connectionTextField(c, "connection-name", "连接名称", &v.draft.Name, busy).Ref(&v.focus)
				ui.Row(c).FillWidth().Gap(16).Children(func() {
					ui.Column(c).Grow(1).MinWidth(0).Gap(4).Children(func() {
						ui.Text(c, "API 协议").FontSize(12).TextColor(c.Theme().TextMuted)
						label := connectionProtocolLabel(v.draft.Protocol)
						labels := make([]string, len(connectionProtocols))
						for i, protocol := range connectionProtocols {
							labels[i] = protocol.label
						}
						ui.Select(c.Key("connection-protocol"), &label, labels).Label("Connection protocol").FillWidth().Height(36).Disabled(busy).OnChange(func() {
							for _, protocol := range connectionProtocols {
								if protocol.label == label {
									v.draft.Protocol = protocol.id
								}
							}
						})
					})
					ui.Column(c).Width(180).Gap(4).Children(func() {
						ui.Text(c, "认证方式").FontSize(12).TextColor(c.Theme().TextMuted)
						label := "API Key"
						if v.draft.Auth == "none" {
							label = "无认证"
						}
						ui.Select(c.Key("connection-auth"), &label, []string{"API Key", "无认证"}).Label("Connection authentication").FillWidth().Height(36).Disabled(busy).OnChange(func() {
							v.draft.Auth = "api_key"
							if label == "无认证" {
								v.draft.Auth = "none"
							}
						})
					})
				})
				connectionTextField(c, "connection-endpoint", "完整请求地址", &v.draft.Endpoint, busy)
			}
			if v.draft.Auth == "api_key" {
				key := connectionTextField(c, "connection-key", "API Key", &v.draft.APIKey, busy || v.draft.ClearAPIKey).Password()
				if !custom && !v.details {
					key.Ref(&v.focus)
				}
				if entry, ok := p.selectedConnection(); ok && v.draft.ID != "" && entry.HasAPIKey {
					key.Placeholder("已保存；留空保留")
					ui.Checkbox(c, &v.draft.ClearAPIKey, "清除已保存的 Key").Label("Clear saved API Key").Disabled(busy).OnChange(func() {
						if v.draft.ClearAPIKey {
							v.draft.APIKey = ""
						}
					})
				}
			}
			connectionButton(c, "connection-advanced", "请求头", "chevron-down").Label("Advanced connection headers").Disabled(busy).OnClick(func() { v.advanced = !v.advanced })
			if v.advanced {
				p.buildConnectionHeaders(c, busy)
			}
			ui.Divider(c)
			ui.Row(c).FillWidth().AlignItems(ui.Center).Children(func() {
				ui.Text(c, "模型（可选）").FontWeight(600).Grow(1)
				ui.Text(c, fmt.Sprintf("%d 个", len(v.draft.Models))).TextColor(c.Theme().TextMuted).FontSize(12)
			})
			ui.Row(c).FillWidth().Gap(8).AlignItems(ui.End).Children(func() {
				ui.Column(c).Grow(1).MinWidth(0).Gap(4).Children(func() {
					connectionTextField(c, "connection-model-id", "模型 ID", &v.modelID, busy).OnTextKey(func(k ui.TextKey) bool {
						if k.Mods != 0 || k.Key != ui.KeyEnter {
							return false
						}
						v.modelID = k.Text()
						p.addConnectionModel()
						k.SetText(v.modelID)
						return true
					})
				})
				ui.Column(c).Grow(1).MinWidth(0).Gap(4).Children(func() { connectionTextField(c, "connection-model-name", "显示名称", &v.modelName, busy) })
				canAdd := strings.TrimSpace(v.modelID) != ""
				for _, model := range v.draft.Models {
					if model.ID == v.modelID {
						canAdd = false
					}
				}
				ui.Button(c, "添加").Label("Add model to connection draft").Height(36).Disabled(busy || !canAdd).OnClick(p.addConnectionModel)
			})
			for i := range v.draft.Models {
				model := &v.draft.Models[i]
				ui.Row(c.Key(model.ID)).FillWidth().Gap(12).Padding(8, 0).AlignItems(ui.Center).Children(func() {
					ui.Checkbox(c, &model.Enabled, model.ID).Label("Enable model " + model.ID).Disabled(busy).Grow(1).MinWidth(0)
					ui.Checkbox(c, &model.ImageCapable, "声明图像输入").Label("Image input declaration " + model.ID).Disabled(busy)
					workspaceIconAction(c, "remove-model", "Remove model "+model.ID, "trash-2").Disabled(busy).OnClick(func() { v.draft.Models = append(v.draft.Models[:i], v.draft.Models[i+1:]...) })
				})
			}
			ui.Checkbox(c, &v.draft.Enabled, "启用此连接").Label("Enable connection draft").Disabled(busy)
		})
	})
	ui.Row(c).FillWidth().MaxWidth(760).Gap(8).Children(func() {
		ui.Button(c, "取消").Label("Cancel connection draft").Height(36).Disabled(busy).OnClick(p.cancelConnectionDraft)
		ui.Box(c).Grow(1)
		ui.Button(c, "保存连接").Label("Save connection").Height(36).Disabled(busy || v.readFailed).OnClick(p.saveConnectionDraft)
	})
}

func (p *NativePage) buildConnectionHeaders(c *ui.Context, busy bool) {
	v := &p.connections
	if entry, ok := p.selectedConnection(); ok && v.draft.ID != "" && len(entry.HeaderNames) > 0 {
		ui.Text(c, "已保存："+strings.Join(entry.HeaderNames, ", ")).TextColor(c.Theme().TextMuted).FontSize(12)
	}
	ui.Checkbox(c, &v.draft.ReplaceHeaders, "替换请求头").Label("Replace saved headers").Disabled(busy)
	if !v.draft.ReplaceHeaders {
		return
	}
	for i := range v.headers {
		header := &v.headers[i]
		ui.Row(c.Key(i)).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
			ui.TextInput(c.Key("name"), &header.name).Label(fmt.Sprintf("Header name %d", i+1)).Placeholder("名称").Grow(1).MinWidth(0).Height(36).Disabled(busy)
			ui.TextInput(c.Key("value"), &header.value).Label(fmt.Sprintf("Header value %d", i+1)).Placeholder("值").Password().Grow(1).MinWidth(0).Height(36).Disabled(busy)
			workspaceIconAction(c, "remove", fmt.Sprintf("Remove header %d", i+1), "trash-2").Disabled(busy).OnClick(func() { v.headers = append(v.headers[:i], v.headers[i+1:]...) })
		})
	}
	connectionButton(c, "connection-header-add", "添加请求头", "plus").Label("Add header").Disabled(busy).OnClick(func() { v.headers = append(v.headers, connectionHeaderDraft{}) })
}
