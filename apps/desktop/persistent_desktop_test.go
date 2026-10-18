// INPUT: Actual MyGo controls, durable direct Service and local HTTP file-tool approval fixture
// OUTPUT: Cold native session/create/composer/Inspector and cross-pane exact approval acceptance
// POS: [L6 application] Headless direct UI tests use temporary native facts and no Hub, external accounts or OS window.
//go:build desktop

package desktop

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/direct"
	"github.com/QuincySx/Alva/integration/llm/provider"
	"github.com/egoist/mygo/ui"
)

func persistentFixture(t *testing.T, config direct.Config) (*PersistentPage, *ui.Tester) {
	t.Helper()
	if config.DataDirectory == "" {
		config.DataDirectory = filepath.Join(t.TempDir(), "data")
	}
	service, err := direct.Open(t.Context(), config)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
		defer cancel()
		if err := service.Close(ctx); err != nil {
			t.Error(err)
		}
	})
	p := NewPersistentPage(service)
	ctx, cancel := context.WithCancel(t.Context())
	p.ctx = ctx
	t.Cleanup(func() {
		cancel()
		if p.detachRun != nil {
			p.detachRun()
		}
	})
	return p, ui.NewTester(p.Build, 1160, 1000)
}
func pumpPersistent(t *testing.T, p *PersistentPage, tester *ui.Tester, ready func() bool) {
	t.Helper()
	deadline := time.NewTimer(4 * time.Second)
	defer deadline.Stop()
	ticker := time.NewTicker(5 * time.Millisecond)
	defer ticker.Stop()
	for !ready() {
		select {
		case fn := <-p.updates:
			fn()
			p.refreshPermissions()
			tester.Frame()
		case <-ticker.C:
			p.refreshPermissions()
			tester.Frame()
		case <-deadline.C:
			t.Fatalf("actual persistent UI did not settle: busy=%v status=%s problem=%s", p.busy, p.model.Status, p.problem)
		}
	}
}

func TestPersistentNativeHomeCreateSendAndInspectorAreActualDirectPaths(t *testing.T) {
	p, tester := persistentFixture(t, direct.Config{Demo: true})
	if !tester.HasText("Create or select a durable direct session. Nothing executes on startup.") || p.Service.HasActive() {
		t.Fatal("cold direct home executed or hid its identity")
	}
	page, err := p.Service.List(t.Context(), "")
	if err != nil || len(page.Sessions) != 0 {
		t.Fatal("build created native facts", err)
	}
	if err := tester.Click("New session"); err != nil {
		t.Fatal(err)
	}
	pumpPersistent(t, p, tester, func() bool { return p.selected != "" && !p.busy && !p.listBusy })
	id := p.selected
	if p.problem != "" || p.Service.HasActive() {
		t.Fatal("native create executed", p.problem)
	}
	if err := tester.Click("Message"); err != nil {
		t.Fatal(err)
	}
	tester.Type("native persistent explicit Send")
	if p.Service.HasActive() {
		t.Fatal("typing ran Agent")
	}
	if err := tester.Click("Send"); err != nil {
		t.Fatal(err)
	}
	pumpPersistent(t, p, tester, func() bool {
		return !p.busy && p.model.Status == Completed && p.model.ActiveRunID == "" && len(p.model.Messages) > 0
	})
	if !tester.HasText("Tool result: native persistent explicit Send") || p.selected != id {
		t.Fatal("actual durable direct answer not rendered")
	}
	run, ok := p.Service.CurrentRun(id)
	if !ok {
		t.Fatal("actual Controller identity lost")
	}
	if err := tester.Click("Inspector"); err != nil {
		t.Fatal(err)
	}
	pumpPersistent(t, p, tester, func() bool { return !p.inspectorBusy && len(p.inspector.Entries) > 0 })
	if p.Service.HasActive() {
		t.Fatal("cold Inspector resumed execution")
	}
	if err := tester.Click("Read original 2 KiB"); err != nil {
		t.Fatal(err)
	}
	pumpPersistent(t, p, tester, func() bool { return !p.inspectorBusy && p.inspectURI != "" })
	if !strings.Contains(p.inspectText.Text, "alva.direct") {
		t.Fatal("Inspector did not read actual first metadata record")
	}
	beforeCursor, beforeHigh := p.inspectCursor, p.inspector.HighWater
	if err := tester.Click("Records: LLM"); err != nil {
		t.Fatal(err)
	}
	if p.inspectCursor != beforeCursor || p.inspector.HighWater != beforeHigh || p.Service.HasActive() {
		t.Fatal("category changed query/runtime authority")
	}
	if err := tester.Click("Records: All"); err != nil {
		t.Fatal(err)
	}
	if err := tester.Click("Export all records at this snapshot"); err != nil {
		t.Fatal(err)
	}
	pumpPersistent(t, p, tester, func() bool { return !p.inspectorBusy && p.inspectExportPath != "" })
	exportPath := strings.SplitN(p.inspectExportPath, "\n", 2)[0]
	bundle, err := os.ReadFile(exportPath)
	if err != nil || !strings.Contains(string(bundle), `"type":"complete"`) {
		t.Fatal("actual native export did not produce complete JSONL", err)
	}
	current, ok := p.Service.CurrentRun(id)
	if !ok || current.ID != run.ID {
		t.Fatal("Inspector replaced execution owner")
	}
	if err := tester.Click("Chat"); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(t.TempDir(), "fixture.txt")
	if err := os.WriteFile(path, []byte("original reference"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := p.Service.Upload(t.Context(), id, path); err != nil {
		t.Fatal(err)
	}
	p.loadHistory(0)
	pumpPersistent(t, p, tester, func() bool { return !p.busy && len(p.attachments) == 1 })
	if err := tester.Click("Remove draft reference"); err != nil {
		t.Fatal(err)
	}
	pumpPersistent(t, p, tester, func() bool { return !p.busy && len(p.attachments) == 0 })
	if p.Service.HasActive() {
		t.Fatal("attachment controls executed")
	}
}

func TestPersistentNativeApprovalRemainsVisibleInInspectorAndUsesExactRequest(t *testing.T) {
	workspace := t.TempDir()
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		finish := "stop"
		delta := map[string]any{"content": "native approved write"}
		if calls.Add(1) == 1 {
			args, _ := json.Marshal(map[string]string{"path": "approved.txt", "content": "native actual approval"})
			finish = "tool_calls"
			delta = map[string]any{"tool_calls": []any{map[string]any{"index": 0, "id": "native-call", "type": "function", "function": map[string]string{"name": "write_file", "arguments": string(args)}}}}
		}
		raw, _ := json.Marshal(map[string]any{"choices": []any{map[string]any{"index": 0, "delta": delta, "finish_reason": finish}}})
		w.Header().Set("Content-Type", "text/event-stream")
		fmt.Fprintf(w, "data: %s\n\ndata: [DONE]\n\n", raw)
	}))
	defer server.Close()
	p, tester := persistentFixture(t, direct.Config{Workspace: workspace, ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: server.URL, Model: "fixture"}})
	if err := tester.Click("New session"); err != nil {
		t.Fatal(err)
	}
	pumpPersistent(t, p, tester, func() bool { return p.selected != "" && !p.busy })
	if err := tester.Click("Message"); err != nil {
		t.Fatal(err)
	}
	tester.Type("guarded actual UI write")
	if err := tester.Click("Send"); err != nil {
		t.Fatal(err)
	}
	pumpPersistent(t, p, tester, func() bool { return len(p.approvals) == 1 && !p.busy })
	if _, err := os.Stat(filepath.Join(workspace, "approved.txt")); !os.IsNotExist(err) {
		t.Fatal("UI write happened before consent")
	}
	if err := tester.Click("Inspector"); err != nil {
		t.Fatal(err)
	}
	pumpPersistent(t, p, tester, func() bool { return !p.inspectorBusy })
	if _, ok := tester.Find("Requested input — review before allowing"); !ok || !strings.Contains(p.approvals[0].InputSummary, "approved.txt") {
		t.Fatal("actual approval input hidden in Inspector")
	}
	if err := tester.Click("Allow once"); err != nil {
		t.Fatal(err)
	}
	pumpPersistent(t, p, tester, func() bool { return !p.busy && p.model.Status == Completed && len(p.approvals) == 0 })
	raw, err := os.ReadFile(filepath.Join(workspace, "approved.txt"))
	if err != nil || string(raw) != "native actual approval" || calls.Load() != 2 {
		t.Fatal("actual UI approval did not settle normal file tool", err)
	}
}
