// INPUT: Dialog-local query text, shared Store.PageSessions pages and the page's own session selection owner.
// OUTPUT: Centered saved-session search dialog with committed-query pages, stale-input guard, real retry and cancel.
// POS: [L7 Hub UI] Reads bounded session indexes only; opening, typing, paging or cancelling never navigates or executes.
//go:build desktop

package hubdesktop

import (
	"context"
	"log/slog"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/egoist/mygo/ui"
)

// workspaceSearchPageSize stays inside the shared store's bounded session page
// budget. Paging is real: the dialog never claims a full search from page one.
const workspaceSearchPageSize = 32

// The dialog owns its typed text, committed query, pages, epoch and cancel, so
// the sidebar filter, its paging and its in-flight reads stay independent.
// Future settings/command search would add its own source here; no empty
// category or placeholder result is invented for it.
type workspaceSearchState struct {
	open             bool
	query, submitted string
	hasPage, loading bool
	wantFocus        bool
	problem          string
	sessions         []hub.Session
	page             hub.SessionPage
	cursor           hub.SessionCursor
	back             []hub.SessionCursor
	loadedQuery      string
	request          workspacePageRequest[hub.SessionCursor]
	cancel           context.CancelFunc
	epoch            uint64
	input            ui.Handle
	// Render-side phase dedup belongs to the UI thread, like every other field
	// here. The read-side facts travel by value to their own goroutine instead.
	renderEpoch                 uint64
	renderLoading, renderLogged bool
}

// workspaceSearchValid is the only gate for an actionable result set: the typed
// text must still match the query the visible page was read for, the read must
// have succeeded, and nothing newer may be in flight.
func (p *NativePage) workspaceSearchValid() bool {
	v := &p.search
	return v.open && !v.loading && v.problem == "" && v.hasPage && v.query == v.submitted
}

// Opening raises the dialog, focuses its input on the build that creates it and
// reads the real recent sessions when nothing has been read yet.
func (p *NativePage) openWorkspaceSearch() {
	v := &p.search
	v.open, v.wantFocus = true, true
	v.renderLogged = false
	if v.query == "" && (!v.hasPage || v.loadedQuery != "") {
		p.submitWorkspaceSearch()
	}
}

// workspaceSearchTrace is one search read's numeric diagnostic record: epochs,
// byte lengths, durations, counts and bools only. Query text, session IDs,
// paths and error strings never reach a logger, and the goroutine holds the
// record by value so it never reads dialog fields after handing work to the UI.
type workspaceSearchTrace struct {
	logger      *slog.Logger
	epoch       uint64
	typedBytes  int
	targetBytes int
	started     time.Time
	pageSize    int
}

func (p *NativePage) newWorkspaceSearchTrace(epoch uint64, typed, target string) workspaceSearchTrace {
	return workspaceSearchTrace{
		logger:      workspaceSearchTraceLogger(p.diagnosticLog),
		epoch:       epoch,
		typedBytes:  len(typed),
		targetBytes: len(target),
		started:     time.Now(),
		pageSize:    workspaceSearchPageSize,
	}
}

// log emits one line per phase. A nil logger, which is the ordinary desktop
// case unless the app already owns a diagnostic logger, makes every phase inert.
func (t workspaceSearchTrace) log(phase string, attrs ...any) {
	if t.logger == nil {
		return
	}
	t.logger.Info("native_workspace_search_"+phase,
		append([]any{"epoch", t.epoch, "typed_bytes", t.typedBytes, "target_bytes", t.targetBytes,
			"page_size", t.pageSize, "elapsed_ms", time.Since(t.started).Milliseconds()}, attrs...)...)
}

// traceWorkspaceSearchRender reports the dialog's own epoch/loading transition
// once per change, so a held read, an epoch drop and a real apply are tellable
// apart without any per-frame Build spam.
func (p *NativePage) traceWorkspaceSearchRender() {
	v := &p.search
	logger := workspaceSearchTraceLogger(p.diagnosticLog)
	if logger == nil || (v.renderLogged && v.renderEpoch == v.epoch && v.renderLoading == v.loading) {
		return
	}
	v.renderLogged, v.renderEpoch, v.renderLoading = true, v.epoch, v.loading
	logger.Info("native_workspace_search_render", "epoch", v.epoch, "typed_bytes", len(v.query),
		"target_bytes", len(v.submitted), "loading", v.loading, "has_page", v.hasPage,
		"problem", v.problem != "", "result_count", len(v.sessions), "focus_pending", v.wantFocus)
}

func (p *NativePage) submitWorkspaceSearch() {
	p.loadWorkspaceSearchTarget(workspacePageTarget(hub.SessionCursor{}, nil, p.search.query))
}

func (p *NativePage) closeWorkspaceSearch() {
	v := &p.search
	v.open, v.wantFocus, v.loading = false, false, false
	v.renderLogged = false
	p.cancelWorkspaceSearchRead()
}

func (p *NativePage) cancelWorkspaceSearchRead() {
	v := &p.search
	if v.cancel != nil {
		v.cancel()
		v.cancel = nil
	}
	v.epoch++
}

// A re-entry cancels and epochs out the previous read, so fast re-typing, a
// close, or a reopen cannot let an older page overwrite a newer one.
func (p *NativePage) loadWorkspaceSearchTarget(target workspacePageRequest[hub.SessionCursor]) {
	v := &p.search
	p.cancelWorkspaceSearchRead()
	ctx, cancel := context.WithCancel(p.ctx)
	v.cancel = cancel
	v.epoch++
	epoch, query, cursor := v.epoch, target.query, target.cursor
	v.request = workspacePageTarget(cursor, target.back, query)
	v.loading, v.problem = true, ""
	// request fires after the cancel and epoch settle so the initial read, a
	// pager step and an explicit retry all report the same epoch they carry.
	trace := p.newWorkspaceSearchTrace(epoch, p.search.query, query)
	trace.log("request", "open", v.open, "back_len", len(target.back), "had_page", v.hasPage)
	go func() {
		defer cancel()
		trace.log("store-start", "cancelled_at_start", ctx.Err() != nil)
		storeStarted := time.Now()
		page, err := p.Service.Store.PageSessions(ctx, query, cursor, workspaceSearchPageSize)
		trace.log("store-done", "store_ms", time.Since(storeStarted).Milliseconds(),
			"err", err != nil, "result_count", len(page.Sessions))
		if ctx.Err() != nil {
			trace.log("ctx-skip", "cancelled", true)
			return // cancelled by a newer query, a close, or window teardown
		}
		trace.log("dispatch-enqueued")
		p.dispatch(func() {
			if p.search.epoch != epoch {
				trace.log("epoch-drop", "current_epoch", p.search.epoch)
				return
			}
			v.loading, v.cancel = false, nil
			if err != nil {
				v.problem = err.Error()
				return
			}
			// Requested cursor, query and page history are adopted together,
			// only after a successful read.
			v.sessions, v.page, v.cursor = page.Sessions, page, cursor
			v.back, v.submitted, v.loadedQuery, v.hasPage = target.back, query, query, true
			trace.log("applied", "loading", false, "result_count", len(page.Sessions))
		})
	}()
}

func (p *NativePage) retryWorkspaceSearch() {
	v := &p.search
	if v.loading || v.problem == "" || v.request.query != v.query {
		return
	}
	p.loadWorkspaceSearchTarget(v.request)
}

func (p *NativePage) nextWorkspaceSearch() {
	v := &p.search
	if !p.workspaceSearchValid() || !v.page.HasMore {
		return
	}
	back := append(append([]hub.SessionCursor(nil), v.back...), v.cursor)
	p.loadWorkspaceSearchTarget(workspacePageTarget(v.page.Next, back, v.submitted))
}

func (p *NativePage) previousWorkspaceSearch() {
	v := &p.search
	if !p.workspaceSearchValid() || len(v.back) == 0 {
		return
	}
	last := len(v.back) - 1
	p.loadWorkspaceSearchTarget(workspacePageTarget(v.back[last], v.back[:last], v.submitted))
}

// openWorkspaceSearchResult only accepts an ID from the currently valid page
// and navigates through the existing selection owner, which keeps the drafts
// and flushes the source scope itself. The dialog closes once that navigation
// actually took; a busy page or rejected selection leaves it open.
func (p *NativePage) openWorkspaceSearchResult(id string) {
	if p.busy || id == "" || !p.workspaceSearchValid() {
		return
	}
	var found bool
	for _, session := range p.search.sessions {
		if session.ID == id {
			found = true
			break
		}
	}
	if !found {
		return
	}
	p.selectSession(id)
	if p.selected == id {
		p.closeWorkspaceSearch()
	}
}

func (p *NativePage) buildWorkspaceSearch(c *ui.Context) {
	v := &p.search
	if !v.open {
		if v.loading || v.cancel != nil {
			p.closeWorkspaceSearch() // a closed dialog never keeps a read alive
		}
		return
	}
	modal := ui.DialogBase(c.Key("workspace-search-dialog"), &v.open, func(backdrop, panel ui.Element) {
		t := c.Theme()
		p.traceWorkspaceSearchRender()
		backdrop.Background(ui.RGBA(0, 0, 0, .35))
		width, height := c.Size()
		panel.Label("Saved session search dialog").
			Width(min(float32(560), width-2*workspaceWindowInset)).
			MaxWidth(min(float32(560), width-2*workspaceWindowInset)).
			MaxHeight(min(float32(600), height-2*workspaceWindowInset)).
			Padding(16).Gap(12).Radius(workspacePaneRadius).Background(t.Surface)
		ui.Row(c).FillWidth().Gap(8).AlignItems(ui.Center).Shrink(0).Children(func() {
			ui.Icon(c, workspaceIcon("search")).Size(workspaceIconSize, workspaceIconSize).TextColor(t.TextMuted).Shrink(0)
			search := workspaceControlInput(c, "session-search", &v.query).Label("Find saved session by title or workspace").
				Placeholder("搜索已存任务").Grow(1).MinWidth(0).Bind(&v.input)
			search.OnSubmit(p.submitWorkspaceSearch)
			search.OnShortcut(0, ui.KeyEscape, p.closeWorkspaceSearch)
			if v.wantFocus {
				v.input.Focus(c)
				v.wantFocus = false
			}
			workspaceControlIconButton(c, "session-search-submit", "搜索", "search", "secondary").
				Label("Submit saved session search").OnClick(p.submitWorkspaceSearch)
			workspaceIconAction(c, "workspace-search-close", "Close session search", "x").
				Tooltip("关闭搜索").OnClick(p.closeWorkspaceSearch)
		})
		valid := p.workspaceSearchValid()
		ui.Scroll(c.Key("session-search-results")).Label("Saved session search results").
			Grow(1).MinHeight(0).Gap(4).Children(func() {
			switch {
			case v.loading:
				workspaceSearchStatus(c, "正在搜索已存任务…", "Saved session search loading")
			case v.problem != "":
				ui.Row(c).FillWidth().Gap(8).AlignItems(ui.Center).Children(func() {
					ui.Text(c, v.problem).Label("Saved session search error").Grow(1).MinWidth(0).
						FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(t.Danger)
					workspaceControlButton(c, "session-search-retry", "重试", "danger", v.request.query != v.query).
						Label("Retry saved session search").OnClick(p.retryWorkspaceSearch)
				})
			case !valid:
				// Typed but uncommitted text has no result set yet; the visible
				// list stays empty instead of reusing the previous query's page.
				workspaceSearchStatus(c, "未搜索", "Saved session search not submitted")
			case len(v.sessions) == 0:
				label := "没有匹配的任务"
				if v.submitted == "" {
					label = "还没有已存任务"
				}
				workspaceSearchStatus(c, label, "Saved session search empty")
			default:
				for _, session := range v.sessions {
					p.buildWorkspaceSearchResult(c, session)
				}
			}
		})
		if valid && (v.page.HasMore || len(v.back) > 0) {
			ui.Row(c).FillWidth().Gap(8).AlignItems(ui.Center).Shrink(0).Children(func() {
				workspaceControlButton(c, "session-search-previous", "上一页", "secondary", !valid || len(v.back) == 0).
					Label("Previous saved session page").OnClick(p.previousWorkspaceSearch)
				ui.Spacer(c)
				workspaceControlButton(c, "session-search-next", "下一页", "secondary", !valid || !v.page.HasMore).
					Label("Next saved session page").OnClick(p.nextWorkspaceSearch)
			})
		}
	})
	if modal.Dismissed() {
		p.closeWorkspaceSearch() // Escape and backdrop cancel the read too
	}
}

func workspaceSearchStatus(c *ui.Context, label, accessible string) {
	t := c.Theme()
	ui.Text(c, label).Label(accessible).FillWidth().Padding(4, 8).
		FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(t.TextMuted)
}

func (p *NativePage) buildWorkspaceSearchResult(c *ui.Context, session hub.Session) {
	t := c.Theme()
	label := p.sessionLabel(session)
	selected := p.selected == session.ID
	row := ui.ButtonBase(c.Key("session-result-"+session.ID)).Label("Open saved session "+session.ID).
		Tooltip(label).FillWidth().MinWidth(0).MinHeight(workspaceNavigationRowHeight).
		Padding(4, 8).Gap(8).AlignItems(ui.Center).Radius(8).Selected(selected).Disabled(p.busy)
	if selected {
		row.Background(t.Surface)
	} else if row.Hovered() {
		row.Background(t.SurfaceHover)
	}
	row.OnClick(func() { p.openWorkspaceSearchResult(session.ID) }).Children(func() {
		iconColor := t.TextMuted
		if selected {
			iconColor = t.Text
		}
		ui.Icon(c, workspaceIcon("message-square")).Size(workspaceIconSize, workspaceIconSize).TextColor(iconColor).Shrink(0)
		directory := session.Workspace
		if directory == "" {
			directory = "未指定项目"
		}
		ui.Column(c).Grow(1).MinWidth(0).Children(func() {
			ui.Text(c, session.Title).Label("Session title " + session.ID).FillWidth().SingleLine().
				FontSize(workspaceFontBody).LineHeight(1.5).TextColor(t.Text)
			ui.Text(c, directory).Label("Session workspace " + session.ID).FillWidth().SingleLine().
				FontSize(workspaceFontCaption).LineHeight(1.5).TextColor(t.TextMuted)
		})
	})
}
