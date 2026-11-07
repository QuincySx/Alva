// INPUT: Explicit shared workspace/session indexes, current recent page and native navigation controls.
// OUTPUT: Bounded project hierarchy and separate recent-session paging with truthful loading/empty/error states.
// POS: [L7 Hub UI] Cold projections only; browsing or expanding a project never creates sessions or reads file bodies.
//go:build desktop

package hubdesktop

import (
	"context"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/egoist/mygo/ui"
)

type workspaceNavigationState struct {
	page     hub.WorkspacePage
	cursor   hub.WorkspaceCursor
	back     []hub.WorkspaceCursor
	query    string
	loaded   bool
	loading  bool
	problem  string
	epoch    uint64
	cancel   context.CancelFunc
	projects map[string]*workspaceProjectNavigationState
	request  workspacePageRequest[hub.WorkspaceCursor]
	detail   workspaceNavigationDetail
}

type workspaceNavigationDetail struct {
	session, branch string
	generation      uint64
}

// Only the existing, successfully validated Detail read owner calls this.
// Generic operation Busy flags are not evidence of a matching read receipt.
func (p *NativePage) acceptWorkspaceNavigationDetail(id string, generation uint64) {
	if id == "" || p.selected != id || p.generation != generation || p.detail.Session.ID != id || p.detail.Session.ActiveBranchID == "" {
		return
	}
	p.navigation.detail = workspaceNavigationDetail{session: id, branch: p.detail.Session.ActiveBranchID, generation: generation}
}

func (p *NativePage) workspaceNavigationDetailReady() bool {
	v := p.navigation.detail
	return p.selected != "" && v.session == p.selected && v.generation == p.generation &&
		p.detail.Session.ID == v.session && v.branch != "" && p.detail.Session.ActiveBranchID == v.branch
}

// Keep the requested cursor/query/history separate from the last committed
// page. All three owners adopt them together, only after a successful read.
type workspacePageRequest[C any] struct {
	cursor C
	back   []C
	query  string
}

func workspacePageTarget[C any](cursor C, back []C, query string) workspacePageRequest[C] {
	return workspacePageRequest[C]{cursor: cursor, back: append([]C(nil), back...), query: query}
}

type workspaceProjectNavigationState struct {
	open, loaded, loading bool
	page                  hub.SessionPage
	cursor                hub.SessionCursor
	back                  []hub.SessionCursor
	problem               string
	request               workspacePageRequest[hub.SessionCursor]
}

func (p *NativePage) refreshWorkspaceNavigation() {
	p.refreshSessions()
	p.refreshWorkspaceProjects()
}

func (p *NativePage) refreshWorkspaceProjects() {
	p.loadWorkspaceProjectsTarget(workspacePageTarget(hub.WorkspaceCursor{}, nil, p.sessionQuery))
}

func (p *NativePage) loadWorkspaceProjects(cursor hub.WorkspaceCursor) {
	p.loadWorkspaceProjectsTarget(workspacePageTarget(cursor, p.navigation.back, p.sessionQuery))
}

func (p *NativePage) loadWorkspaceProjectsTarget(target workspacePageRequest[hub.WorkspaceCursor]) {
	v := &p.navigation
	if v.cancel != nil {
		v.cancel()
	}
	ctx, cancel := context.WithCancel(p.ctx)
	v.cancel = cancel
	v.epoch++
	epoch, query, cursor := v.epoch, target.query, target.cursor
	v.request = workspacePageTarget(cursor, target.back, query)
	v.loading, v.problem = true, ""
	go func() {
		defer cancel()
		page, err := p.Service.BrowseWorkspaces(ctx, query, cursor, 8)
		p.dispatch(func() {
			if p.navigation.epoch != epoch {
				return
			}
			v.loading, v.cancel = false, nil
			if err != nil {
				v.problem = err.Error()
				return
			}
			v.page, v.cursor, v.query, v.back, v.loaded = page, cursor, query, target.back, true
			// A page replacement invalidates old child receipts, even when the
			// same path reappears after a later cursor/query change.
			v.projects = make(map[string]*workspaceProjectNavigationState)
		})
	}()
}

func (p *NativePage) retryWorkspaceProjects() {
	v := &p.navigation
	if v.loading || v.problem == "" || v.request.query != p.sessionQuery {
		return
	}
	p.loadWorkspaceProjectsTarget(v.request)
}

func (p *NativePage) nextWorkspaceProjects() {
	v := &p.navigation
	if v.loading || !v.page.HasMore || v.query != p.sessionQuery {
		return
	}
	target := workspacePageTarget(v.page.Next, v.back, v.query)
	target.back = append(target.back, v.cursor)
	p.loadWorkspaceProjectsTarget(target)
}

func (p *NativePage) previousWorkspaceProjects() {
	v := &p.navigation
	if v.loading || len(v.back) == 0 || v.query != p.sessionQuery {
		return
	}
	last := len(v.back) - 1
	p.loadWorkspaceProjectsTarget(workspacePageTarget(v.back[last], v.back[:last], v.query))
}

func (p *NativePage) toggleWorkspaceProject(path string) {
	v := &p.navigation
	if v.loading || v.query != p.sessionQuery {
		return
	}
	if v.projects == nil {
		v.projects = make(map[string]*workspaceProjectNavigationState)
	}
	s := v.projects[path]
	if s == nil {
		s = &workspaceProjectNavigationState{}
		v.projects[path] = s
	}
	s.open = !s.open
	if s.open && !s.loaded && !s.loading {
		p.loadWorkspaceProjectSessions(path, hub.SessionCursor{})
	}
}

func (p *NativePage) loadWorkspaceProjectSessions(path string, cursor hub.SessionCursor) {
	s := p.navigation.projects[path]
	if s == nil {
		return
	}
	p.loadWorkspaceProjectSessionsTarget(path, workspacePageTarget(cursor, s.back, p.navigation.query))
}

func (p *NativePage) loadWorkspaceProjectSessionsTarget(path string, target workspacePageRequest[hub.SessionCursor]) {
	s := p.navigation.projects[path]
	if s == nil || s.loading || p.navigation.loading {
		return
	}
	s.loading, s.problem = true, ""
	query, cursor, epoch := target.query, target.cursor, p.navigation.epoch
	s.request = workspacePageTarget(cursor, target.back, query)
	go func() {
		page, err := p.Service.BrowseWorkspaceSessions(p.ctx, path, query, cursor, 16)
		p.dispatch(func() {
			// This state admits only one child read at a time. Settle that
			// owner's loading flag even if a failed parent refresh retained
			// the same map after invalidating this receipt's parent epoch.
			s.loading = false
			if p.navigation.projects[path] != s {
				return
			}
			if p.navigation.epoch != epoch {
				s.problem = "项目列表已刷新，请重试任务加载"
				return
			}
			if err != nil {
				s.problem = err.Error()
				return
			}
			s.page, s.cursor, s.back, s.loaded = page, cursor, target.back, true
		})
	}()
}

func (p *NativePage) retryWorkspaceProjectSessions(path string) {
	s := p.navigation.projects[path]
	if s == nil || s.loading || s.problem == "" || s.request.query != p.navigation.query || p.navigation.query != p.sessionQuery {
		return
	}
	p.loadWorkspaceProjectSessionsTarget(path, s.request)
}

func (p *NativePage) nextWorkspaceProjectSessions(path string) {
	s := p.navigation.projects[path]
	if s == nil || s.loading || !s.page.HasMore || p.navigation.loading || p.navigation.query != p.sessionQuery {
		return
	}
	target := workspacePageTarget(s.page.Next, s.back, p.navigation.query)
	target.back = append(target.back, s.cursor)
	p.loadWorkspaceProjectSessionsTarget(path, target)
}

func (p *NativePage) previousWorkspaceProjectSessions(path string) {
	s := p.navigation.projects[path]
	if s == nil || s.loading || len(s.back) == 0 || p.navigation.loading || p.navigation.query != p.sessionQuery {
		return
	}
	last := len(s.back) - 1
	p.loadWorkspaceProjectSessionsTarget(path, workspacePageTarget(s.back[last], s.back[:last], p.navigation.query))
}

type workspaceNavigationRow struct {
	kind, key, label, path string
	project                hub.WorkspaceEntry
	session                hub.Session
}

func (p *NativePage) workspaceNavigationRows() []workspaceNavigationRow {
	rows := []workspaceNavigationRow{{kind: "heading", key: "projects-heading", label: "项目"}}
	v := &p.navigation
	if v.loading {
		rows = append(rows, workspaceNavigationRow{kind: "status", key: "projects-loading", label: "正在加载项目…"})
	} else if v.problem != "" {
		rows = append(rows, workspaceNavigationRow{kind: "project-error", key: "projects-error", label: "项目加载失败"})
	} else if !v.loaded {
		rows = append(rows, workspaceNavigationRow{kind: "project-load", key: "projects-load", label: "载入项目"})
	} else if len(v.page.Items) == 0 {
		label := "还没有已保存的项目"
		if v.query != "" {
			label = "没有匹配的项目"
		} else if v.cursor.Path != "" {
			label = "此页没有项目"
		}
		rows = append(rows, workspaceNavigationRow{kind: "status", key: "projects-empty", label: label})
	}
	for _, project := range v.page.Items {
		rows = append(rows, workspaceNavigationRow{kind: "project", key: "project:" + project.Path, label: project.Name, path: project.Path, project: project})
		s := v.projects[project.Path]
		if s == nil || !s.open {
			continue
		}
		if s.loading {
			rows = append(rows, workspaceNavigationRow{kind: "status", key: "project-loading:" + project.Path, label: "正在加载任务…"})
		} else if s.problem != "" {
			rows = append(rows, workspaceNavigationRow{kind: "child-error", key: "project-error:" + project.Path, label: "任务加载失败", path: project.Path})
		} else if len(s.page.Sessions) == 0 {
			rows = append(rows, workspaceNavigationRow{kind: "status", key: "project-empty:" + project.Path, label: "此页没有匹配的任务"})
		}
		for _, session := range s.page.Sessions {
			rows = append(rows, workspaceNavigationRow{kind: "session", key: "project-session:" + project.Path + ":" + session.ID, label: p.sessionLabel(session), session: session})
		}
		if len(s.page.Sessions) > 0 {
			rows = append(rows, workspaceNavigationRow{kind: "files", key: "project-files:" + project.Path, label: "项目文件", path: project.Path, session: s.page.Sessions[0]})
		}
		if len(s.back) > 0 || s.page.HasMore {
			rows = append(rows, workspaceNavigationRow{kind: "child-pager", key: "project-pager:" + project.Path, path: project.Path, label: "项目任务分页"})
		}
	}
	if len(v.back) > 0 || v.page.HasMore {
		rows = append(rows, workspaceNavigationRow{kind: "projects-pager", key: "projects-pager", label: "项目分页"})
	}
	rows = append(rows, workspaceNavigationRow{kind: "heading", key: "recent-heading", label: "最近"})
	if p.sessionLoading {
		rows = append(rows, workspaceNavigationRow{kind: "status", key: "recent-loading", label: "正在加载任务…"})
	} else if len(p.sessions) == 0 {
		label := "还没有任务"
		if p.sessionLoadedQuery != "" {
			label = "没有匹配的任务"
		} else if p.sessionCursor != (hub.SessionCursor{}) {
			label = "此页没有任务"
		}
		rows = append(rows, workspaceNavigationRow{kind: "status", key: "recent-empty", label: label})
	}
	for _, session := range p.sessions {
		rows = append(rows, workspaceNavigationRow{kind: "recent", key: "recent:" + session.ID, label: p.sessionLabel(session), session: session})
	}
	if len(p.sessionBack) > 0 || p.sessionPage.HasMore {
		rows = append(rows, workspaceNavigationRow{kind: "recent-pager", key: "recent-pager", label: "最近任务分页"})
	}
	return rows
}

func (p *NativePage) buildWorkspaceNavigation(c *ui.Context) {
	rows := p.workspaceNavigationRows()
	p.list.Key = func(i int) any { return rows[i].key }
	p.list.Label = func(i int) string { return rows[i].label }
	ui.List(c.Key("workspace-session-list"), &p.list, len(rows), func(i int) {
		r := rows[i]
		switch r.kind {
		case "heading":
			if !p.layout.sidebarCollapsed {
				ui.Text(c, r.label).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(c.Theme().TextMuted).Padding(0, 12).Margin(4, 0, 0)
			}
		case "status":
			if !p.layout.sidebarCollapsed {
				ui.Text(c, r.label).FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(c.Theme().TextMuted).Padding(4, 12)
			}
		case "project-error", "child-error":
			disabled := p.navigation.loading || p.navigation.request.query != p.sessionQuery
			if r.kind == "child-error" {
				child := p.navigation.projects[r.path]
				disabled = p.navigation.loading || child.loading || p.navigation.query != p.sessionQuery
			}
			p.workspaceNavigationAction(c, r.key, r.label+" · 重试", "triangle-alert", false).
				Disabled(disabled).OnClick(func() {
				if r.kind == "project-error" {
					p.retryWorkspaceProjects()
				} else {
					p.retryWorkspaceProjectSessions(r.path)
				}
			})
		case "project-load":
			p.workspaceNavigationAction(c, r.key, r.label, "folder", false).OnClick(p.refreshWorkspaceProjects)
		case "project":
			p.workspaceNavigationAction(c, r.key, r.label, "folder", false).Tooltip(r.project.Path).
				Disabled(p.navigation.loading || p.navigation.query != p.sessionQuery).OnClick(func() { p.toggleWorkspaceProject(r.path) })
		case "session", "recent":
			p.buildWorkspaceNavigationSession(c, r.session, r.kind == "recent")
		case "files":
			p.workspaceNavigationAction(c, r.key, r.label, "folder", false).
				Disabled(p.busy || p.Service.WorkspaceViews == nil).OnClick(func() { p.openWorkspaceSessionChooserForSession("resources", r.session.ID) })
		case "projects-pager":
			p.buildWorkspaceProjectPager(c)
		case "child-pager":
			p.buildWorkspaceProjectSessionPager(c, r.path)
		case "recent-pager":
			p.buildWorkspaceRecentPager(c)
		}
	}).Grow(1).MinHeight(0).Gap(4)
}

func (p *NativePage) workspaceNavigationAction(c *ui.Context, key, label, icon string, selected bool) ui.Element {
	if p.layout.sidebarCollapsed {
		return workspaceIconAction(c, key, label, icon).AlignSelf(ui.Center)
	}
	return workspaceAction(c, key, label, icon, selected).Height(workspaceNavigationRowHeight).Padding(0, 8)
}

func (p *NativePage) buildWorkspaceProjectPager(c *ui.Context) {
	v := &p.navigation
	p.buildWorkspaceNavigationPager(c, "projects", "Previous projects", "More projects", v.loading || v.query != p.sessionQuery,
		len(v.back) > 0, v.page.HasMore, p.previousWorkspaceProjects, p.nextWorkspaceProjects)
}

func (p *NativePage) buildWorkspaceProjectSessionPager(c *ui.Context, path string) {
	s := p.navigation.projects[path]
	p.buildWorkspaceNavigationPager(c, "project:"+path, "Previous project tasks", "More project tasks", s.loading || p.navigation.loading || p.navigation.query != p.sessionQuery,
		len(s.back) > 0, s.page.HasMore, func() { p.previousWorkspaceProjectSessions(path) }, func() { p.nextWorkspaceProjectSessions(path) })
}

func (p *NativePage) buildWorkspaceRecentPager(c *ui.Context) {
	p.buildWorkspaceNavigationPager(c, "recent", "Previous sessions", "More sessions", p.sessionLoading || p.sessionLoadedQuery != p.sessionQuery,
		len(p.sessionBack) > 0, p.sessionPage.HasMore, func() {
			last := len(p.sessionBack) - 1
			cursor := p.sessionBack[last]
			p.sessionBack = p.sessionBack[:last]
			p.loadSessions(cursor)
		}, func() { p.sessionBack = append(p.sessionBack, p.sessionCursor); p.loadSessions(p.sessionPage.Next) })
}

func (p *NativePage) buildWorkspaceNavigationPager(c *ui.Context, key, previous, next string, busy, hasPrevious, hasNext bool, goPrevious, goNext func()) {
	pager := ui.Row(c.Key(key)).Gap(8).AlignItems(ui.Center).FillWidth()
	if p.layout.sidebarCollapsed {
		pager.Column()
	}
	pager.Children(func() {
		workspaceIconAction(c, key+"-previous", previous, "chevron-left").Disabled(busy || !hasPrevious).OnClick(goPrevious)
		if !p.layout.sidebarCollapsed {
			ui.Spacer(c)
		}
		workspaceIconAction(c, key+"-next", next, "chevron-right").Disabled(busy || !hasNext).OnClick(goNext)
	})
}
