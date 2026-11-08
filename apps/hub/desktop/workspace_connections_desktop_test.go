// INPUT: Actual native MyGo controls, shared connection service and durable SQLite with bounded faults.
// OUTPUT: Explicit-save CRUD, secret redaction, keyboard/draft navigation and truthful failure regressions.
// POS: [L7 Hub UI testing] Uses explicit loopback fixtures; no OS window, account discovery, model generation or paid request.
//go:build desktop

package hubdesktop

import (
	"context"
	"crypto/sha256"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
)

type connectionFaultStore struct {
	hub.ConnectionsStore
	reads, writes      atomic.Int32
	failRead, failSave atomic.Bool
	forgeReceipt       atomic.Bool
	loseReceipt        atomic.Bool
	entered            chan struct{}
	release            chan struct{}
}

func (s *connectionFaultStore) Connections(ctx context.Context) ([]hub.ModelConnection, error) {
	s.reads.Add(1)
	if s.failRead.Load() {
		return nil, errors.New("read unavailable")
	}
	return s.ConnectionsStore.Connections(ctx)
}
func (s *connectionFaultStore) SaveConnection(ctx context.Context, u hub.ConnectionUpdate) (hub.ModelConnection, error) {
	return s.saveConnection(ctx, func() (hub.ModelConnection, error) { return s.ConnectionsStore.SaveConnection(ctx, u) })
}
func (s *connectionFaultStore) CreateConnection(ctx context.Context, request hub.ConnectionCreate) (hub.ModelConnection, error) {
	return s.saveConnection(ctx, func() (hub.ModelConnection, error) {
		return s.ConnectionsStore.(hub.ConnectionCreationStore).CreateConnection(ctx, request)
	})
}
func (s *connectionFaultStore) ConnectionCreation(ctx context.Context, id string) (hub.ConnectionCreationReceipt, error) {
	return s.ConnectionsStore.(hub.ConnectionCreationStore).ConnectionCreation(ctx, id)
}
func (s *connectionFaultStore) saveConnection(ctx context.Context, commit func() (hub.ModelConnection, error)) (hub.ModelConnection, error) {
	s.writes.Add(1)
	if s.entered != nil {
		s.entered <- struct{}{}
		select {
		case <-s.release:
		case <-ctx.Done():
			return hub.ModelConnection{}, ctx.Err()
		}
	}
	if s.failSave.Load() {
		return hub.ModelConnection{}, errors.New("private backend failure")
	}
	entry, err := commit()
	if err == nil && s.loseReceipt.Load() {
		return hub.ModelConnection{}, errors.New("fixture lost committed receipt")
	}
	if err == nil && s.forgeReceipt.Load() {
		entry.Name = "unverified receipt name"
	}
	return entry, err
}

func connectionsFixture(t *testing.T) (*product.HubService, *connectionFaultStore) {
	t.Helper()
	s, _ := fixture(t)
	store := &connectionFaultStore{ConnectionsStore: s.Store.(hub.ConnectionsStore)}
	var err error
	s.Connections, err = product.NewConnectionsService(t.Context(), store)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = s.Connections.Close(context.Background()) })
	return s, store
}

func settleConnections(t *testing.T, p *NativePage, tester *ui.Tester) {
	t.Helper()
	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		tester.Frame()
		if !p.connections.loading && !p.connections.saving {
			return
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("connection storage did not settle")
}

func connectionClick(t *testing.T, tester *ui.Tester, label string) {
	t.Helper()
	if label != "Save connection" && label != "Cancel connection draft" && label != "Back from connection form" && label != "Reload connection draft" && label != "Recover connection creation" {
		for range 12 {
			viewport, ok := tester.Find("Connection form details")
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
	}
	if err := tester.Click(label); err != nil {
		t.Fatal(err)
	}
}
func connectionType(t *testing.T, tester *ui.Tester, label, value string) {
	t.Helper()
	connectionClick(t, tester, label)
	tester.Key(ui.Cmd, ui.KeyA)
	tester.Type(value)
}
func openConnectionTestPage(t *testing.T, p *NativePage, width, height int) *ui.Tester {
	t.Helper()
	p.openPreferences()
	tester := ui.NewTester(workspaceStaticView(p), width, height)
	connectionClick(t, tester, "Models and connections")
	settleConnections(t, p, tester)
	return tester
}
func startOpenAIConnection(t *testing.T, tester *ui.Tester) {
	t.Helper()
	connectionClick(t, tester, "添加连接")
	connectionClick(t, tester, "Add OpenAI connection")
}

func TestNativeConnectionsExplicitSaveRoutesAndSecretRedaction(t *testing.T) {
	s, store := connectionsFixture(t)
	p := NewNativePage(s)
	p.model.Draft = "保留任务草稿"
	tester := openConnectionTestPage(t, p, 1240, 850)
	startOpenAIConnection(t, tester)
	if tester.HasText("完整请求地址") || !tester.HasText("测试连接") || !tester.HasText("获取模型") || tester.HasText("本机 Agent 账户") {
		t.Fatal("preset expanded by default or did not expose explicit read-only catalog requests")
	}
	connectionType(t, tester, "Edit API Key", "dummy-key-never-a-real-credential")
	connectionType(t, tester, "Edit 模型 ID", "exact/model-ID")
	tester.Key(0, ui.KeyEnter)
	if len(p.connections.draft.Models) != 1 || p.connections.modelID != "" {
		t.Fatal("Enter did not add exactly one draft model")
	}
	connectionType(t, tester, "Edit 模型 ID", "exact/model-ID")
	connectionClick(t, tester, "Add model to connection draft")
	if len(p.connections.draft.Models) != 1 {
		t.Fatal("duplicate model added")
	}
	connectionClick(t, tester, "Enable connection draft")
	if store.writes.Load() != 0 || len(s.Connections.Targets()) != 0 || tester.HasText("dummy-key-never-a-real-credential") {
		t.Fatal("draft wrote configuration, enabled routes or exposed a credential")
	}
	connectionClick(t, tester, "Save connection")
	settleConnections(t, p, tester)
	entries := s.Connections.Snapshot()
	if len(entries) != 1 || entries[0].Revision != 1 || p.connections.mode != "" || len(s.Connections.Targets()) != 1 || !entries[0].HasAPIKey {
		t.Fatal("explicit save did not commit connection/model route")
	}
	id := entries[0].ID
	for _, label := range []string{"Connection OpenAI", "Edit connection"} {
		r, ok := tester.Find(label)
		manager, managerOK := tester.Find("Connection manager")
		if !ok || !managerOK || r.Y < manager.Y || r.Y+r.H > manager.Y+manager.H {
			t.Fatalf("connection manager content escaped its region: %s %+v", label, r)
		}
	}
	if p.selected != "" || p.model.Draft != "保留任务草稿" || s.Runtime.HasActive() {
		t.Fatal("settings created execution or changed the workspace draft")
	}
	connectionClick(t, tester, "Back to workspace")
	connectionClick(t, tester, "Agent and model")
	connectionClick(t, tester, "Use alva · exact/model-ID")
	if hub.RouteKey(p.workspaceComposerTarget()) != hub.RouteKey(s.Connections.Targets()[0]) || p.model.Draft != "保留任务草稿" || s.Runtime.HasActive() {
		t.Fatal("saved route did not become an explicit composer choice")
	}
	p.openConnections()
	settleConnections(t, p, tester)
	connectionClick(t, tester, "Edit connection")
	if p.connections.draft.ID != id || p.connections.draft.APIKey != "" {
		t.Fatal("edit lost identity or read a saved secret into its draft")
	}
	connectionClick(t, tester, "Advanced connection headers")
	connectionClick(t, tester, "Replace saved headers")
	connectionClick(t, tester, "Add header")
	connectionType(t, tester, "Header name 1", "X-Private-Token")
	connectionType(t, tester, "Header value 1", "dummy-header-secret")
	if tester.HasText("dummy-header-secret") {
		t.Fatal("header editor exposed secret text")
	}
	connectionClick(t, tester, "Save connection")
	settleConnections(t, p, tester)
	entry := s.Connections.Snapshot()[0]
	secret, err := store.ConnectionSecrets(t.Context(), id, entry.Revision)
	if err != nil || secret.APIKey != "dummy-key-never-a-real-credential" || secret.Headers["X-Private-Token"] != "dummy-header-secret" || entry.ID != id || len(entry.HeaderNames) != 1 {
		t.Fatal("edit did not preserve identity/key or save explicit headers")
	}
	for _, text := range tester.Texts() {
		if strings.Contains(text, "dummy-") {
			t.Fatal("management list exposed a credential")
		}
	}
	connectionClick(t, tester, "Edit connection")
	connectionClick(t, tester, "Clear saved API Key")
	connectionClick(t, tester, "Save connection")
	settleConnections(t, p, tester)
	entry = s.Connections.Snapshot()[0]
	secret, err = store.ConnectionSecrets(t.Context(), id, entry.Revision)
	if err != nil || entry.HasAPIKey || secret.APIKey != "" || entry.Auth != "none" {
		t.Fatal("clearing the saved key did not leave a keyless connection")
	}
	connectionClick(t, tester, "Toggle connection enabled")
	settleConnections(t, p, tester)
	if len(s.Connections.Targets()) != 0 || s.Connections.Snapshot()[0].Enabled {
		t.Fatal("explicit disable did not remove future routes")
	}
	connectionClick(t, tester, "Delete connection")
	if len(s.Connections.Snapshot()) != 1 {
		t.Fatal("delete review deleted early")
	}
	connectionClick(t, tester, "Cancel delete connection")
	connectionClick(t, tester, "Delete connection")
	connectionClick(t, tester, "Confirm delete connection")
	settleConnections(t, p, tester)
	if len(s.Connections.Snapshot()) != 0 {
		t.Fatal("confirmed delete did not remove the connection")
	}
}

func TestNativeConnectionsSaveWithoutAPIKey(t *testing.T) {
	s, store := connectionsFixture(t)
	p := NewNativePage(s)
	tester := openConnectionTestPage(t, p, 1240, 850)
	startOpenAIConnection(t, tester)
	if !tester.HasText("API Key（可选）") || tester.HasText("认证方式") {
		t.Fatal("the form still asks for an authentication mode instead of an optional key")
	}
	connectionType(t, tester, "Edit 模型 ID", "exact/model-ID")
	connectionClick(t, tester, "Add model to connection draft")
	connectionClick(t, tester, "Enable connection draft")
	connectionClick(t, tester, "Save connection")
	settleConnections(t, p, tester)
	entries := s.Connections.Snapshot()
	if len(entries) != 1 || entries[0].Auth != "none" || entries[0].HasAPIKey || len(s.Connections.Targets()) != 1 {
		t.Fatalf("a connection without a key was not saved as a usable route: %+v", entries)
	}
	secret, err := store.ConnectionSecrets(t.Context(), entries[0].ID, entries[0].Revision)
	if err != nil || secret.APIKey != "" {
		t.Fatal("a keyless connection stored a credential", err)
	}
}

func TestNativeConnectionsBackCancelFailureConflictAndColdBuild(t *testing.T) {
	s, store := connectionsFixture(t)
	p := NewNativePage(s)
	p.pane = "inspector"
	p.inspectCursor = 37
	tester := openConnectionTestPage(t, p, 1000, 720)
	startOpenAIConnection(t, tester)
	connectionType(t, tester, "Edit API Key", "dummy-draft-key")
	connectionClick(t, tester, "Back from connection form")
	connectionClick(t, tester, "Add OpenAI connection")
	if p.connections.draft.APIKey != "dummy-draft-key" {
		t.Fatal("source Back discarded the draft")
	}
	connectionClick(t, tester, "Save connection")
	settleConnections(t, p, tester)
	connectionClick(t, tester, "Edit connection")
	connectionType(t, tester, "Edit 连接名称", "retained edit")
	reads := store.reads.Load()
	for i := 0; i < 3; i++ {
		tester.Frame()
		p.poll()
	}
	if store.reads.Load() != reads {
		t.Fatal("Build or observation read storage")
	}
	store.failSave.Store(true)
	connectionClick(t, tester, "Save connection")
	settleConnections(t, p, tester)
	if p.connections.draft.Name != "retained edit" || p.connections.mode != "form" || !tester.HasText("草稿已保留") || s.Connections.Snapshot()[0].Name != "OpenAI" || tester.HasText("private backend failure") {
		t.Fatal("failed save lost a draft, leaked backend detail or claimed success")
	}
	store.failSave.Store(false)
	entry := s.Connections.Snapshot()[0]
	if _, err := s.Connections.SetEnabled(t.Context(), entry.ID, entry.Revision, true); err != nil {
		t.Fatal(err)
	}
	connectionClick(t, tester, "Save connection")
	settleConnections(t, p, tester)
	if !tester.HasText("其他窗口") || p.connections.draft.Name != "retained edit" {
		t.Fatal("conflict overwrote the current draft")
	}
	connectionClick(t, tester, "Reload connection draft")
	settleConnections(t, p, tester)
	if p.connections.draft.Name != "OpenAI" || p.connections.draft.ExpectedRevision != 2 {
		t.Fatal("explicit reload did not load committed revision")
	}
	connectionType(t, tester, "Edit API Key", "dummy-unsaved-key")
	connectionClick(t, tester, "Cancel connection draft")
	if p.connections.draft.APIKey != "" || len(p.connections.headers) != 0 || p.connections.mode != "" {
		t.Fatal("Cancel retained secret fields")
	}
	connectionClick(t, tester, "Back to workspace")
	if p.pane != "inspector" || p.inspectCursor != 37 {
		t.Fatal("settings return lost the original Inspector")
	}
	if s.Runtime.HasActive() {
		t.Fatal("settings executed a model")
	}
}

func TestNativeConnectionsInvalidReadAndActualSaveCloseGuard(t *testing.T) {
	s, store := connectionsFixture(t)
	p := NewNativePage(s)
	tester := openConnectionTestPage(t, p, 1240, 850)
	store.failRead.Store(true)
	connectionClick(t, tester, "刷新连接")
	settleConnections(t, p, tester)
	connectionClick(t, tester, "添加连接")
	if p.connections.mode != "" || !p.connections.readFailed {
		t.Fatal("failed read allowed configuration writes")
	}
	store.failRead.Store(false)
	connectionClick(t, tester, "刷新连接")
	settleConnections(t, p, tester)
	startOpenAIConnection(t, tester)
	connectionType(t, tester, "Edit API Key", "dummy-close-key")
	store.entered, store.release = make(chan struct{}, 1), make(chan struct{})
	connectionClick(t, tester, "Save connection")
	select {
	case <-store.entered:
	case <-time.After(time.Second):
		t.Fatal("save did not reach storage")
	}
	event := new(mygo.CloseEvent)
	if !p.guardConnectionsClose(event) || !event.DefaultPrevented() || !p.connections.saving || len(s.Connections.Snapshot()) != 0 {
		t.Fatal("close discarded a pending save owner or claimed it committed")
	}
	close(store.release)
	settleConnections(t, p, tester)
	if p.connections.saving || len(s.Connections.Snapshot()) != 1 || p.guardConnectionsClose(new(mygo.CloseEvent)) {
		t.Fatal("save completion did not release close guard")
	}
}

func TestNativeConnectionsNativeChoicesValidationAndViewport(t *testing.T) {
	for _, size := range []struct{ width, height int }{{1240, 850}, {1000, 720}, {960, 680}} {
		s, store := connectionsFixture(t)
		p := NewNativePage(s)
		tester := openConnectionTestPage(t, p, size.width, size.height)
		connectionClick(t, tester, "添加连接")
		connectionClick(t, tester, "Add Local connection")
		if !tester.HasText("API Key（可选）") || p.connections.draft.Auth != "none" || p.connections.draft.Endpoint != "" {
			t.Fatal("local connection invented an endpoint or did not offer the key as optional")
		}
		connectionClick(t, tester, "Connection protocol")
		connectionClick(t, tester, "Anthropic Messages")
		if p.connections.draft.Protocol != "anthropic" {
			t.Fatal("native protocol choice did not edit the draft")
		}
		connectionClick(t, tester, "Connection protocol")
		tester.Key(0, ui.KeyEscape)
		if p.connections.mode != "form" {
			t.Fatal("Escape from a select closed its form")
		}
		for _, label := range []string{"Save connection", "Cancel connection draft", "Back from connection form"} {
			r, ok := tester.Find(label)
			if !ok || r.X < workspaceWindowInset || r.Y < 0 || r.X+r.W > float32(size.width)-workspaceWindowInset || r.Y+r.H > float32(size.height)-workspaceWindowInset {
				t.Fatalf("%s outside the connection viewport: %+v", label, r)
			}
		}
		connectionType(t, tester, "Edit 完整请求地址", "http://remote.example/v1/messages?debug=1")
		connectionClick(t, tester, "Save connection")
		settleConnections(t, p, tester)
		if p.connections.mode != "form" || p.connections.draft.Endpoint != "http://remote.example/v1/messages?debug=1" || !tester.HasText("草稿已保留") || store.writes.Load() != 0 {
			t.Fatal("invalid endpoint reached storage or discarded the draft")
		}
		// Plain HTTP to a non-loopback host is allowed (LAN and self-hosted gateways).
		connectionType(t, tester, "Edit 完整请求地址", "http://remote.example/v1/messages")
		connectionClick(t, tester, "Save connection")
		settleConnections(t, p, tester)
		if saved := s.Connections.Snapshot(); len(saved) != 1 || saved[0].Endpoint != "http://remote.example/v1/messages" || len(s.Connections.Targets()) != 0 || s.Runtime.HasActive() {
			t.Fatal("empty-model remote HTTP connection could not save or fabricated a usable model", p.connections.problem)
		}
	}
}

func settleConnectionProbe(t *testing.T, p *NativePage, tester *ui.Tester) {
	t.Helper()
	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		tester.Frame()
		if !p.connections.requestBusy {
			return
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("connection request did not finish actual cleanup")
}
func startLocalCatalogForm(t *testing.T, p *NativePage, tester *ui.Tester, address string) {
	t.Helper()
	connectionClick(t, tester, "添加连接")
	connectionClick(t, tester, "Add Local connection")
	connectionType(t, tester, "Edit 完整请求地址", address+"/chat/completions")
	connectionClick(t, tester, "Advanced connection headers")
	connectionType(t, tester, "Edit 模型目录地址（可选，同源）", address+"/models")
}
func TestNativeConnectionCatalogRealClickSixStatesAndManualMerge(t *testing.T) {
	var calls atomic.Int32
	var mode atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls.Add(1)
		if r.Method != "GET" || r.URL.Path != "/models" || r.Header.Get("Authorization") != "" {
			t.Error("UI request changed explicit source/method/auth")
		}
		switch mode.Load() {
		case 1:
			w.WriteHeader(401)
			fmt.Fprint(w, "secret-response-never-shown")
		case 2:
			fmt.Fprint(w, `{"data":[]}`)
		case 3:
			w.WriteHeader(404)
		case 4:
			fmt.Fprint(w, `{"data":[`)
		default:
			fmt.Fprint(w, `{"data":[{"id":"manual-ID"},{"id":"candidate-ID"},{"id":"third-ID"}`)
			for i := 0; i < 8; i++ {
				fmt.Fprintf(w, `,{"id":"extra-%02d"}`, i)
			}
			fmt.Fprint(w, `]}`)
		}
	}))
	defer server.Close()
	s, store := connectionsFixture(t)
	p := NewNativePage(s)
	tester := openConnectionTestPage(t, p, 960, 680)
	startLocalCatalogForm(t, p, tester, server.URL)
	if p.connections.catalogState != "idle" || calls.Load() != 0 || store.writes.Load() != 0 {
		t.Fatal("cold form performed network/storage or lacked Idle")
	}
	connectionType(t, tester, "Edit 模型 ID", "manual-ID")
	tester.Key(0, ui.KeyEnter)
	connectionClick(t, tester, "Fetch connection models")
	settleConnectionProbe(t, p, tester)
	if p.connections.catalogState != "ready" || len(p.connections.catalogModels) != 11 || len(p.connections.draft.Models) != 1 {
		t.Fatal("fetch did not produce candidates without selection")
	}
	connectionClick(t, tester, "Next model catalog page")
	if _, found := tester.Find("Add catalog model extra-07"); !found {
		t.Fatal("long catalog next page did not expose later candidates")
	}
	connectionClick(t, tester, "Previous model catalog page")
	connectionType(t, tester, "Search fetched models", "candidate")
	if !tester.HasText("candidate-ID") || tester.HasText("third-ID") {
		t.Fatal("catalog query did not filter exact results")
	}
	// Select a fetched candidate with the actual native button.
	connectionClick(t, tester, "Add catalog model candidate-ID")
	if len(p.connections.draft.Models) != 2 || p.connections.draft.Models[1].Enabled {
		t.Fatal("candidate silently enabled or did not merge")
	}
	connectionType(t, tester, "Search fetched models", "manual")
	connectionClick(t, tester, "Add catalog model manual-ID")
	if len(p.connections.draft.Models) != 2 || !p.connections.draft.Models[0].Enabled {
		t.Fatal("catalog selection duplicated or changed manual model")
	}
	connectionClick(t, tester, "Test connection catalog access")
	settleConnectionProbe(t, p, tester)
	if p.connections.testState != "ready" || !tester.HasText("生成与 Agent 兼容性尚未验证") {
		t.Fatal("test result misstated validation scope")
	}
	for _, tc := range []struct {
		mode  int32
		state string
	}{{1, "error"}, {2, "empty"}, {3, "unavailable"}, {4, "error"}} {
		mode.Store(tc.mode)
		connectionClick(t, tester, "Fetch connection models")
		settleConnectionProbe(t, p, tester)
		if p.connections.catalogState != tc.state || len(p.connections.draft.Models) != 2 || tester.HasText("secret-response-never-shown") {
			t.Fatal("catalog state conflated or draft lost", tc.state, p.connections.catalogState)
		}
	}
	if store.writes.Load() != 0 || s.Runtime.HasActive() {
		t.Fatal("catalog/test created execution or saved config")
	}
	for _, label := range []string{"Save connection", "Cancel connection draft", "Back from connection form"} {
		box, ok := tester.Find(label)
		if !ok || box.Y < 0 || box.Y+box.H > 680 {
			t.Fatal("fixed form action unreachable at minimum size", label, box)
		}
	}
}
func TestNativeConnectionCatalogLoadingCancelSourceEditLateAndClose(t *testing.T) {
	entered := make(chan struct{}, 4)
	release := make(chan struct{})
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		entered <- struct{}{}
		select {
		case <-release:
			fmt.Fprint(w, `{"data":[{"id":"obsolete-model"}]}`)
		case <-r.Context().Done():
		}
	}))
	defer server.Close()
	s, _ := connectionsFixture(t)
	p := NewNativePage(s)
	tester := openConnectionTestPage(t, p, 960, 680)
	startLocalCatalogForm(t, p, tester, server.URL)
	connectionClick(t, tester, "Fetch connection models")
	select {
	case <-entered:
	case <-time.After(time.Second):
		t.Fatal("explicit fetch did not reach server")
	}
	if !p.connections.requestBusy || p.connections.catalogState != "loading" {
		t.Fatal("actual Loading missing")
	}
	connectionType(t, tester, "Edit 模型 ID", "typed-while-loading")
	tester.Key(0, ui.KeyEnter)
	if len(p.connections.draft.Models) != 1 {
		t.Fatal("loading blocked manual entry")
	}
	connectionType(t, tester, "Edit 完整请求地址", server.URL+"/new-source")
	settleConnectionProbe(t, p, tester)
	if p.connections.catalogState != "idle" || len(p.connections.catalogModels) != 0 || p.connections.draft.Endpoint != server.URL+"/new-source" || len(p.connections.draft.Models) != 1 {
		t.Fatal("late request overwrote edited source/manual draft")
	}
	connectionClick(t, tester, "Fetch connection models")
	select {
	case <-entered:
	case <-time.After(time.Second):
		t.Fatal("request did not reach explicit catalog")
	}
	connectionClick(t, tester, "Cancel connection request")
	settleConnectionProbe(t, p, tester)
	if p.connections.catalogState != "idle" || !tester.HasText("请求已取消") {
		t.Fatal("explicit cancellation not truthful")
	}
	connectionClick(t, tester, "Fetch connection models")
	select {
	case <-entered:
	case <-time.After(time.Second):
		t.Fatal("request did not reach explicit catalog")
	}
	event := new(mygo.CloseEvent)
	if !p.guardConnectionsClose(event) || !event.DefaultPrevented() || !p.connections.requestBusy {
		t.Fatal("Close failed to retain pending request owner")
	}
	settleConnectionProbe(t, p, tester)
	if p.guardConnectionsClose(new(mygo.CloseEvent)) {
		t.Fatal("finished cleanup retained close guard")
	}
	connectionClick(t, tester, "Fetch connection models")
	select {
	case <-entered:
	case <-time.After(time.Second):
		t.Fatal("request did not reach explicit catalog")
	}
	connectionClick(t, tester, "Back from connection form")
	connectionClick(t, tester, "Add OpenAI connection")
	close(release)
	settleConnectionProbe(t, p, tester)
	if p.connections.source != "OpenAI" || p.connections.draft.Endpoint != "https://api.openai.com/v1/responses" || len(p.connections.catalogModels) != 0 || p.connections.testState == "ready" {
		t.Fatal("old source completion contaminated replacement draft")
	}
	connectionClick(t, tester, "Cancel connection draft")
	if p.connections.draft.APIKey != "" || p.connections.requestBusy {
		t.Fatal("cancel failed to clear secret draft/finish request")
	}
}

func TestNativeConnectionCatalogRetryKeepsItsActionWhileRequestIsInFlight(t *testing.T) {
	entered, release := make(chan struct{}, 2), make(chan struct{})
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if calls.Add(1) == 1 {
			http.Error(w, "temporary fixture failure", http.StatusServiceUnavailable)
			return
		}
		entered <- struct{}{}
		select {
		case <-release:
			fmt.Fprint(w, `{"data":[{"id":"recovered-model"}]}`)
		case <-r.Context().Done():
		}
	}))
	defer server.Close()
	defer close(release)
	service, _ := connectionsFixture(t)
	p := NewNativePage(service)
	tester := openConnectionTestPage(t, p, 960, 680)
	startLocalCatalogForm(t, p, tester, server.URL)
	connectionClick(t, tester, "Fetch connection models")
	settleConnectionProbe(t, p, tester)
	if p.connections.catalogState != "error" || !tester.HasText("重试") {
		t.Fatal("catalog fixture did not reach its retry state")
	}
	before, _ := tester.Find("Fetch connection models")
	connectionClick(t, tester, "Fetch connection models")
	select {
	case <-entered:
	case <-time.After(time.Second):
		t.Fatal("explicit retry did not reach the local catalog")
	}
	if after, ok := tester.Find("Fetch connection models"); !ok || before != after || !tester.HasText("重试") || !tester.Focused("Fetch connection models") {
		t.Fatal("in-flight retry changed its action name, width or focus", before, after)
	}
	connectionClick(t, tester, "Fetch connection models")
	tester.Key(0, ui.KeyEnter)
	if calls.Load() != 2 || !p.connections.requestBusy {
		t.Fatal("busy retry dispatched another request", calls.Load())
	}
	connectionClick(t, tester, "Cancel connection request")
	settleConnectionProbe(t, p, tester)
	if p.connections.catalogState != "idle" || !tester.HasText("请求已取消") {
		t.Fatal("retry cancellation did not retain its actual cleanup result")
	}
}

func TestNativeConnectionsFoundationGeometry(t *testing.T) {
	s, _ := connectionsFixture(t)
	_, err := s.Connections.Save(t.Context(), hub.ConnectionUpdate{Name: "OpenAI", Protocol: "openai_responses", Auth: "none", Endpoint: "https://api.openai.com/v1/responses", Models: []hub.ConnectionModel{{ID: "test-model", Name: "Test model"}}})
	if err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(s)
	tester := openConnectionTestPage(t, p, 1240, 850)
	list, listOK := tester.Find("Connection list")
	models, modelsOK := tester.Find("Connection models")
	row, rowOK := tester.Find("Connection OpenAI")
	if !listOK || !modelsOK || !rowOK || list.W != 208 || models.X-list.X-list.W != 12 || row.H != 56 {
		t.Fatalf("Foundation manager layout: list=%+v models=%+v row=%+v", list, models, row)
	}
	startOpenAIConnection(t, tester)
	dialog, dialogOK := tester.Find("Connection form dialog")
	footer, footerOK := tester.Find("Connection form footer")
	cancel, cancelOK := tester.Find("Cancel connection draft")
	save, saveOK := tester.Find("Save connection")
	key, keyOK := tester.Find("Edit API Key")
	password, passwordOK := tester.Find("API Key password control")
	leading, leadingOK := tester.Find("API Key leading icon")
	reveal, revealOK := tester.Find("API Key reveal icon")
	fetch, fetchOK := tester.Find("Fetch connection models")
	if !dialogOK || !footerOK || !cancelOK || !saveOK || !keyOK || dialog.W != 800 || footer.W != 752 || footer.H != 52 || key.H != 32 || cancel.H != 32 || save.H != 32 || save.X-cancel.X-cancel.W != 8 || footer.X+footer.W != save.X+save.W {
		t.Fatalf("Foundation dialog controls: dialog=%+v footer=%+v key=%+v cancel=%+v save=%+v", dialog, footer, key, cancel, save)
	}
	if !passwordOK || !leadingOK || !revealOK || !fetchOK || password.W != 752 || leading.W != 14 || leading.H != 14 || reveal.W != 14 || reveal.H != 14 || leading.X-password.X != 8 || key.X-leading.X-leading.W != 8 || password.X+password.W-reveal.X-reveal.W != 8 || fetch.X+fetch.W != footer.X+footer.W {
		t.Fatalf("Foundation Password slots or model action alignment: control=%+v leading=%+v input=%+v reveal=%+v fetch=%+v", password, leading, key, reveal, fetch)
	}
	if tester.HasText("模型目录地址（可选，同源）") || !tester.HasText("api.openai.com") || !tester.HasText("返回选择供应商") {
		t.Fatal("preset hierarchy expanded optional details or lost canonical source/header")
	}
}

func TestNativeConnectionsKeyVisibilityOnlyRevealsCurrentDraft(t *testing.T) {
	s, store := connectionsFixture(t)
	p := NewNativePage(s)
	tester := openConnectionTestPage(t, p, 1240, 850)
	startOpenAIConnection(t, tester)
	const key = "dummy-visible-only-after-explicit-action"
	connectionType(t, tester, "Edit API Key", key)
	if p.connections.revealAPIKey || tester.HasText(key) {
		t.Fatal("new key draft was not masked by default")
	}
	connectionClick(t, tester, "Toggle API Key visibility")
	tester.Frame()
	tester.Frame()
	if !p.connections.revealAPIKey || p.connections.draft.APIKey != key {
		t.Fatal("explicit reveal did not retain the current draft")
	}
	revealed := connectionInputRendering(t, tester, "Edit API Key")
	connectionClick(t, tester, "Toggle API Key visibility")
	tester.Frame()
	tester.Frame()
	if p.connections.revealAPIKey || tester.HasText(key) || connectionInputRendering(t, tester, "Edit API Key") == revealed {
		t.Fatal("explicit hide did not mask the draft")
	}
	connectionClick(t, tester, "Toggle API Key visibility")
	connectionClick(t, tester, "Back from connection form")
	connectionClick(t, tester, "Add OpenAI connection")
	if p.connections.revealAPIKey || tester.HasText(key) || p.connections.draft.APIKey != key {
		t.Fatal("returning through sources exposed or discarded the retained key draft")
	}
	connectionClick(t, tester, "Toggle API Key visibility")
	p.openPreferences()
	tester.Frame()
	p.openConnections()
	tester.Frame()
	if p.connections.revealAPIKey || tester.HasText(key) || p.connections.draft.APIKey != key {
		t.Fatal("leaving the connection page retained reveal or discarded its draft")
	}
	connectionClick(t, tester, "Toggle API Key visibility")
	store.failSave.Store(true)
	connectionClick(t, tester, "Save connection")
	settleConnections(t, p, tester)
	if p.connections.revealAPIKey || tester.HasText(key) || p.connections.draft.APIKey != key {
		t.Fatal("failed save retained reveal or lost the editable draft")
	}
	store.failSave.Store(false)
	connectionClick(t, tester, "Save connection")
	settleConnections(t, p, tester)
	connectionClick(t, tester, "Edit connection")
	connectionClick(t, tester, "Toggle API Key visibility")
	if p.connections.draft.APIKey != "" || tester.HasText(key) {
		t.Fatal("reveal read a saved credential into the empty edit draft")
	}
	connectionClick(t, tester, "Cancel connection draft")
	if p.connections.revealAPIKey || p.connections.draft.APIKey != "" {
		t.Fatal("cancel retained reveal or the key draft")
	}
}

// Tester.Texts lists element labels, so it cannot verify a password's
// displayed value. Compare only the rendered input interior for reveal/hide.
func connectionInputRendering(t *testing.T, tester *ui.Tester, label string) [32]byte {
	t.Helper()
	r, ok := tester.Find(label)
	if !ok {
		t.Fatal("missing password input")
	}
	img := tester.Image()
	var pixels []byte
	for y := int(r.Y) + 2; y < int(r.Y+r.H)-2; y++ {
		for x := int(r.X) + 2; x < int(r.X+r.W)-2; x++ {
			pixel := img.RGBAAt(x, y)
			pixels = append(pixels, pixel.R, pixel.G, pixel.B, pixel.A)
		}
	}
	return sha256.Sum256(pixels)
}

func TestNativeConnectionsDeleteReviewCannotFollowAnotherWindowsEdit(t *testing.T) {
	s, _ := connectionsFixture(t)
	entry, err := s.Connections.Save(t.Context(), hub.ConnectionUpdate{Name: "reviewed connection", Protocol: "openai_chat", Endpoint: "http://127.0.0.1:1/chat", Auth: "none"})
	if err != nil {
		t.Fatal(err)
	}
	reviewer, editor := NewNativePage(s), NewNativePage(s)
	reviewUI := openConnectionTestPage(t, reviewer, 1240, 850)
	editUI := openConnectionTestPage(t, editor, 1240, 850)
	connectionClick(t, reviewUI, "Delete connection")
	connectionClick(t, editUI, "Edit connection")
	connectionType(t, editUI, "Edit 连接名称", "changed after delete review")
	connectionClick(t, editUI, "Save connection")
	settleConnections(t, editor, editUI)
	reviewUI.Frame()
	if reviewer.connections.deleteReview || !reviewUI.HasText("重新审阅") {
		t.Fatal("another window's edit retained an approval to delete the newer revision")
	}
	if _, ok := reviewUI.Find("Confirm delete connection"); ok {
		t.Fatal("stale delete confirmation remained actionable")
	}
	current, ok := s.Connections.Connection(entry.ID)
	if !ok || current.Revision != entry.Revision+1 || current.Name != "changed after delete review" {
		t.Fatal("review invalidation modified the other window's committed connection")
	}
	connectionClick(t, reviewUI, "Delete connection")
	connectionClick(t, reviewUI, "Confirm delete connection")
	settleConnections(t, reviewer, reviewUI)
	if len(s.Connections.Snapshot()) != 0 || s.Runtime.HasActive() {
		t.Fatal("a fresh review could not delete exactly its current revision")
	}
}

func TestNativeConnectionsDeletedEditKeepsItsOwnDraftAndReloadExplainsAbsence(t *testing.T) {
	s, store := connectionsFixture(t)
	entry, err := s.Connections.Save(t.Context(), hub.ConnectionUpdate{Name: "editing connection", Protocol: "openai_chat", Endpoint: "http://127.0.0.1:1/chat", Auth: "none"})
	if err != nil {
		t.Fatal(err)
	}
	_, err = s.Connections.Save(t.Context(), hub.ConnectionUpdate{Name: "other account", Protocol: "openai_chat", Endpoint: "http://127.0.0.1:2/chat", Auth: "api_key", APIKey: "dummy-other-account-key", ReplaceHeaders: true, Headers: map[string]string{"X-Other-Account": "dummy-other-header"}})
	if err != nil {
		t.Fatal(err)
	}
	p, remover := NewNativePage(s), NewNativePage(s)
	tester := openConnectionTestPage(t, p, 960, 680)
	removeUI := openConnectionTestPage(t, remover, 960, 680)
	connectionClick(t, tester, "Connection editing connection")
	connectionClick(t, tester, "Edit connection")
	connectionType(t, tester, "Edit API Key", "dummy-unsaved-edit")
	connectionClick(t, tester, "Advanced connection headers")
	connectionClick(t, removeUI, "Connection editing connection")
	connectionClick(t, removeUI, "Delete connection")
	connectionClick(t, removeUI, "Confirm delete connection")
	settleConnections(t, remover, removeUI)
	tester.Frame()
	if _, found := tester.Find("Clear saved API Key"); found || tester.HasText("已保存：X-Other-Account") {
		t.Fatal("deleted connection's draft borrowed another account's saved secret metadata")
	}
	connectionClick(t, tester, "Reload connection draft")
	settleConnections(t, p, tester)
	if !tester.HasText("已被删除") || p.connections.draft.ID != entry.ID || p.connections.draft.APIKey != "dummy-unsaved-edit" {
		t.Fatal("reload silently retained a missing edit or discarded its recoverable draft")
	}
	for _, label := range []string{"Save connection", "Cancel connection draft", "Reload connection draft"} {
		bounds, ok := tester.Find(label)
		if !ok || bounds.Y < 24 || bounds.Y+bounds.H > 656 {
			t.Fatalf("missing-connection error moved %s outside the viewport: %+v", label, bounds)
		}
	}
	writes := store.writes.Load()
	connectionClick(t, tester, "Save connection")
	if p.connections.saving || store.writes.Load() != writes {
		t.Fatal("known deleted edit accepted another save")
	}
	connectionClick(t, tester, "Cancel connection draft")
	if p.connections.draft.APIKey != "" || !tester.HasText("other account") || s.Runtime.HasActive() {
		t.Fatal("return from missing edit blocked current connections or retained its secret draft")
	}
}

func TestNativeConnectionsUnverifiedSaveKeepsEditUntilExplicitReload(t *testing.T) {
	s, store := connectionsFixture(t)
	entry, err := s.Connections.Save(t.Context(), hub.ConnectionUpdate{Name: "before edit", Protocol: "openai_chat", Endpoint: "http://127.0.0.1:1/chat", Auth: "none"})
	if err != nil {
		t.Fatal(err)
	}
	p := NewNativePage(s)
	tester := openConnectionTestPage(t, p, 1240, 850)
	connectionClick(t, tester, "Edit connection")
	connectionType(t, tester, "Edit 连接名称", "actual committed edit")
	store.forgeReceipt.Store(true)
	connectionClick(t, tester, "Save connection")
	settleConnections(t, p, tester)
	if p.connections.mode != "form" || p.connections.draft.Name != "actual committed edit" || !tester.HasText("草稿已保留") || !tester.HasText("reload before retrying") {
		t.Fatal("an unverified save receipt cleared the draft or claimed success")
	}
	if current, _ := s.Connections.Connection(entry.ID); current.Revision != entry.Revision || current.Name != "before edit" {
		t.Fatal("unverified receipt was published as committed configuration")
	}
	store.forgeReceipt.Store(false)
	connectionClick(t, tester, "Reload connection draft")
	settleConnections(t, p, tester)
	if p.connections.draft.ExpectedRevision != entry.Revision+1 || p.connections.draft.Name != "actual committed edit" {
		t.Fatal("explicit reload did not reconcile the actual committed result")
	}
	connectionType(t, tester, "Edit 连接名称", "reviewed final edit")
	connectionClick(t, tester, "Save connection")
	settleConnections(t, p, tester)
	if current, _ := s.Connections.Connection(entry.ID); current.Revision != entry.Revision+2 || current.Name != "reviewed final edit" || p.connections.mode != "" {
		t.Fatal("explicit edit after reconciliation did not save the verified revision")
	}
}

func TestNativeConnectionsLostCreationReceiptRetryRecoverAndCancel(t *testing.T) {
	for _, action := range []string{"retry", "recover", "cancel"} {
		t.Run(action, func(t *testing.T) {
			s, store := connectionsFixture(t)
			p := NewNativePage(s)
			p.model.Draft = "keep workspace text"
			tester := openConnectionTestPage(t, p, 960, 680)
			startOpenAIConnection(t, tester)
			connectionType(t, tester, "Edit API Key", "dummy-original-creation-key")
			connectionType(t, tester, "Edit 模型 ID", "accurate-created-model")
			tester.Key(0, ui.KeyEnter)
			store.loseReceipt.Store(true)
			connectionClick(t, tester, "Save connection")
			settleConnections(t, p, tester)
			request := p.connections.creationRequest
			stored, err := store.ConnectionsStore.Connections(t.Context())
			if err != nil || len(stored) != 1 || request == "" || !tester.HasText("创建结果尚未确认") || p.connections.draft.APIKey != "dummy-original-creation-key" {
				t.Fatal("lost receipt did not retain one actual entity and its recoverable draft", err)
			}
			for _, label := range []string{"Recover connection creation", "Save connection", "Cancel connection draft"} {
				bounds, ok := tester.Find(label)
				if !ok || bounds.Y < 24 || bounds.Y+bounds.H > 656 {
					t.Fatalf("uncertain creation moved %s outside 960x680 viewport: %+v", label, bounds)
				}
			}
			connectionClick(t, tester, "Back from connection form")
			connectionClick(t, tester, "Add OpenAI connection")
			if p.connections.creationRequest != request {
				t.Fatal("Back regenerated the uncertain creation identity")
			}
			store.loseReceipt.Store(false)
			switch action {
			case "retry":
				connectionClick(t, tester, "Save connection")
			case "recover":
				connectionClick(t, tester, "Recover connection creation")
			case "cancel":
				connectionClick(t, tester, "Cancel connection draft")
				connectionClick(t, tester, "刷新连接")
			}
			settleConnections(t, p, tester)
			actual, err := store.ConnectionsStore.Connections(t.Context())
			if err != nil || len(actual) != 1 || actual[0].ID != stored[0].ID || actual[0].Revision != 1 || p.connections.mode != "" || p.connections.creationRequest != "" || p.connections.draft.APIKey != "" {
				t.Fatal("explicit recovery/retry/cancel duplicated, edited or lost the committed creation", err)
			}
			if p.model.Draft != "keep workspace text" || p.selected != "" || s.Runtime.HasActive() {
				t.Fatal("creation recovery modified the workspace or executed")
			}
		})
	}
}

// The catalog field sits under 高级设置. A preset's catalog that stayed on the
// preset host after the endpoint moved elsewhere failed validation with no
// visible field to fix, so the form keeps a form-filled catalog on the
// endpoint's origin and never overrides one the user typed.
func TestNativeConnectionsPresetCatalogFollowsEditedEndpoint(t *testing.T) {
	s, store := connectionsFixture(t)
	p := NewNativePage(s)
	tester := openConnectionTestPage(t, p, 1240, 850)
	startOpenAIConnection(t, tester)
	connectionClick(t, tester, "Connection details")
	connectionType(t, tester, "Edit 完整请求地址", "https://api.deepseek.com/chat/completions")
	if got := p.connections.draft.CatalogURL; got != "https://api.deepseek.com/models" {
		t.Fatal("preset catalog stayed on its own host after the endpoint moved", got)
	}
	connectionType(t, tester, "Edit API Key", "dummy-deepseek-key")
	connectionClick(t, tester, "Save connection")
	settleConnections(t, p, tester)
	saved := s.Connections.Snapshot()
	if len(saved) != 1 || saved[0].CatalogURL != "https://api.deepseek.com/models" || store.writes.Load() == 0 {
		t.Fatal("moved preset connection did not save with a same-origin catalog", saved, p.connections.problem)
	}
	connectionClick(t, tester, "Edit connection")
	connectionClick(t, tester, "Advanced connection headers")
	connectionType(t, tester, "Edit 模型目录地址（可选，同源）", "https://api.deepseek.com/v1/models")
	connectionType(t, tester, "Edit 完整请求地址", "https://api.deepseek.com/beta/chat/completions")
	if got := p.connections.draft.CatalogURL; got != "https://api.deepseek.com/v1/models" {
		t.Fatal("editing the endpoint replaced a catalog the user typed", got)
	}
}

func TestNativeConnectionsRecoverCreationPreservesLaterFieldsAndSecrets(t *testing.T) {
	s, store := connectionsFixture(t)
	p := NewNativePage(s)
	tester := openConnectionTestPage(t, p, 960, 680)
	startOpenAIConnection(t, tester)
	connectionType(t, tester, "Edit API Key", "dummy-original-creation-key")
	store.loseReceipt.Store(true)
	connectionClick(t, tester, "Save connection")
	settleConnections(t, p, tester)
	request := p.connections.creationRequest
	connectionClick(t, tester, "Connection details")
	connectionType(t, tester, "Edit 连接名称", "later unsaved title")
	connectionType(t, tester, "Edit API Key", "dummy-later-unsaved-key")
	store.loseReceipt.Store(false)
	connectionClick(t, tester, "Save connection")
	settleConnections(t, p, tester)
	if p.connections.creationRequest != request || !tester.HasText("请先恢复保存结果") {
		t.Fatal("changed request silently created another connection")
	}
	connectionClick(t, tester, "Recover connection creation")
	settleConnections(t, p, tester)
	if p.connections.mode != "form" || p.connections.draft.ID == "" || p.connections.draft.ExpectedRevision != 1 || p.connections.draft.Name != "later unsaved title" || p.connections.draft.APIKey != "dummy-later-unsaved-key" {
		t.Fatal("recovery discarded later fields/secrets or failed to bind the stable created ID")
	}
	connectionClick(t, tester, "Save connection")
	settleConnections(t, p, tester)
	actual := s.Connections.Snapshot()
	if len(actual) != 1 || actual[0].Revision != 2 || actual[0].Name != "later unsaved title" {
		t.Fatal("explicit post-recovery edit duplicated or failed to update the same connection")
	}
	secret, err := store.ConnectionSecrets(t.Context(), actual[0].ID, actual[0].Revision)
	if err != nil || secret.APIKey != "dummy-later-unsaved-key" {
		t.Fatal("post-recovery save lost the explicit replacement key", err)
	}
}

func TestNativeConnectionsRecoverRevisedOrDeletedCreationRequiresReview(t *testing.T) {
	for _, action := range []string{"edit", "delete"} {
		t.Run(action, func(t *testing.T) {
			s, store := connectionsFixture(t)
			p := NewNativePage(s)
			tester := openConnectionTestPage(t, p, 960, 680)
			startOpenAIConnection(t, tester)
			connectionType(t, tester, "Edit API Key", "dummy-uncertain-key")
			store.loseReceipt.Store(true)
			connectionClick(t, tester, "Save connection")
			settleConnections(t, p, tester)
			store.loseReceipt.Store(false)
			other := NewNativePage(s)
			otherUI := openConnectionTestPage(t, other, 960, 680)
			original := s.Connections.Snapshot()[0]
			if action == "edit" {
				connectionClick(t, otherUI, "Edit connection")
				connectionType(t, otherUI, "Edit 连接名称", "other window title")
				connectionClick(t, otherUI, "Save connection")
			} else {
				connectionClick(t, otherUI, "Delete connection")
				connectionClick(t, otherUI, "Confirm delete connection")
			}
			settleConnections(t, other, otherUI)
			connectionClick(t, tester, "Recover connection creation")
			settleConnections(t, p, tester)
			if p.connections.mode != "form" || p.connections.draft.ID != original.ID || p.connections.draft.ExpectedRevision != 1 || p.connections.draft.APIKey != "dummy-uncertain-key" {
				t.Fatal("recovery lost the original identity or unsaved private draft")
			}
			writes := store.writes.Load()
			connectionClick(t, tester, "Save connection")
			if p.connections.saving || store.writes.Load() != writes {
				t.Fatal("recovery overwrote a later edit or recreated a deleted connection")
			}
			if action == "edit" {
				connectionClick(t, tester, "Reload connection draft")
				settleConnections(t, p, tester)
				if p.connections.draft.ExpectedRevision != 2 || p.connections.draft.Name != "other window title" || p.connections.draft.APIKey != "" {
					t.Fatal("explicit reload did not review the newer committed connection")
				}
			} else if !tester.HasText("被删除") || len(s.Connections.Snapshot()) != 0 {
				t.Fatal("deleted recovery did not explain the preserved tombstone identity")
			}
		})
	}
}
