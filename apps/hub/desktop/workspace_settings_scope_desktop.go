// INPUT: Explicit destination, bounded saved-session pages and the existing session navigation owner.
// OUTPUT: Shared settings/resource/automation scope chooser with cold reads and explicit open after selection.
// POS: [L7 Hub UI] Choosing scope preserves real drafts and never creates a session, sends or grants permission.
//go:build desktop

package hubdesktop

import (
	"context"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/egoist/mygo/ui"
)

type workspaceSettingsScopeState struct {
	destination, query, loadedQuery string
	page                            hub.SessionPage
	cursor                          hub.SessionCursor
	back                            []hub.SessionCursor
	loading                         bool
	problem                         string
	epoch                           uint64
	cancel                          context.CancelFunc
	selected                        string
	selectedGeneration              uint64
	candidate                       hub.Session
	request                         workspacePageRequest[hub.SessionCursor]
	requestCandidateID              string
	list                            ui.ListState
}

func workspaceScopeDestination(destination string) bool {
	switch destination {
	case "resources", "automation", "plugins", "rules", "data", "security":
		return true
	}
	return false
}

func (p *NativePage) openWorkspaceResourceChooser() {
	p.openWorkspaceSessionChooser("resources")
}

func (p *NativePage) openWorkspaceSessionChooser(destination string) {
	if !workspaceScopeDestination(destination) || p.busy || p.paneBusy {
		return
	}
	if p.settingsScope.cancel != nil {
		p.settingsScope.cancel()
	}
	epoch := p.settingsScope.epoch + 1
	p.settingsScope = workspaceSettingsScopeState{destination: destination, epoch: epoch}
	p.setPane(workspaceScopePane(destination))
	p.loadWorkspaceScopeSessions(hub.SessionCursor{})
}

func workspaceScopePane(destination string) string {
	if destination == "resources" || destination == "automation" {
		return "workspace-scope"
	}
	return "settings-scope"
}

// An index row may name an existing owner directly. Verify that exact identity
// before offering selection; do not reconstruct an owner from a title/path.
func (p *NativePage) openWorkspaceSessionChooserForSession(destination, id string) {
	if id == "" || !workspaceScopeDestination(destination) || p.busy || p.paneBusy {
		return
	}
	if p.settingsScope.cancel != nil {
		p.settingsScope.cancel()
	}
	epoch := p.settingsScope.epoch + 1
	ctx, cancel := context.WithCancel(p.ctx)
	p.settingsScope = workspaceSettingsScopeState{destination: destination, epoch: epoch, cancel: cancel, loading: true, requestCandidateID: id}
	p.setPane(workspaceScopePane(destination))
	go func() {
		defer cancel()
		detail, err := p.Service.Conversations.Detail(ctx, id)
		p.dispatch(func() {
			s := &p.settingsScope
			if s.epoch != epoch || s.destination != destination {
				return
			}
			s.loading, s.cancel = false, nil
			if err != nil {
				s.problem = err.Error()
				return
			}
			if detail.Session.ID != id {
				s.problem = "未找到此任务，请重新选择。"
				return
			}
			s.candidate = detail.Session
		})
	}()
}

func (p *NativePage) loadWorkspaceScopeSessions(cursor hub.SessionCursor) {
	p.loadWorkspaceScopeSessionsTarget(workspacePageTarget(cursor, p.settingsScope.back, p.settingsScope.query))
}

func (p *NativePage) loadWorkspaceScopeSessionsTarget(target workspacePageRequest[hub.SessionCursor]) {
	s := &p.settingsScope
	if s.loading {
		return
	}
	s.epoch++
	epoch, query, cursor, destination := s.epoch, target.query, target.cursor, s.destination
	s.request = workspacePageTarget(cursor, target.back, query)
	s.requestCandidateID = ""
	ctx, cancel := context.WithCancel(p.ctx)
	s.cancel, s.loading, s.problem = cancel, true, ""
	// A different result page is a new choice surface. Do not leave Open bound
	// to a prior row that may no longer be visible after search or pagination.
	s.selected, s.selectedGeneration = "", 0
	go func() {
		defer cancel()
		page, err := p.Service.Conversations.Page(ctx, query, cursor, 16)
		p.dispatch(func() {
			if s.epoch != epoch || s.destination != destination {
				return
			}
			s.loading, s.cancel = false, nil
			if err != nil {
				s.problem = err.Error()
				return
			}
			s.page, s.cursor, s.loadedQuery, s.back = page, cursor, query, target.back
			s.candidate = hub.Session{}
		})
	}()
}

func (p *NativePage) refreshWorkspaceScopeSessions() {
	if p.pane != workspaceScopePane(p.settingsScope.destination) {
		return
	}
	p.loadWorkspaceScopeSessionsTarget(workspacePageTarget(hub.SessionCursor{}, nil, p.settingsScope.query))
}

func (p *NativePage) retryWorkspaceScopeRead() {
	s := &p.settingsScope
	if s.loading || s.problem == "" || p.busy || p.paneBusy || p.pane != workspaceScopePane(s.destination) {
		return
	}
	if s.requestCandidateID != "" {
		p.openWorkspaceSessionChooserForSession(s.destination, s.requestCandidateID)
		return
	}
	if s.request.query == s.query {
		p.loadWorkspaceScopeSessionsTarget(s.request)
	}
}

func (p *NativePage) nextWorkspaceScopeSessions() {
	s := &p.settingsScope
	if s.loading || !s.page.HasMore || s.query != s.loadedQuery || p.pane != workspaceScopePane(s.destination) {
		return
	}
	target := workspacePageTarget(s.page.Next, s.back, s.loadedQuery)
	target.back = append(target.back, s.cursor)
	p.loadWorkspaceScopeSessionsTarget(target)
}

func (p *NativePage) previousWorkspaceScopeSessions() {
	s := &p.settingsScope
	if s.loading || len(s.back) == 0 || s.query != s.loadedQuery || p.pane != workspaceScopePane(s.destination) {
		return
	}
	last := len(s.back) - 1
	p.loadWorkspaceScopeSessionsTarget(workspacePageTarget(s.back[last], s.back[:last], s.loadedQuery))
}

func (s *workspaceSettingsScopeState) contains(id string) bool {
	if s.candidate.ID != "" {
		return s.candidate.ID == id
	}
	for _, session := range s.page.Sessions {
		if session.ID == id {
			return true
		}
	}
	return false
}

func (p *NativePage) selectWorkspaceScopeSession(id string) {
	if id == "" || p.busy || p.paneBusy || p.settingsScope.loading || p.pane != workspaceScopePane(p.settingsScope.destination) || !p.settingsScope.contains(id) {
		return
	}
	if p.settingsScope.selected == id && p.selected == id && (p.chat.Busy || p.workspaceScopeReady()) {
		return
	}
	destination := p.settingsScope.destination
	p.selectSession(id)
	if p.selected != id {
		return
	}
	p.settingsScope.selected, p.settingsScope.selectedGeneration = id, p.generation
	p.setPane(workspaceScopePane(destination))
}

func (p *NativePage) workspaceScopeReady() bool {
	s := &p.settingsScope
	return !s.loading && s.selected != "" && s.contains(s.selected) && p.selected == s.selected && p.generation == s.selectedGeneration &&
		p.workspaceNavigationDetailReady() && !p.busy && !p.chat.Busy
}

func (p *NativePage) openSelectedWorkspaceScope() {
	if !p.workspaceScopeReady() || p.paneBusy || p.pane != workspaceScopePane(p.settingsScope.destination) {
		return
	}
	switch p.settingsScope.destination {
	case "resources":
		p.setPane("")
		p.openWorkspaceResources()
	case "automation":
		p.openAutomations()
	default:
		p.openWorkspaceSettingsCategory(p.settingsScope.destination)
	}
}

func (p *NativePage) buildWorkspaceSessionScope(c *ui.Context) bool {
	if p.pane != "workspace-scope" {
		return false
	}
	ui.Column(c).FillWidth().MaxWidth(workspaceReadingWidth + 32).AlignSelf(ui.Start).Grow(1).MinHeight(0).Padding(16).Gap(12).Children(func() { p.buildWorkspaceScopeChooser(c) })
	return true
}

func (p *NativePage) cancelWorkspaceScopeSelection() {
	s := &p.settingsScope
	if s.cancel != nil {
		s.cancel()
	}
	s.selected, s.selectedGeneration = "", 0
	if s.destination == "resources" {
		p.setPane("")
	} else if s.destination == "automation" {
		p.openWorkspaceAutomations()
	} else {
		p.openWorkspaceSettingsCategory(s.destination)
	}
}

func (p *NativePage) buildWorkspaceScopeChooser(c *ui.Context) {
	s := &p.settingsScope
	t := c.Theme()
	if p.pane != "workspace-scope" {
		ui.Row(c).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
			ui.Text(c, "选择任务").FontSize(workspaceFontSubtitle).FontWeight(600).Grow(1)
			workspaceControlButton(c, "scope-cancel", "返回", "ghost", p.busy).Label("Cancel scope selection").OnClick(p.cancelWorkspaceScopeSelection)
		})
	}
	ui.Row(c).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
		search := workspaceControlInput(c, "scope-session-query", &s.query).Label("Find scope session").Placeholder("搜索任务与项目").Grow(1).MinWidth(0)
		if search.Submitted() && !s.loading {
			p.refreshWorkspaceScopeSessions()
		}
		workspaceControlButton(c, "scope-session-search", "搜索", "secondary", s.loading).Label("Search scope sessions").OnClick(p.refreshWorkspaceScopeSessions)
	})
	if s.problem != "" {
		ui.Text(c, s.problem).Role(ui.RoleStatus).FontSize(workspaceFontBody).LineHeight(1.5).TextColor(t.Danger)
		workspaceControlButton(c, "scope-retry", "重试", "secondary", s.loading || p.busy || p.paneBusy || s.requestCandidateID == "" && s.request.query != s.query).
			Label("Retry scope sessions").OnClick(p.retryWorkspaceScopeRead)
	}
	if s.loading {
		ui.Text(c, "正在读取任务…").Role(ui.RoleStatus).FontSize(workspaceFontBody).TextColor(t.TextMuted)
	}
	if s.candidate.ID != "" {
		candidate := s.candidate
		workspaceAction(c, "scope-candidate", candidate.Title, "message-square", s.selected == candidate.ID).Height(workspaceControlHeight).Padding(0, 8).
			Label("Select scope session " + candidate.ID).Disabled(s.loading || p.busy || p.paneBusy).OnClick(func() { p.selectWorkspaceScopeSession(candidate.ID) })
	} else {
		s.list.Key = func(i int) any { return s.page.Sessions[i].ID }
		s.list.Label = func(i int) string { return s.page.Sessions[i].Title }
		ui.List(c.Key("scope-session-list"), &s.list, len(s.page.Sessions), func(i int) {
			session := s.page.Sessions[i]
			workspaceAction(c, "scope-session:"+session.ID, session.Title, "message-square", s.selected == session.ID).Height(workspaceControlHeight).Padding(0, 8).
				Label("Select scope session " + session.ID).Tooltip(session.Workspace).Disabled(s.loading || p.busy || p.paneBusy).
				OnClick(func() { p.selectWorkspaceScopeSession(session.ID) })
		}).FillWidth().Grow(1).MinHeight(0).Gap(4)
		if !s.loading && len(s.page.Sessions) == 0 && s.problem == "" {
			ui.Text(c, "没有找到已保存的任务。").FontSize(workspaceFontBody).TextColor(t.TextMuted)
		}
	}
	ui.Row(c).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
		workspaceIconAction(c, "scope-previous", "Previous scope sessions", "chevron-left").Disabled(s.loading || len(s.back) == 0 || s.query != s.loadedQuery).
			OnClick(p.previousWorkspaceScopeSessions)
		workspaceIconAction(c, "scope-next", "More scope sessions", "chevron-right").Disabled(s.loading || !s.page.HasMore || s.query != s.loadedQuery).
			OnClick(p.nextWorkspaceScopeSessions)
		ui.Spacer(c)
		workspaceControlButton(c, "scope-open", "打开", "primary", !p.workspaceScopeReady() || p.paneBusy).Label("Open selected scope").OnClick(p.openSelectedWorkspaceScope)
	})
}

func workspaceScopeLabel(destination string) string {
	switch destination {
	case "resources":
		return "文件与任务"
	case "automation":
		return "自动化"
	case "plugins":
		return "插件与工具"
	case "rules":
		return "规则与 Skills"
	case "data":
		return "数据与存储"
	case "security":
		return "权限与安全"
	}
	return "设置"
}
