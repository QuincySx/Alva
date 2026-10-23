// INPUT: Actual native MyGo controls, shared connection service and durable SQLite with bounded faults.
// OUTPUT: Explicit-save CRUD, secret redaction, keyboard/draft navigation and truthful failure regressions.
// POS: [L7 Hub UI testing] No OS window, account discovery, network, model generation or paid request.
//go:build desktop

package hubdesktop

import (
	"context"
	"errors"
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
	return s.ConnectionsStore.SaveConnection(ctx, u)
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
	tester := ui.NewTester(p.Build, width, height)
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
	if tester.HasText("完整请求地址") || tester.HasText("测试连接") || tester.HasText("获取模型") || tester.HasText("本机 Agent 账户") {
		t.Fatal("preset expanded by default or advertised unsupported requests")
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
		if !ok || r.Y > 120 {
			t.Fatalf("connection manager did not start below its toolbar: %s %+v", label, r)
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
	connectionClick(t, tester, "Connection authentication")
	connectionClick(t, tester, "无认证")
	connectionClick(t, tester, "Save connection")
	settleConnections(t, p, tester)
	entry = s.Connections.Snapshot()[0]
	secret, err = store.ConnectionSecrets(t.Context(), id, entry.Revision)
	if err != nil || entry.HasAPIKey || secret.APIKey != "" || entry.Auth != "none" {
		t.Fatal("explicit clear/no-auth did not remove the saved key")
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
	for _, size := range []struct{ width, height int }{{1240, 850}, {1000, 720}} {
		s, store := connectionsFixture(t)
		p := NewNativePage(s)
		tester := openConnectionTestPage(t, p, size.width, size.height)
		connectionClick(t, tester, "添加连接")
		connectionClick(t, tester, "Add Local connection")
		if tester.HasText("Edit API Key") || p.connections.draft.Auth != "none" || p.connections.draft.Endpoint != "" {
			t.Fatal("local connection invented an endpoint or required a key")
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
			if !ok || r.X < 244 || r.Y < 0 || r.X+r.W > float32(size.width)-24 || r.Y+r.H > float32(size.height)-24 {
				t.Fatalf("%s outside the connection viewport: %+v", label, r)
			}
		}
		connectionType(t, tester, "Edit 完整请求地址", "http://remote.example/v1/messages")
		connectionClick(t, tester, "Save connection")
		settleConnections(t, p, tester)
		if p.connections.mode != "form" || p.connections.draft.Endpoint != "http://remote.example/v1/messages" || !tester.HasText("草稿已保留") || store.writes.Load() != 0 {
			t.Fatal("invalid endpoint reached storage or discarded the draft")
		}
		connectionType(t, tester, "Edit 完整请求地址", "http://127.0.0.1:11434/v1/messages")
		connectionClick(t, tester, "Save connection")
		settleConnections(t, p, tester)
		if len(s.Connections.Snapshot()) != 1 || len(s.Connections.Targets()) != 0 || s.Runtime.HasActive() {
			t.Fatal("empty-model local connection could not save or fabricated a usable model")
		}
	}
}
