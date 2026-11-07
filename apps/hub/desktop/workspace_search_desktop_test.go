// INPUT: Real saved-session indexes, one blocking/failing search store and native headless frames.
// OUTPUT: Centered dialog, Enter submit, real paging, stale/cancelled read isolation, retry, viewport-visible results, navigation.
// POS: [L7 Hub UI testing] No model, engine adapter, OS window or account is used by these regressions.
//go:build desktop

package hubdesktop

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strconv"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/egoist/mygo/ui"
)

// workspaceSearchGateStore is the narrow search-only probe: it records the real
// query/limit the shared store received, can fail once, and can block a read so
// cancellation and stale-page isolation are observable instead of assumed.
type workspaceSearchGateStore struct {
	product.ServiceStore
	fail, block atomic.Bool
	entered     chan string
	cancelled   chan string
	release     chan struct{}
	releaseOnce sync.Once
	mu          sync.Mutex
	queries     []string
	limits      []int
}

func newWorkspaceSearchGateStore(store product.ServiceStore) *workspaceSearchGateStore {
	return &workspaceSearchGateStore{ServiceStore: store, entered: make(chan string, 4), cancelled: make(chan string, 4),
		release: make(chan struct{}),
	}
}

func (s *workspaceSearchGateStore) releaseReads() { s.releaseOnce.Do(func() { close(s.release) }) }

func (s *workspaceSearchGateStore) record(query string, limit int) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.queries = append(s.queries, query)
	s.limits = append(s.limits, limit)
}

// seenReports renders what the shared store was actually asked for, so a test
// can prove bounded paging and query isolation instead of trusting the page.
func (s *workspaceSearchGateStore) seenReports() string {
	s.mu.Lock()
	defer s.mu.Unlock()
	report := "queries="
	for i, query := range s.queries {
		if i > 0 {
			report += ","
		}
		report += "«" + query + "»"
	}
	return report + " limits=" + itoa(s.limits)
}

func itoa(values []int) string {
	out := ""
	for i, value := range values {
		if i > 0 {
			out += ","
		}
		out += strconv.Itoa(value)
	}
	return out
}

func (s *workspaceSearchGateStore) PageSessions(ctx context.Context, query string, cursor hub.SessionCursor, limit int) (hub.SessionPage, error) {
	s.record(query, limit)
	if s.fail.Swap(false) {
		return hub.SessionPage{}, errors.New("fixture session search read failed")
	}
	if s.block.Swap(false) {
		s.entered <- query
		select {
		case <-s.release:
		case <-ctx.Done():
			select {
			case s.cancelled <- query:
			default:
			}
			return hub.SessionPage{}, ctx.Err()
		}
	}
	return s.ServiceStore.PageSessions(ctx, query, cursor, limit)
}

func workspaceSearchPage(t *testing.T, service *product.HubService, width, height int) (*NativePage, *ui.Tester, *workspaceSearchGateStore) {
	t.Helper()
	store := newWorkspaceSearchGateStore(service.Store)
	service.Store = store
	p := NewNativePage(service)
	p.ctx = t.Context()
	return p, ui.NewTester(workspaceStaticView(p), width, height), store
}

func waitWorkspaceSearch(t *testing.T, tester *ui.Tester, ready func() bool) {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		tester.Frame()
		if ready() {
			return
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("session search dialog did not settle")
}

func awaitSearchRead(t *testing.T, store *workspaceSearchGateStore, query string) {
	t.Helper()
	select {
	case got := <-store.entered:
		if got != query {
			t.Fatalf("blocked search read carried %q, want %q", got, query)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("session search read did not reach its barrier")
	}
}

func workspaceSearchClick(t *testing.T, tester *ui.Tester, label string) {
	t.Helper()
	for range 12 {
		viewport, ok := tester.Find("Saved session search results")
		box, found := tester.Find(label)
		if !ok || !found {
			break
		}
		dy := float32(0)
		if box.Y < viewport.Y {
			dy = box.Y - viewport.Y - 8
		} else if box.Y+box.H > viewport.Y+viewport.H {
			dy = box.Y + box.H - viewport.Y - viewport.H + 8
		} else {
			break
		}
		tester.Scroll(viewport.X+viewport.W-12, viewport.Y+20, 0, dy)
	}
	if err := tester.Click(label); err != nil {
		t.Fatal(err)
	}
}

// typeWorkspaceSearchQuery replaces the dialog input through the real control
// and proves the text actually landed before the test submits it.
func typeWorkspaceSearchQuery(t *testing.T, p *NativePage, tester *ui.Tester, value string) {
	t.Helper()
	if err := tester.Click("Find saved session by title or workspace"); err != nil {
		t.Fatal(err)
	}
	tester.Command("selectAll")
	tester.Type(value)
	tester.Frame()
	if p.search.query != value {
		t.Fatalf("dialog input did not take %q, it holds %q", value, p.search.query)
	}
}

func TestWorkspaceSearchDialogCentersAndOpensRealSavedSession(t *testing.T) {
	service, requests := fixture(t)
	// The shared store matches literal title or workspace text; the fixture uses
	// real Chinese words for both, with no pinyin or semantic guessing.
	directory := filepath.Join(t.TempDir(), "支付网关")
	if err := os.MkdirAll(directory, 0o700); err != nil {
		t.Fatal(err)
	}
	login, err := service.Create(t.Context(), "登录问题排查", directory)
	if err != nil {
		t.Fatal(err)
	}
	pipeline, err := service.Create(t.Context(), "构建流水线说明", "")
	if err != nil {
		t.Fatal(err)
	}
	p, tester, store := workspaceSearchPage(t, service, 1240, 850)
	p.model.Draft = "保留这段草稿"
	tester.Frame()
	if err := tester.Click("Search saved sessions"); err != nil {
		t.Fatal(err)
	}
	waitWorkspaceSearch(t, tester, func() bool { return p.search.open && !p.search.loading })
	dialog, ok := tester.Find("Saved session search dialog")
	if !ok || dialog.W != 560 || dialog.X != (1240-560)/2 || abs(float64(dialog.Y-(850-dialog.H)/2)) > 1 ||
		dialog.H > 850-2*workspaceWindowInset || dialog.X < workspaceWindowInset || dialog.X+dialog.W > 1240-workspaceWindowInset {
		t.Fatalf("search dialog is not centered inside the window bounds: dialog=%+v", dialog)
	}
	if _, inline := tester.Find("Find session title/workspace"); inline {
		t.Fatal("old inline sidebar search row is still present")
	}
	for _, session := range []hub.Session{login, pipeline} {
		if _, ok := tester.Find("Session title " + session.ID); !ok {
			t.Fatalf("empty query did not show the real recent session %s: %q", session.ID, tester.Texts())
		}
		if _, ok := tester.Find("Session workspace " + session.ID); !ok {
			t.Fatalf("recent session %s has no directory line", session.ID)
		}
	}
	if !p.search.hasPage || p.search.submitted != "" || len(p.search.sessions) != 2 {
		t.Fatalf("recent session page is not real: %+v %s", p.search, store.seenReports())
	}
	// Title match, entered with Enter on the focused input.
	typeWorkspaceSearchQuery(t, p, tester, "登录")
	if p.search.query != "登录" || p.search.submitted != "" {
		t.Fatalf("typed text did not enter the dialog-local query or leaked into the committed query: %+v", p.search)
	}
	if _, ok := tester.Find("Saved session search not submitted"); !ok {
		t.Fatalf("uncommitted typed text has no truthful state: %q", tester.Texts())
	}
	if _, ok := tester.Find("Session title " + login.ID); ok {
		t.Fatal("uncommitted typed text still showed the previous query's results")
	}
	tester.Key(0, ui.KeyEnter)
	waitWorkspaceSearch(t, tester, func() bool { return !p.search.loading && p.search.submitted == "登录" })
	if len(p.search.sessions) != 1 || p.search.sessions[0].ID != login.ID {
		t.Fatalf("Enter submit did not match the real session title: %+v %s", p.search.sessions, store.seenReports())
	}
	box, ok := tester.Find("Session title " + login.ID)
	if !ok {
		t.Fatalf("results did not follow the committed query: %q", tester.Texts())
	}
	viewport, ok := tester.Find("Saved session search results")
	if !ok || viewport.H <= 0 {
		t.Fatalf("result list has no viewport to scroll in: viewport=%+v", viewport)
	}
	centerX, centerY := box.X+box.W/2, box.Y+box.H/2
	if centerX < viewport.X || centerX > viewport.X+viewport.W || centerY < viewport.Y || centerY > viewport.Y+viewport.H {
		t.Fatalf("rendered result center escaped its viewport: box=%+v viewport=%+v", box, viewport)
	}
	for _, absent := range []string{"Session title " + pipeline.ID, "Session workspace " + pipeline.ID} {
		if _, ok := tester.Find(absent); ok {
			t.Fatalf("the filtered page still shows the non-matching session: %q", absent)
		}
	}
	// Workspace match proves the dialog searches the real directory field too.
	typeWorkspaceSearchQuery(t, p, tester, "支付网关")
	tester.Key(0, ui.KeyEnter)
	waitWorkspaceSearch(t, tester, func() bool { return !p.search.loading && p.search.submitted == "支付网关" })
	if len(p.search.sessions) != 1 || p.search.sessions[0].ID != login.ID {
		t.Fatalf("committed query did not match the saved workspace: %+v %s", p.search.sessions, store.seenReports())
	}
	if p.search.sessions[0].Workspace != login.Workspace {
		t.Fatalf("workspace match did not return the saved directory: want %q got %q", login.Workspace, p.search.sessions[0].Workspace)
	}
	if _, ok := tester.Find("Session workspace " + login.ID); !ok {
		t.Fatal("result row did not render the saved session's directory line")
	}
	workspaceSearchClick(t, tester, "Open saved session "+login.ID)
	waitWorkspaceSearch(t, tester, func() bool { return !p.search.open })
	if p.selected != login.ID || p.search.open {
		t.Fatalf("result click did not navigate then close: selected=%s open=%v", p.selected, p.search.open)
	}
	if p.drafts[""] != "保留这段草稿" || p.model.Draft != p.drafts[login.ID] {
		t.Fatal("opening a search result lost the previous draft", p.drafts, p.model.Draft)
	}
	rows, err := service.Store.ListSessions(t.Context(), 128)
	if err != nil || len(rows) != 2 || service.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("searching or opening a result created a session or executed a model", len(rows), err)
	}
	store.mu.Lock()
	limits := append([]int(nil), store.limits...)
	store.mu.Unlock()
	for _, limit := range limits {
		if limit < 1 || limit > 128 {
			t.Fatalf("search asked the shared store for an unbounded page: %d", limit)
		}
	}
}

func TestWorkspaceSearchPagesForRealAndKeepsSidebarStateIndependent(t *testing.T) {
	service, requests := fixture(t)
	want := make(map[string]bool, 40)
	for index := range 40 {
		session, err := service.Create(t.Context(), pageSessionTitle(index), "")
		if err != nil {
			t.Fatal(err)
		}
		want[session.ID] = true
	}
	p, tester, store := workspaceSearchPage(t, service, 1240, 850)
	p.sessionQuery = "侧栏过滤"
	paneBefore, resourcesBefore, collapsedBefore := p.pane, p.workspaceResourcesVisible(), p.layout.sidebarCollapsed
	if err := tester.Click("Search saved sessions"); err != nil {
		t.Fatal(err)
	}
	waitWorkspaceSearch(t, tester, func() bool { return p.search.open && !p.search.loading })
	if len(p.search.sessions) != workspaceSearchPageSize || !p.search.page.HasMore {
		t.Fatalf("first search page is not a real bounded page: %d hasMore=%v %s", len(p.search.sessions), p.search.page.HasMore, store.seenReports())
	}
	for _, session := range p.search.sessions {
		if !want[session.ID] {
			t.Fatal("search page invented a session", session.ID)
		}
		delete(want, session.ID)
	}
	workspaceSearchClick(t, tester, "Next saved session page")
	waitWorkspaceSearch(t, tester, func() bool { return !p.search.loading })
	if len(p.search.sessions) != 40-workspaceSearchPageSize || p.search.page.HasMore || len(p.search.back) != 1 {
		t.Fatalf("next page did not return the remaining real sessions: %d back=%d", len(p.search.sessions), len(p.search.back))
	}
	for _, session := range p.search.sessions {
		if !want[session.ID] {
			t.Fatal("second search page repeated or invented a session", session.ID)
		}
		delete(want, session.ID)
	}
	if len(want) != 0 {
		t.Fatalf("paging hid real saved sessions: %d missing %s", len(want), store.seenReports())
	}
	if _, ok := tester.Find("Previous saved session page"); !ok {
		t.Fatal("a page with history lost its previous-page control")
	}
	// The last real page must not accept another forward read.
	store.mu.Lock()
	readsBefore := len(store.queries)
	store.mu.Unlock()
	if err := tester.Click("Next saved session page"); err == nil {
		tester.Frame()
	}
	store.mu.Lock()
	readsAfter := len(store.queries)
	store.mu.Unlock()
	if readsAfter != readsBefore || len(p.search.sessions) != 40-workspaceSearchPageSize || p.search.page.HasMore {
		t.Fatalf("the last page accepted another forward read: %s", store.seenReports())
	}
	workspaceSearchClick(t, tester, "Previous saved session page")
	waitWorkspaceSearch(t, tester, func() bool { return !p.search.loading && len(p.search.sessions) == workspaceSearchPageSize })
	workspaceSearchClick(t, tester, "Close session search")
	waitWorkspaceSearch(t, tester, func() bool { return !p.search.open })
	if p.sessionQuery != "侧栏过滤" || p.navigation.query != "" || !p.search.open && p.search.submitted != "" {
		t.Fatalf("search changed the sidebar filter or its own query: %+v navigation=%+v", p.search, p.navigation)
	}
	if p.pane != paneBefore || p.workspaceResourcesVisible() != resourcesBefore || p.layout.sidebarCollapsed != collapsedBefore || p.selected != "" {
		t.Fatalf("opening or closing the search dialog changed page state: pane=%q selected=%q", p.pane, p.selected)
	}
	// The dialog never fed the sidebar's own session page, and the sidebar never
	// received a search read: its recent list is still empty and unfiltered.
	if len(p.sessions) != 0 || p.sessionLoadedQuery != "" || p.sessionLoading || p.sessionQuery != "侧栏过滤" {
		t.Fatalf("search leaked into sidebar session state: sessions=%d loadedQuery=%q", len(p.sessions), p.sessionLoadedQuery)
	}
	store.mu.Lock()
	queries := append([]string(nil), store.queries...)
	store.mu.Unlock()
	for _, query := range queries {
		if query == "侧栏过滤" {
			t.Fatalf("a search read carried the sidebar filter: %s", store.seenReports())
		}
	}
	if p.search.cancel != nil || p.search.loading {
		t.Fatal("closing the dialog left a search read alive")
	}
	if _, shown := tester.Find("Saved session search dialog"); shown {
		t.Fatal("close left the search dialog visible")
	}
	if !tester.HasText("伙伴与群聊") {
		t.Fatal("the sidebar navigation shell changed")
	}
	if service.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("searching executed a model")
	}
}

func pageSessionTitle(index int) string {
	return "分页会话 " + string(rune('A'+index/10)) + string(rune('A'+index%10))
}

func TestWorkspaceSearchReplacedQueryCancelsOldReadAndKeepsCurrentPage(t *testing.T) {
	service, _ := fixture(t)
	for _, title := range []string{"旧查询结果", "新查询结果"} {
		if _, err := service.Create(t.Context(), title, ""); err != nil {
			t.Fatal(err)
		}
	}
	p, tester, store := workspaceSearchPage(t, service, 1240, 850)
	p.search.open = true
	tester.Frame()
	store.block.Store(true)
	p.submitWorkspaceSearch()
	awaitSearchRead(t, store, "")
	tester.Frame()
	if !p.search.loading {
		t.Fatal("blocked search read did not put the dialog into its loading state")
	}
	if _, ok := tester.Find("Saved session search loading"); !ok {
		t.Fatalf("loading state did not speak for itself: %q", tester.Texts())
	}
	p.search.query = "新查询"
	p.submitWorkspaceSearch()
	select {
	case got := <-store.cancelled:
		if got != "" {
			t.Fatalf("cancelled read carried %q, want the replaced empty query", got)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("replacing the query did not cancel the older search read")
	}
	epochAtReplace := p.search.epoch
	store.releaseReads()
	waitWorkspaceSearch(t, tester, func() bool { return !p.search.loading && p.search.submitted == "新查询" })
	if p.search.epoch < epochAtReplace || p.search.loadedQuery != "新查询" || p.search.request.query != "新查询" {
		t.Fatalf("replaced query did not commit its own read: epoch=%d want>=%d %+v", p.search.epoch, epochAtReplace, p.search)
	}
	if len(p.search.sessions) != 1 || p.search.sessions[0].Title != "新查询结果" {
		t.Fatalf("replaced query did not keep its own page: %+v %s", p.search.sessions, store.seenReports())
	}
	for _, session := range p.search.sessions {
		if session.Title == "旧查询结果" {
			t.Fatalf("the cancelled recent page leaked into the current results: %s", session.ID)
		}
	}
	if p.search.cancel != nil || p.search.loading {
		t.Fatal("search read ownership did not settle")
	}
}

func TestWorkspaceSearchEscapeCancelsReadAndCmdKReopenRefusesLateResults(t *testing.T) {
	service, _ := fixture(t)
	stale, err := service.Create(t.Context(), "关闭后不得回盖的结果", "")
	if err != nil {
		t.Fatal(err)
	}
	p, tester, store := workspaceSearchPage(t, service, 1240, 850)
	if err := tester.Click("Search saved sessions"); err != nil {
		t.Fatal(err)
	}
	waitWorkspaceSearch(t, tester, func() bool { return p.search.open && !p.search.loading })
	store.block.Store(true)
	p.search.query = "关闭"
	p.submitWorkspaceSearch()
	awaitSearchRead(t, store, "关闭")
	tester.Frame()
	epochWaiting := p.search.epoch
	tester.Key(0, ui.KeyEscape)
	tester.Frame()
	if p.search.open || p.search.loading || p.search.cancel != nil || p.search.epoch == epochWaiting {
		t.Fatalf("Escape left the dialog, its read, or its epoch unchanged: %+v", p.search)
	}
	if _, shown := tester.Find("Saved session search dialog"); shown {
		t.Fatal("Escape left the search dialog visible")
	}
	committed, loaded, epochClosed := p.search.submitted, p.search.loadedQuery, p.search.epoch
	store.releaseReads()
	for range 6 {
		tester.Frame()
		time.Sleep(5 * time.Millisecond)
	}
	if p.search.open || p.search.loading || p.search.submitted != committed || p.search.loadedQuery != loaded ||
		p.search.epoch != epochClosed || len(p.search.sessions) != 1 || p.search.sessions[0].Title != "关闭后不得回盖的结果" {
		t.Fatalf("a cancelled read returned after Escape: %+v", p.search)
	}
	// Reopen through the real shortcut: the stale page must not be reused as
	// current results, and a fresh committed read must produce them.
	tester.Key(ui.Cmd, ui.KeyK)
	tester.Frame()
	if !p.search.open {
		t.Fatal("Cmd+K did not reopen the search dialog")
	}
	if _, ok := tester.Find("Saved session search dialog"); !ok {
		t.Fatal("reopened dialog is not visible")
	}
	if _, ok := tester.Find("Saved session search not submitted"); !ok {
		t.Fatalf("reopened dialog treated the uncommitted query as current: %+v texts=%q", p.search, tester.Texts())
	}
	tester.Key(0, ui.KeyEnter)
	waitWorkspaceSearch(t, tester, func() bool {
		return !p.search.loading && p.search.submitted == "关闭" && p.search.loadedQuery == "关闭"
	})
	if len(p.search.sessions) != 1 || p.search.sessions[0].Title != "关闭后不得回盖的结果" {
		t.Fatalf("reopened search did not re-read the committed query: %+v %s", p.search, store.seenReports())
	}
	if _, ok := tester.Find("Session title " + stale.ID); !ok {
		t.Fatalf("reopened dialog did not show its fresh result: %q", tester.Texts())
	}
	workspaceSearchClick(t, tester, "Close session search")
	tester.Frame()
	if p.search.open || p.search.cancel != nil || p.selected != "" {
		t.Fatal("reopened search left state behind", p.search, p.selected)
	}
}

func TestWorkspaceSearchReadFailureShowsRealErrorAndRetryRecovers(t *testing.T) {
	service, _ := fixture(t)
	if _, err := service.Create(t.Context(), "重试后应出现的结果", ""); err != nil {
		t.Fatal(err)
	}
	p, tester, store := workspaceSearchPage(t, service, 1240, 850)
	if err := tester.Click("Search saved sessions"); err != nil {
		t.Fatal(err)
	}
	waitWorkspaceSearch(t, tester, func() bool { return p.search.open && !p.search.loading })
	store.fail.Store(true)
	p.search.query = "重试"
	p.submitWorkspaceSearch()
	waitWorkspaceSearch(t, tester, func() bool { return !p.search.loading && p.search.problem != "" })
	if _, ok := tester.Find("Saved session search error"); !ok {
		t.Fatalf("search failure is hidden from the dialog: %q", tester.Texts())
	}
	if p.search.problem != "fixture session search read failed" {
		t.Fatalf("search failure reported %q", p.search.problem)
	}
	if len(p.search.sessions) != 1 || p.search.loadedQuery != "" {
		t.Fatalf("failed search changed the last good page: %+v", p.search)
	}
	if _, ok := tester.Find("Retry saved session search"); !ok {
		t.Fatal("failed search has no retry control")
	}
	workspaceSearchClick(t, tester, "Retry saved session search")
	waitWorkspaceSearch(t, tester, func() bool { return !p.search.loading && p.search.problem == "" })
	if p.search.submitted != "重试" || p.search.loadedQuery != "重试" || len(p.search.sessions) != 1 ||
		p.search.sessions[0].Title != "重试后应出现的结果" {
		t.Fatalf("retry did not re-read the current query: %+v %s", p.search, store.seenReports())
	}
	workspaceSearchClick(t, tester, "Close session search")
	tester.Frame()
	if p.search.open || p.search.cancel != nil {
		t.Fatal("failed then retried search left a read behind")
	}
}

func abs(v float64) float64 {
	if v < 0 {
		return -v
	}
	return v
}

// countQuery reports how many reads the shared store was asked for with exactly
// this query, so one physical Enter is counted instead of assumed.
func (s *workspaceSearchGateStore) countQuery(query string) int {
	s.mu.Lock()
	defer s.mu.Unlock()
	var calls int
	for _, seen := range s.queries {
		if seen == query {
			calls++
		}
	}
	return calls
}

// One Enter on a typed query must reach the shared store once and keep its read
// alive until the store really returns: a submit that replays across the frame
// passes would show up here as a second read plus a cancellation of the held
// one. The same text is then submitted again on purpose, because an explicit
// second Enter is a legitimate single refresh, not a replay.
func TestWorkspaceSearchOneEnterIssuesOneReadWhileHeld(t *testing.T) {
	service, _ := fixture(t)
	directory := filepath.Join(t.TempDir(), "支付网关")
	if err := os.MkdirAll(directory, 0o700); err != nil {
		t.Fatal(err)
	}
	match, err := service.Create(t.Context(), "UI 联调验收", directory)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := service.Create(t.Context(), "构建流水线说明", ""); err != nil {
		t.Fatal(err)
	}
	p, tester, store := workspaceSearchPage(t, service, 1240, 850)
	p.model.Draft = "保留这段草稿"
	tester.Frame()
	if err := tester.Click("Search saved sessions"); err != nil {
		t.Fatal(err)
	}
	waitWorkspaceSearch(t, tester, func() bool { return p.search.open && !p.search.loading })
	const query = "UI 联调验收"
	typeWorkspaceSearchQuery(t, p, tester, query)
	store.block.Store(true)
	tester.Key(0, ui.KeyEnter)
	awaitSearchRead(t, store, query)
	for range 40 {
		tester.Frame()
		if calls := store.countQuery(query); calls != 1 {
			t.Fatalf("one Enter reached the shared store %d times for %q while its read was held: %s", calls, query, store.seenReports())
		}
		select {
		case got := <-store.cancelled:
			t.Fatalf("the held read for %q was cancelled by %q before the store returned", query, got)
		default:
		}
		if !p.search.loading {
			t.Fatalf("the dialog claimed the search settled while the store still held the read: %+v %s", p.search, store.seenReports())
		}
		time.Sleep(5 * time.Millisecond)
	}
	store.releaseReads()
	waitWorkspaceSearch(t, tester, func() bool { return !p.search.loading && p.search.submitted == query })
	if len(p.search.sessions) != 1 || p.search.sessions[0].ID != match.ID {
		t.Fatalf("the held read did not settle on the real saved session: %+v %s", p.search.sessions, store.seenReports())
	}
	if p.model.Draft != "保留这段草稿" || !p.search.open || p.busy {
		t.Fatalf("submitting the search changed unrelated page state: draft=%q page=%+v busy=%v", p.model.Draft, p.search, p.busy)
	}
	tester.Key(0, ui.KeyEnter)
	waitWorkspaceSearch(t, tester, func() bool { return !p.search.loading && store.countQuery(query) == 2 })
	if calls := store.countQuery(query); calls != 2 {
		t.Fatalf("two explicit Enters reached the shared store %d times, want exactly 2: %s", calls, store.seenReports())
	}
	if len(p.search.sessions) != 1 || p.search.sessions[0].ID != match.ID {
		t.Fatalf("the explicit same-query refresh lost the real saved session: %+v %s", p.search.sessions, store.seenReports())
	}
}
