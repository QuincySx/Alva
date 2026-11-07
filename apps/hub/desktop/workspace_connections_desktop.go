// INPUT: Shared connection service, redacted committed DTOs and native MyGo controls.
// OUTPUT: Request-bound create/recovery, explicit CRUD, masked drafts and cancellable catalog/manual model selection.
// POS: [L7 Hub UI] Owns page-local drafts and navigation; product service owns policy, secrets and routes.
//go:build desktop

package hubdesktop

import (
	"context"
	"errors"
	"fmt"
	"net/url"
	"reflect"
	"strings"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

type connectionHeaderDraft struct{ name, value string }

type modelConnectionsPane struct {
	entries                       []hub.ModelConnection
	selected, mode, source, query string
	draft                         hub.ConnectionUpdate
	// autoCatalog is the catalog URL the form filled in itself (preset or
	// derived from the endpoint). While the draft still holds it, editing the
	// endpoint re-derives it; a value the user typed is never replaced.
	autoCatalog                     string
	headers                         []connectionHeaderDraft
	modelID, modelName              string
	details, advanced, deleteReview bool
	deleteID                        string
	deleteRevision                  uint64
	draftMissing                    bool
	draftStale                      bool
	creationRequest                 string
	creationAttempt                 *hub.ConnectionUpdate
	revealAPIKey                    bool
	loading, saving, readFailed     bool
	problem                         string
	focus                           ui.Handle
	catalogState, testState         string
	catalogProblem, testProblem     string
	catalogQuery                    string
	catalogAction                   string
	catalogModels                   []hub.ConnectionModel
	catalogPage                     int
	requestID                       uint64
	requestCancel                   context.CancelFunc
	requestBusy                     bool
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
		if v.deleteReview {
			reviewed := false
			for _, entry := range v.entries {
				reviewed = reviewed || entry.ID == v.deleteID && entry.Revision == v.deleteRevision
			}
			if !reviewed {
				v.deleteReview = false
				v.problem = "连接已更改或删除，请重新审阅后再删除。"
			}
		}
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
				v.draftMissing = true
				for _, entry := range entries {
					if entry.ID == v.draft.ID {
						p.editConnection(entry)
						break
					}
				}
				if v.draftMissing {
					p.resetConnectionProbe()
					v.problem = "此连接已被删除。草稿已保留，请返回模型与连接查看现有连接。"
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

func (p *NativePage) editingConnection() (hub.ModelConnection, bool) {
	for _, entry := range p.connections.entries {
		if entry.ID == p.connections.draft.ID {
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
	v.revealAPIKey = false
	// Returning through the source chooser keeps the same source's draft.
	if v.source != source {
		p.resetConnectionProbe()
		v.draft = hub.ConnectionUpdate{Auth: "api_key"}
		v.headers, v.modelID, v.modelName = nil, "", ""
		v.details, v.advanced = false, false
		v.draftMissing = false
		v.draftStale, v.creationRequest, v.creationAttempt = false, "", nil
		switch source {
		case "OpenAI":
			v.draft.Name, v.draft.Protocol, v.draft.Endpoint = "OpenAI", "openai_responses", "https://api.openai.com/v1/responses"
			v.draft.CatalogURL = "https://api.openai.com/v1/models"
		case "Anthropic":
			v.draft.Name, v.draft.Protocol, v.draft.Endpoint = "Anthropic", "anthropic", "https://api.anthropic.com/v1/messages"
			v.draft.CatalogURL = "https://api.anthropic.com/v1/models"
		case "Gemini":
			v.draft.Name, v.draft.Protocol, v.draft.Endpoint = "Google Gemini", "gemini", "https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent"
			v.draft.CatalogURL = "https://generativelanguage.googleapis.com/v1beta/models"
		case "Local":
			v.draft.Name, v.draft.Protocol, v.draft.Auth = "本地模型服务", "openai_chat", "none"
		default:
			v.draft.Protocol = "openai_responses"
		}
		v.autoCatalog = v.draft.CatalogURL
	}
	v.source, v.mode, v.problem = source, "form", ""
	v.focus.Focus()
}

func (p *NativePage) editConnection(entry hub.ModelConnection) {
	v := &p.connections
	v.revealAPIKey = false
	p.resetConnectionProbe()
	v.selected, v.source, v.mode = entry.ID, "Custom", "form"
	v.draft = hub.ConnectionUpdate{ID: entry.ID, ExpectedRevision: entry.Revision, Name: entry.Name,
		Protocol: entry.Protocol, Endpoint: entry.Endpoint, CatalogURL: entry.CatalogURL, Auth: entry.Auth, Enabled: entry.Enabled,
		Models: append([]hub.ConnectionModel(nil), entry.Models...)}
	v.autoCatalog = ""
	if entry.CatalogURL == "" || entry.CatalogURL == hub.DefaultCatalogURL(entry.Endpoint) {
		v.autoCatalog = entry.CatalogURL
	}
	v.headers, v.modelID, v.modelName = nil, "", ""
	v.details, v.advanced, v.deleteReview, v.problem = true, false, false, ""
	v.draftMissing = false
	v.draftStale, v.creationRequest, v.creationAttempt = false, "", nil
	v.focus.Focus()
}

func (p *NativePage) cancelConnectionDraft() {
	v := &p.connections
	if v.saving {
		return
	}
	p.resetConnectionProbe()
	v.mode, v.source, v.problem = "", "", ""
	v.draftMissing = false
	v.draftStale, v.creationRequest, v.creationAttempt = false, "", nil
	v.revealAPIKey = false
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
	if p.Service.Connections == nil || v.saving || v.loading || v.readFailed || v.requestBusy || v.draftMissing || v.draftStale {
		return
	}
	v.revealAPIKey = false
	update, valid := p.connectionDraftSnapshot()
	if !valid {
		return
	}
	requestID := ""
	if update.ID == "" {
		if v.creationRequest == "" {
			id, err := hub.NewID("connection-request-")
			if err != nil {
				v.problem = "无法生成连接创建请求身份，请重试。"
				return
			}
			v.creationRequest = id
		}
		requestID = v.creationRequest
		if err := (hub.ConnectionCreate{RequestID: requestID, Update: update}).Validate(); err != nil {
			v.problem = connectionOperationError(err)
			return
		}
		if v.creationAttempt == nil {
			captured := update
			v.creationAttempt = &captured
		}
	}
	v.saving, v.problem = true, ""
	go func() {
		var entry hub.ModelConnection
		var err error
		if requestID != "" {
			entry, err = p.Service.Connections.Create(p.ctx, hub.ConnectionCreate{RequestID: requestID, Update: update})
		} else {
			entry, err = p.Service.Connections.Save(p.ctx, update)
		}
		p.dispatch(func() {
			v.saving = false
			if err != nil {
				v.problem = connectionOperationError(err)
				if requestID != "" && !errors.Is(err, hub.ErrConnectionCreationConflict) {
					v.problem = "创建结果尚未确认。草稿已保留，可重试同一次保存或恢复保存结果。" + err.Error()
				}
				return
			}
			p.cancelConnectionDraft()
			v.selected = entry.ID
			p.observeConnections()
		})
	}()
}

func (p *NativePage) recoverConnectionCreation() {
	v := &p.connections
	if p.Service.Connections == nil || v.creationRequest == "" || v.saving || v.loading || v.requestBusy {
		return
	}
	v.loading, v.revealAPIKey = true, false
	requestID := v.creationRequest
	go func() {
		receipt, err := p.Service.Connections.ResolveCreation(p.ctx, requestID)
		p.dispatch(func() {
			v.loading = false
			if v.creationRequest != requestID {
				return
			}
			if err != nil {
				if errors.Is(err, hub.ErrNotFound) {
					v.creationAttempt = nil
					v.problem = "本次创建尚未保存。草稿已保留，可修改后明确重试。"
				} else {
					v.problem = "无法恢复保存结果。草稿与请求身份已保留，请重试恢复。"
				}
				return
			}
			update, valid := p.connectionDraftSnapshot()
			unchanged := valid && v.creationAttempt != nil && reflect.DeepEqual(update, *v.creationAttempt)
			p.resetConnectionProbe()
			v.selected, v.draft.ID, v.draft.ExpectedRevision = receipt.Created.ID, receipt.Created.ID, receipt.Created.Revision
			if receipt.Current == nil {
				v.draftMissing = true
				v.problem = "本次连接已保存后被删除。草稿已保留，不会重新创建；请返回模型与连接查看。"
				return
			}
			if receipt.Current.Revision != receipt.Created.Revision {
				v.draftStale = true
				v.problem = "本次连接已保存后在其他窗口更改。草稿已保留，请明确重新载入后再编辑。"
				return
			}
			if unchanged {
				p.cancelConnectionDraft()
				v.selected = receipt.Created.ID
				p.observeConnections()
				return
			}
			v.creationRequest, v.creationAttempt = "", nil
			v.problem = "已找到本次保存的连接。后续修改仍在草稿中，可明确保存到此连接。"
		})
	}()
}

func (p *NativePage) connectionDraftSnapshot() (hub.ConnectionUpdate, bool) {
	v := &p.connections
	update := v.draft
	update.Models = append([]hub.ConnectionModel(nil), update.Models...)
	if update.ID == "" && update.Auth == "none" {
		update.APIKey = ""
	}
	if update.ReplaceHeaders {
		update.Headers = make(map[string]string, len(v.headers))
		for _, header := range v.headers {
			if header.name == "" {
				v.problem = "请填写请求头名称，或移除空行。"
				return hub.ConnectionUpdate{}, false
			}
			if _, duplicate := update.Headers[header.name]; duplicate {
				v.problem = "请求头名称重复，请保留一行。"
				return hub.ConnectionUpdate{}, false
			}
			update.Headers[header.name] = header.value
		}
	}
	return update, true
}

func connectionOperationError(err error) string {
	if errors.Is(err, hub.ErrConnectionCreationConflict) {
		return "本次创建请求已保存或连接已更改。草稿已保留，请先恢复保存结果。"
	}
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
	if entry.ID != v.deleteID || entry.Revision != v.deleteRevision {
		v.deleteReview = false
		v.problem = "连接已更改或删除，请重新审阅后再删除。"
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
	if p.connections.requestBusy && !p.connections.saving {
		event.PreventDefault()
		p.cancelConnectionProbe()
		p.setPane("connections")
		p.connections.problem = "正在取消连接请求，请等待真实清理完成后关闭窗口。"
		return true
	}
	if !p.connections.saving {
		return false
	}
	event.PreventDefault()
	p.setPane("connections")
	p.connections.problem = "正在保存连接，请等待存储完成后关闭窗口。"
	return true
}

func connectionButton(c *ui.Context, key, text, icon string, disabled ...bool) ui.Element {
	return workspaceControlIconButton(c, key, text, icon, "ghost", disabled...)
}

func (p *NativePage) buildConnections(c *ui.Context) {
	v := &p.connections
	if p.Service.Connections == nil {
		ui.Text(c, "模型连接服务不可用").Role(ui.RoleStatus)
		return
	}
	if v.mode == "" {
		ui.Row(c).FillWidth().Gap(8).AlignItems(ui.Start).Children(func() {
			ui.Column(c).Grow(1).MinWidth(0).Gap(8).Children(func() {
				ui.Text(c, "模型与连接").FontSize(workspaceFontHeading).FontWeight(600).LineHeight(1.5)
			})
			connectionButton(c, "connections-reload", "刷新连接", "refresh-cw", v.loading || v.saving).OnClick(p.reloadConnections)
			workspaceControlIconButton(c, "connections-add", "添加供应商", "plus", "primary", v.loading || v.saving || v.readFailed).Label("添加连接").OnClick(func() { v.mode, v.deleteReview, v.problem = "sources", false, "" })
		})
	}
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
	// Hug the normal card so the sheet body keeps it centered; when pending
	// panels need space, shrink only this viewport and scroll the intact card.
	ui.Scroll(c.Key("connection-sources-viewport")).Label("Connection sources viewport").FillWidth().Shrink(1).MinHeight(0).AlignItems(ui.Center).Children(func() {
		ui.Column(c).Label("Connection source dialog").FillWidth().MaxWidth(720).Shrink(0).Padding(24).Gap(16).
			Radius(16).Background(c.Theme().Surface).DrawOver(func(painter *ui.Painter, r ui.Rect) { painter.Stroke(r, c.Theme().Border, 16, 1) }).Children(func() {
			ui.Row(c).FillWidth().Padding(0, 0, 16).AlignItems(ui.Start).Children(func() {
				ui.Text(c, "添加供应商").FontSize(workspaceFontHeading).FontWeight(600).LineHeight(1.5).Grow(1)
				workspaceIconAction(c, "connection-sources-close", "Cancel connection sources", "x").OnClick(p.cancelConnectionDraft)
			})
			workspaceControlInput(c, "connections-source-search", &v.query).Label("Search connection sources").Placeholder("搜索服务…").FillWidth()
			for _, group := range []string{"API 服务", "其他来源"} {
				ui.Column(c).FillWidth().Gap(4).Children(func() {
					ui.Text(c, group).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(c.Theme().TextMuted)
					for _, source := range []struct{ id, name, icon, description, group string }{
						{"OpenAI", "OpenAI", "cpu", "", "API 服务"}, {"Anthropic", "Anthropic", "cpu", "", "API 服务"},
						{"Gemini", "Google Gemini", "sparkles", "", "API 服务"},
						{"Custom", "自定义连接", "globe", "兼容 API 或自有服务", "其他来源"},
						{"Local", "本地服务", "terminal", "连接已经启动的 HTTP 服务", "其他来源"},
					} {
						if source.group != group || v.query != "" && !strings.Contains(strings.ToLower(source.name+source.id), strings.ToLower(v.query)) {
							continue
						}
						connectionSourceRow(c, source.id, source.name, source.description, source.icon).
							Label("Add " + source.id + " connection").OnClick(func() { p.chooseConnectionSource(source.id) })
					}
				})
			}
		})
	})
}

func connectionSourceRow(c *ui.Context, key, title, description, icon string) ui.Element {
	t := c.Theme()
	b := ui.ButtonBase(c.Key("connection-source-"+key)).FillWidth().MinWidth(0).MinHeight(workspaceControlHeight).Padding(4, 8).Gap(8).
		AlignItems(ui.Center).Justify(ui.Start).Radius(8).TextColor(t.Text)
	if b.Hovered() {
		b.Background(t.SurfaceHover)
	}
	b.Children(func() {
		ui.Icon(c, workspaceIcon(icon)).Size(workspaceIconSize, workspaceIconSize).Shrink(0).TextColor(t.TextMuted)
		ui.Column(c).Grow(1).MinWidth(0).Gap(4).Children(func() {
			ui.Text(c, title).FontSize(workspaceFontBody).LineHeight(1.5).SingleLine()
			if description != "" {
				ui.Text(c, description).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(t.TextMuted)
			}
		})
	})
	return b
}

func (p *NativePage) buildConnectionManager(c *ui.Context) {
	v := &p.connections
	ui.Row(c).Label("Connection manager").Fill().Gap(12).Children(func() {
		ui.Column(c).Label("Connection list").Width(208).FillHeight().Shrink(0).Padding(12).Gap(8).Background(c.Theme().Surface).Children(func() {
			ui.Text(c, "我的连接").FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(c.Theme().TextMuted)
			ui.Scroll(c.Key("connections-list")).Grow(1).MinHeight(0).FillWidth().Children(func() {
				ui.Column(c).FillWidth().Gap(8).Children(func() {
					for _, entry := range v.entries {
						connectionListRow(c, entry, entry.ID == v.selected).Disabled(v.saving).
							OnClick(func() { v.selected, v.deleteReview, v.problem = entry.ID, false, "" })
					}
					if len(v.entries) == 0 && !v.loading {
						ui.Text(c, "尚未添加连接").TextColor(c.Theme().TextMuted)
					}
				})
			})
			workspaceControlIconButton(c, "connection-list-add", "添加供应商", "plus", "secondary", v.loading || v.saving || v.readFailed).FillWidth().OnClick(func() { v.mode, v.deleteReview, v.problem = "sources", false, "" })
		})
		ui.Column(c).Label("Connection models").Grow(1).MinWidth(0).FillHeight().Padding(16).Gap(16).Background(c.Theme().Surface).Children(func() {
			entry, ok := p.selectedConnection()
			if !ok {
				return
			}
			ui.Row(c).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
				ui.Column(c).Grow(1).MinWidth(0).Gap(8).Children(func() {
					ui.Text(c, entry.Name).FontSize(workspaceFontSubtitle).FontWeight(600).LineHeight(1.5)
					ui.Text(c, connectionProtocolLabel(entry.Protocol)+" · ALVA").FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(c.Theme().TextMuted)
				})
				workspaceControlButton(c, "connection-edit", "连接设置", "secondary", v.saving || v.loading || v.readFailed).Label("Edit connection").OnClick(func() { p.editConnection(entry) })
				workspaceIconAction(c, "connection-delete", "Delete connection", "trash-2").Disabled(v.saving || v.loading || v.readFailed).OnClick(func() {
					v.deleteReview, v.deleteID, v.deleteRevision = true, entry.ID, entry.Revision
				})
			})
			ui.Row(c).Gap(8).AlignItems(ui.Center).Children(func() {
				label, status := "启用连接", "已禁用"
				if entry.Enabled {
					label, status = "禁用连接", "已启用"
				}
				ui.Text(c, status).Grow(1)
				workspaceControlButton(c, "connection-toggle", label, "secondary", v.saving || v.loading || v.readFailed).Label("Toggle connection enabled").OnClick(func() { p.changeConnectionEnabled(entry) })
			})
			if v.deleteReview {
				ui.Text(c, "删除连接将移除其模型和凭据，之后无法再选择；现有会话记录保留。").TextColor(c.Theme().Danger)
				ui.Row(c).Gap(8).Children(func() {
					workspaceControlButton(c, "connection-delete-cancel", "取消删除", "secondary", v.saving).Label("Cancel delete connection").OnClick(func() { v.deleteReview = false })
					workspaceControlButton(c, "connection-delete-confirm", "确认删除", "danger", v.saving).Label("Confirm delete connection").OnClick(func() { p.deleteConnection(entry) })
				})
			}
			ui.Row(c).FillWidth().AlignItems(ui.Center).Gap(8).Children(func() {
				ui.Text(c, "模型").FontSize(workspaceFontSubtitle).FontWeight(600).LineHeight(1.5).Grow(1)
				workspaceControlIconButton(c, "connection-add-model", "添加模型", "plus", "secondary", v.saving || v.loading || v.readFailed).Label("Edit connection models").OnClick(func() { p.editConnection(entry) })
			})
			ui.Scroll(c.Key("connection-model-list")).Grow(1).MinHeight(0).Children(func() {
				ui.Column(c).FillWidth().Gap(8).Children(func() {
					if len(entry.Models) == 0 {
						ui.Text(c, "尚未添加模型").TextColor(c.Theme().TextMuted)
					}
					for _, model := range entry.Models {
						ui.Row(c.Key(model.ID)).FillWidth().MinHeight(56).Gap(8).Padding(8, 12).AlignItems(ui.Center).Children(func() {
							ui.Icon(c, workspaceIcon("cpu")).Size(workspaceIconSize, workspaceIconSize).Shrink(0).TextColor(c.Theme().TextMuted)
							name := model.Name
							if name == "" {
								name = model.ID
							}
							ui.Column(c).Grow(1).MinWidth(0).Gap(4).Children(func() {
								ui.Text(c, name).FontSize(workspaceFontBody).FontWeight(600).LineHeight(1.5).SingleLine()
								ui.Text(c, model.ID).TextColor(c.Theme().TextMuted).FontSize(workspaceFontCaption).LineHeight(1.5).SingleLine()
							})
							state := "未启用"
							if model.Enabled {
								state = "已启用"
							}
							ui.Text(c, state).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(c.Theme().TextMuted)
						})
					}
				})
			})
		})
	})
}

func connectionListRow(c *ui.Context, entry hub.ModelConnection, selected bool) ui.Element {
	t := c.Theme()
	b := ui.ButtonBase(c.Key(entry.ID)).Label("Connection "+entry.Name).FillWidth().MinWidth(0).
		Padding(8, 12).Gap(8).Radius(8).AlignItems(ui.Center).Justify(ui.Start).Selected(selected)
	if selected {
		b.Background(workspacePalette(c).subtle)
	} else if b.Hovered() {
		b.Background(t.SurfaceHover)
	}
	b.Children(func() {
		ui.Column(c).Grow(1).MinWidth(0).Gap(4).Children(func() {
			ui.Text(c, entry.Name).FontSize(workspaceFontBody).FontWeight(600).LineHeight(1.5).SingleLine().TextColor(t.Text)
			auth := "API Key"
			if entry.Auth == "none" {
				auth = "无认证"
			}
			ui.Text(c, auth).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(t.TextMuted).SingleLine()
		})
		if entry.Enabled {
			ui.Icon(c, workspaceIcon("check")).Size(16, 16).Shrink(0).TextColor(t.Accent)
		}
	})
	return b
}

func connectionTextField(c *ui.Context, key, title string, value *string, disabled bool) ui.Element {
	var input ui.Element
	ui.Column(c.Key(key + "-field")).FillWidth().Gap(8).Children(func() {
		ui.Text(c, title).FontSize(workspaceFontBody).FontWeight(600).LineHeight(1.5)
		input = workspaceControlInput(c, key, value).Label("Edit " + title).FillWidth().Disabled(disabled)
	})
	return input
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

func (p *NativePage) backConnectionForm() {
	v := &p.connections
	if v.saving || v.loading {
		return
	}
	v.revealAPIKey = false
	if v.draft.ID != "" {
		p.cancelConnectionDraft()
	} else {
		p.resetConnectionProbe()
		v.mode = "sources"
	}
}

func (p *NativePage) buildConnectionForm(c *ui.Context) {
	v := &p.connections
	busy := v.saving || v.loading
	custom := v.source == "Custom" || v.source == "Local"
	_, windowHeight := c.Size()
	bodyHeight := max(float32(0), windowHeight-workspaceTopInset(c)-workspaceHeaderHeight-workspaceHeaderBottomInset(c)-workspaceWindowInset-48)
	dialogHeight := float32(553)
	if custom || v.details || v.advanced || v.catalogState == "ready" || len(v.draft.Models) > 0 {
		dialogHeight = bodyHeight
	}
	// Pending interaction panels keep their own space. This viewport receives
	// only the remainder and can scroll the whole dialog when its fixed chrome
	// cannot fit; the normal dialog height and its detail scroller stay intact.
	ui.Scroll(c.Key("connection-form-viewport")).Label("Connection form viewport").FillWidth().Grow(1).MinHeight(0).
		MaxHeight(min(dialogHeight, bodyHeight)).AlignItems(ui.Center).Children(func() {
		ui.Column(c).Label("Connection form dialog").FillWidth().MaxWidth(800).Height(min(dialogHeight, bodyHeight)).Shrink(0).MinHeight(0).
			Padding(24).Gap(16).Radius(16).Background(c.Theme().Surface).DrawOver(func(painter *ui.Painter, r ui.Rect) { painter.Stroke(r, c.Theme().Border, 16, 1) }).Children(func() {
			if v.creationRequest != "" && v.creationAttempt != nil || v.draft.ID != "" {
				ui.Row(c).FillWidth().Gap(8).Children(func() {
					ui.Box(c).Grow(1)
					if v.creationRequest != "" && v.creationAttempt != nil {
						connectionButton(c, "connection-form-recover", "恢复保存结果", "refresh-cw", busy || v.requestBusy).Label("Recover connection creation").OnClick(p.recoverConnectionCreation)
					}
					if v.draft.ID != "" {
						connectionButton(c, "connection-form-reload", "重新载入", "refresh-cw", busy).Label("Reload connection draft").OnClick(p.reloadConnections)
					}
				})
			}
			title := v.source
			if title == "Custom" {
				title = "自定义连接"
			}
			if title == "Local" {
				title = "本地服务"
			}
			if v.draft.ID != "" {
				title = v.draft.Name
			}
			ui.Row(c).FillWidth().Padding(0, 0, 16).AlignItems(ui.Start).Gap(8).Children(func() {
				ui.Column(c).Grow(1).MinWidth(0).Gap(8).Children(func() {
					ui.Text(c, title).FontSize(workspaceFontHeading).FontWeight(600).LineHeight(1.5)
					if !custom {
						if endpoint, err := url.Parse(v.draft.Endpoint); err == nil && endpoint.Host != "" {
							ui.Text(c, endpoint.Host).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(c.Theme().TextMuted)
						}
					}
				})
				workspaceIconAction(c, "connection-form-close", "Close connection draft", "x").Disabled(busy).OnClick(p.cancelConnectionDraft)
			})
			ui.Scroll(c.Key("connection-form-scroll")).Label("Connection form details").Grow(1).MinHeight(0).FillWidth().Children(func() {
				ui.Column(c).FillWidth().Gap(16).Children(func() {
					if !custom {
						connectionButton(c, "connection-details", "连接设置", "chevron-down", busy).Label("Connection details").OnClick(func() { v.details = !v.details })
					}
					if custom || v.details {
						ui.Row(c).FillWidth().Gap(16).Children(func() {
							ui.Column(c).Grow(1).MinWidth(0).Children(func() {
								connectionTextField(c, "connection-name", "连接名称", &v.draft.Name, busy).Ref(&v.focus).OnChange(p.resetConnectionProbe)
							})
							ui.Column(c).Grow(1).MinWidth(0).Gap(8).Children(func() {
								ui.Text(c, "认证方式").FontSize(workspaceFontBody).FontWeight(600).LineHeight(1.5)
								label := "API Key"
								if v.draft.Auth == "none" {
									label = "无认证"
								}
								workspaceControlSelect(c, "connection-auth", &label, []string{"API Key", "无认证"}).Label("Connection authentication").FillWidth().Disabled(busy).OnChange(func() {
									v.draft.Auth = "api_key"
									if label == "无认证" {
										v.draft.Auth = "none"
										v.revealAPIKey = false
									}
									p.resetConnectionProbe()
								})
							})
						})
						ui.Column(c).FillWidth().Gap(8).Children(func() {
							ui.Text(c, "API 协议").FontSize(workspaceFontBody).FontWeight(600).LineHeight(1.5)
							label := connectionProtocolLabel(v.draft.Protocol)
							labels := make([]string, len(connectionProtocols))
							for i, protocol := range connectionProtocols {
								labels[i] = protocol.label
							}
							workspaceControlSelect(c, "connection-protocol", &label, labels).Label("Connection protocol").FillWidth().Disabled(busy).OnChange(func() {
								for _, protocol := range connectionProtocols {
									if protocol.label == label {
										v.draft.Protocol = protocol.id
										p.resetConnectionProbe()
									}
								}
							})
						})
						connectionTextField(c, "connection-endpoint", "完整请求地址", &v.draft.Endpoint, busy).OnChange(func() {
							p.followConnectionCatalog()
							p.resetConnectionProbe()
						})
					}
					if v.draft.Auth == "api_key" {
						var key ui.Element
						ui.Column(c).FillWidth().Gap(8).Children(func() {
							ui.Text(c, "API Key").FontSize(workspaceFontBody).FontWeight(600).LineHeight(1.5)
							key = workspaceControlPassword(c, "connection-key", &v.draft.APIKey, &v.revealAPIKey, busy || v.draft.ClearAPIKey).
								Label("Edit API Key").OnChange(p.resetConnectionProbe)
						})
						if !custom && !v.details {
							key.Ref(&v.focus)
						}
						if entry, ok := p.editingConnection(); ok && entry.HasAPIKey {
							key.Placeholder("已保存；留空保留")
							workspaceControlCheckbox(c, "connection-clear-key", &v.draft.ClearAPIKey, "清除已保存的 Key").Label("Clear saved API Key").Disabled(busy).OnChange(func() {
								p.resetConnectionProbe()
								if v.draft.ClearAPIKey {
									v.draft.APIKey = ""
									v.revealAPIKey = false
								}
							})
						}
					}
					ui.Row(c).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
						connectionButton(c, "connection-advanced", "高级设置", "chevron-down", busy).Label("Advanced connection headers").OnClick(func() { v.advanced = !v.advanced })
						ui.Box(c).Grow(1)
						p.buildConnectionProbe(c, busy)
					})
					if v.advanced {
						connectionTextField(c, "connection-catalog-url", "模型目录地址（可选，同源）", &v.draft.CatalogURL, busy).OnChange(func() {
							v.autoCatalog = "" // Typed by the user: never re-derived unless cleared.
							p.resetConnectionProbe()
						})
						p.buildConnectionHeaders(c, busy)
					}
					p.buildConnectionProbeStatus(c)
					ui.Column(c).FillWidth().Gap(8).Children(func() {
						ui.Row(c).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
							ui.Text(c, "模型（可选）").FontSize(workspaceFontSubtitle).FontWeight(600).LineHeight(1.5)
							if len(v.draft.Models) > 0 {
								ui.Text(c, fmt.Sprintf("已选 %d", len(v.draft.Models))).TextColor(c.Theme().TextMuted).FontSize(workspaceFontCaption).LineHeight(1.5)
							}
							ui.Box(c).Grow(1)
							fetchBusy := v.requestBusy && v.catalogState == "loading"
							p.workspaceControlAction(c, "connection-catalog-fetch", v.catalogActionLabel(), "refresh-cw", "secondary", fetchBusy, func() { p.runConnectionProbe(false) }, busy || v.requestBusy && !fetchBusy).
								Label("Fetch connection models")
						})
						p.buildConnectionCatalog(c, busy)
						ui.Row(c).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
							workspaceControlInput(c, "connection-model-id", &v.modelID).Label("Edit 模型 ID").Placeholder("输入模型 ID").Grow(1).MinWidth(0).Disabled(busy).OnTextKey(func(k ui.TextKey) bool {
								if k.Mods != 0 || k.Key != ui.KeyEnter {
									return false
								}
								v.modelID = k.Text()
								p.addConnectionModel()
								k.SetText(v.modelID)
								return true
							})
							workspaceControlInput(c, "connection-model-name", &v.modelName).Label("Edit 显示名称").Placeholder("显示名称（可选）").Grow(1).MinWidth(0).Disabled(busy)
							canAdd := strings.TrimSpace(v.modelID) != ""
							for _, model := range v.draft.Models {
								if model.ID == v.modelID {
									canAdd = false
								}
							}
							workspaceControlButton(c, "connection-model-add", "添加", "secondary", busy || !canAdd).Label("Add model to connection draft").OnClick(p.addConnectionModel)
						})
						for i := range v.draft.Models {
							model := &v.draft.Models[i]
							ui.Row(c.Key(model.ID)).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
								workspaceControlCheckbox(c, "enabled", &model.Enabled, model.ID).Label("Enable model " + model.ID).Disabled(busy).Grow(1).MinWidth(0)
								workspaceControlCheckbox(c, "images", &model.ImageCapable, "声明图像输入").Label("Image input declaration " + model.ID).Disabled(busy)
								workspaceIconAction(c, "remove-model", "Remove model "+model.ID, "trash-2").Disabled(busy).OnClick(func() { v.draft.Models = append(v.draft.Models[:i], v.draft.Models[i+1:]...) })
							})
						}
					})
				})
			})
			ui.Row(c).Label("Connection form footer").FillWidth().Height(52).Padding(16, 0, 0).Gap(8).AlignItems(ui.Center).
				Draw(func(painter *ui.Painter, r ui.Rect) { painter.Line(r.X, r.Y+.5, r.X+r.W, r.Y+.5, 1, c.Theme().Border) }).Children(func() {
				workspaceControlCheckbox(c, "connection-enabled", &v.draft.Enabled, "启用此连接").Label("Enable connection draft").Disabled(busy)
				ui.Box(c).Grow(1)
				workspaceControlButton(c, "connection-cancel", "取消", "secondary", busy).Label("Cancel connection draft").OnClick(p.cancelConnectionDraft)
				p.workspaceControlAction(c, "connection-save", "保存连接", "", "primary", v.saving, p.saveConnectionDraft, v.loading || v.readFailed || v.requestBusy || v.draftMissing || v.draftStale).
					Label("Save connection")
			})
		})
	})
}

func (p *NativePage) buildConnectionHeaders(c *ui.Context, busy bool) {
	v := &p.connections
	if entry, ok := p.editingConnection(); ok && len(entry.HeaderNames) > 0 {
		ui.Text(c, "已保存："+strings.Join(entry.HeaderNames, ", ")).TextColor(c.Theme().TextMuted).FontSize(workspaceFontCaption)
	}
	workspaceControlCheckbox(c, "connection-replace-headers", &v.draft.ReplaceHeaders, "替换请求头").Label("Replace saved headers").Disabled(busy).OnChange(p.resetConnectionProbe)
	if !v.draft.ReplaceHeaders {
		return
	}
	for i := range v.headers {
		header := &v.headers[i]
		ui.Row(c.Key(i)).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
			workspaceControlInput(c, "name", &header.name).Label(fmt.Sprintf("Header name %d", i+1)).Placeholder("名称").Grow(1).MinWidth(0).Disabled(busy).OnChange(p.resetConnectionProbe)
			workspaceControlInput(c, "value", &header.value).Label(fmt.Sprintf("Header value %d", i+1)).Placeholder("值").Password().Grow(1).MinWidth(0).Disabled(busy).OnChange(p.resetConnectionProbe)
			workspaceIconAction(c, "remove", fmt.Sprintf("Remove header %d", i+1), "trash-2").Disabled(busy).OnClick(func() { v.headers = append(v.headers[:i], v.headers[i+1:]...); p.resetConnectionProbe() })
		})
	}
	connectionButton(c, "connection-header-add", "添加请求头", "plus", busy).Label("Add header").OnClick(func() { v.headers = append(v.headers, connectionHeaderDraft{}); p.resetConnectionProbe() })
}

// resetConnectionProbe invalidates evidence as soon as a source or credential draft changes.
// An old operation retains ownership until its actual service call has returned.
// followConnectionCatalog keeps a form-filled catalog URL on the endpoint's
// origin. The field sits under 高级设置, so a preset's catalog that silently
// stayed on another host after the endpoint changed could not be found.
func (p *NativePage) followConnectionCatalog() {
	v := &p.connections
	if v.draft.CatalogURL != "" && v.draft.CatalogURL != v.autoCatalog {
		return
	}
	next := hub.DefaultCatalogURL(v.draft.Endpoint)
	if next == "" && connectionSameOrigin(v.draft.CatalogURL, v.draft.Endpoint) {
		return // An unrecognised path on the same origin keeps a still-valid catalog.
	}
	v.draft.CatalogURL, v.autoCatalog = next, next
}

// connectionSameOrigin mirrors the scheme/host rule ModelConnection.Validate applies.
func connectionSameOrigin(a, b string) bool {
	x, errX := url.Parse(a)
	y, errY := url.Parse(b)
	return errX == nil && errY == nil && x.Host != "" && x.Scheme == y.Scheme && strings.EqualFold(x.Host, y.Host)
}

func (p *NativePage) resetConnectionProbe() {
	v := &p.connections
	v.requestID++
	if v.requestCancel != nil {
		v.requestCancel()
	}
	v.catalogState, v.testState, v.catalogProblem, v.testProblem = "idle", "idle", "", ""
	v.catalogModels, v.catalogQuery, v.catalogPage = nil, "", 0
}
func (p *NativePage) cancelConnectionProbe() {
	if p.connections.requestCancel != nil {
		p.connections.requestCancel()
	}
}
func (p *NativePage) runConnectionProbe(test bool) {
	v := &p.connections
	if p.Service.Connections == nil || v.requestBusy || v.saving || v.loading {
		return
	}
	draft, valid := p.connectionDraftSnapshot()
	if !valid {
		return
	}
	ctx, cancel := context.WithCancel(p.ctx)
	v.requestID++
	id := v.requestID
	v.requestCancel, v.requestBusy = cancel, true
	if test {
		v.testState, v.testProblem = "loading", ""
	} else {
		v.catalogAction = v.catalogActionLabel()
		v.catalogState, v.catalogProblem = "loading", ""
	}
	go func() {
		var result product.ConnectionCatalog
		var err error
		if test {
			_, err = p.Service.Connections.TestConnection(ctx, draft)
		} else {
			result, err = p.Service.Connections.FetchModels(ctx, draft)
		}
		cancel()
		p.dispatch(func() {
			v.requestCancel, v.requestBusy = nil, false
			if v.problem == "正在取消连接请求，请等待真实清理完成后关闭窗口。" {
				v.problem = "连接请求已取消，现在可以关闭窗口。"
			}
			if id != v.requestID || v.mode != "form" {
				return
			}
			state, message := "ready", ""
			if err != nil {
				state, message = "error", connectionProbeError(err)
				if errors.Is(err, product.ErrConnectionCatalogUnavailable) {
					state = "unavailable"
				}
				if errors.Is(err, context.Canceled) {
					state, message = "idle", "请求已取消；草稿和手填模型已保留。"
				}
			}
			if test {
				v.testState, v.testProblem = state, message
				return
			}
			v.catalogState, v.catalogProblem = state, message
			if err == nil {
				v.catalogModels, v.catalogPage = result.Models, 0
				if len(result.Models) == 0 {
					v.catalogState = "empty"
				}
			}
		})
	}()
}

func (v *modelConnectionsPane) catalogActionLabel() string {
	switch v.catalogState {
	case "loading":
		if v.catalogAction != "" {
			return v.catalogAction
		}
	case "empty":
		return "重新获取"
	case "error":
		return "重试"
	}
	return "获取模型"
}
func connectionProbeError(err error) string {
	switch {
	case errors.Is(err, product.ErrConnectionCatalogUnavailable):
		return "此连接未提供可用的模型目录。可填写同源目录地址，或继续手填准确模型 ID。"
	case errors.Is(err, context.DeadlineExceeded):
		return "请求超时；草稿和手填模型已保留，可明确重试。"
	case errors.Is(err, hub.ErrConnectionConflict):
		return "连接版本已更改，请重新载入后再测试。"
	default:
		return err.Error()
	}
}
func (p *NativePage) buildConnectionProbe(c *ui.Context, busy bool) {
	v := &p.connections
	testing := v.requestBusy && v.testState == "loading"
	p.workspaceControlAction(c, "connection-test", "测试连接", "", "ghost", testing, func() { p.runConnectionProbe(true) }, busy || v.requestBusy && !testing).Label("Test connection catalog access").
		Tooltip("仅验证目录 GET 访问与认证；不会发起生成，也不代表 Agent 兼容。")
	if v.requestBusy {
		workspaceControlButton(c, "connection-request-cancel", "取消请求", "ghost").Label("Cancel connection request").OnClick(p.cancelConnectionProbe)
	}
}

func (p *NativePage) buildConnectionProbeStatus(c *ui.Context) {
	v := &p.connections
	switch v.testState {
	case "loading":
		ui.Text(c, "正在测试目录访问…").Role(ui.RoleStatus).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(c.Theme().TextMuted)
	case "ready":
		ui.Text(c, "目录访问测试通过；生成与 Agent 兼容性尚未验证。").Role(ui.RoleStatus).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(c.Theme().TextMuted)
	}
	if v.testProblem != "" {
		ui.Text(c, v.testProblem).Role(ui.RoleStatus).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(c.Theme().TextMuted)
	}
}
func (p *NativePage) buildConnectionCatalog(c *ui.Context, busy bool) {
	v := &p.connections
	switch v.catalogState {
	case "loading":
		ui.Row(c).Gap(8).AlignItems(ui.Center).Children(func() {
			ui.Icon(c, workspaceIcon("loader-circle")).Size(16, 16).TextColor(c.Theme().TextMuted)
			ui.Text(c, "正在获取模型目录…仍可手填模型。").Role(ui.RoleStatus).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(c.Theme().TextMuted)
		})
	case "empty":
		ui.Text(c, "模型目录请求成功，服务返回空目录。可继续手填。").Role(ui.RoleStatus).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(c.Theme().TextMuted)
	case "unavailable":
		ui.Text(c, v.catalogProblem).Role(ui.RoleStatus).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(c.Theme().TextMuted)
	case "error":
		ui.Row(c).FillWidth().Padding(16).Gap(8).Radius(8).Background(workspacePalette(c).dangerBg).Border(1, workspacePalette(c).dangerBg).AlignItems(ui.Start).Children(func() {
			ui.Icon(c, workspaceIcon("triangle-alert")).Size(workspaceIconSize, workspaceIconSize).Shrink(0).TextColor(c.Theme().Danger)
			ui.Column(c).Grow(1).MinWidth(0).Gap(4).Children(func() {
				ui.Text(c, "模型获取失败").FontSize(workspaceFontBody).FontWeight(600).LineHeight(1.5).TextColor(c.Theme().Danger)
				ui.Text(c, v.catalogProblem).Role(ui.RoleStatus).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(c.Theme().TextMuted)
			})
		})
	case "ready":
		workspaceControlInput(c, "connection-catalog-search", &v.catalogQuery).Label("Search fetched models").Placeholder("搜索模型…").FillWidth().OnChange(func() { v.catalogPage = 0 })
		matches := make([]hub.ConnectionModel, 0, len(v.catalogModels))
		for _, model := range v.catalogModels {
			if strings.Contains(strings.ToLower(model.ID+" "+model.Name), strings.ToLower(v.catalogQuery)) {
				matches = append(matches, model)
			}
		}
		pages := max(1, (len(matches)+7)/8)
		v.catalogPage = min(v.catalogPage, pages-1)
		ui.Row(c).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
			ui.Text(c, fmt.Sprintf("目录 %d 个 · 筛选 %d 个 · 已加入 %d 个", len(v.catalogModels), len(matches), len(v.draft.Models))).Grow(1).FontSize(workspaceFontCaption)
			workspaceControlButton(c, "connection-catalog-previous", "上一页", "ghost", v.catalogPage == 0).Label("Previous model catalog page").OnClick(func() { v.catalogPage-- })
			ui.Text(c, fmt.Sprintf("%d / %d", v.catalogPage+1, pages)).FontSize(workspaceFontCaption)
			workspaceControlButton(c, "connection-catalog-next", "下一页", "ghost", v.catalogPage+1 >= pages).Label("Next model catalog page").OnClick(func() { v.catalogPage++ })
		})
		for _, model := range matches[v.catalogPage*8 : min(len(matches), (v.catalogPage+1)*8)] {
			included := false
			for _, selected := range v.draft.Models {
				included = included || selected.ID == model.ID
			}
			ui.Row(c.Key("candidate-" + model.ID)).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
				ui.Column(c).Grow(1).MinWidth(0).Children(func() {
					ui.Text(c, model.ID)
					if model.Name != "" && model.Name != model.ID {
						ui.Text(c, model.Name).FontSize(workspaceFontCaption).TextColor(c.Theme().TextMuted)
					}
				})
				label := "加入"
				if included {
					label = "已加入"
				}
				workspaceControlButton(c, "add", label, "secondary", busy || included || len(v.draft.Models) >= 128).Label("Add catalog model " + model.ID).OnClick(func() {
					// Catalog candidates never enable themselves; preserve existing manual declarations.
					for _, selected := range v.draft.Models {
						if selected.ID == model.ID {
							return
						}
					}
					v.draft.Models = append(v.draft.Models, model)
				})
			})
		}
		if len(matches) == 0 {
			ui.Text(c, "没有匹配的目录模型").TextColor(c.Theme().TextMuted)
		}
	default:
		ui.Text(c, "尚未获取模型目录；也可直接手填准确 ID。").FontSize(workspaceFontCaption).TextColor(c.Theme().TextMuted)
		if v.catalogProblem != "" {
			ui.Text(c, v.catalogProblem).Role(ui.RoleStatus)
		}
	}
}
