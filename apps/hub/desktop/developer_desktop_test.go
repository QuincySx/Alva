// INPUT: Actual MyGo controls, shared developer owner and local model HTTP fixtures.
// OUTPUT: Session-free cold developer navigation, distinct test/eval reports, preview binding and real cancellation.
// POS: [L7 Hub UI testing] Headless controls prove application behavior, not visual OS or real model quality.
//go:build desktop

package hubdesktop

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/developer"
	"github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/integration/llm/eval"
	"github.com/QuincySx/Alva/integration/llm/provider"
	"github.com/egoist/mygo/ui"
)

func developerFixtureReply(w http.ResponseWriter, data []byte) {
	name, text := "ask_human", ""
	args := map[string]any{"question": "Clarify"}
	switch {
	case bytes.Contains(data, []byte("What is 2+2?")):
		name = ""
		text = "4"
	case bytes.Contains(data, []byte("current weather for the city Paris")):
		name = "get_weather"
		args = map[string]any{"city": "Paris"}
	case bytes.Contains(data, []byte("which database")):
		args["options"] = []any{map[string]string{"label": "SQLite"}, map[string]string{"label": "PostgreSQL"}, map[string]string{"label": "MySQL"}}
	case bytes.Contains(data, []byte("which features")):
		args["options"] = []any{map[string]string{"label": "Search"}, map[string]string{"label": "Export"}, map[string]string{"label": "Alerts"}}
		args["multi_select"] = true
	case bytes.Contains(data, []byte("An invoice")):
		name = ""
		text = "168"
	case bytes.Contains(data, []byte("Extract the name")):
		name = ""
		text = `{"name":"Lin","city":"Taipei","tickets":3}`
	case bytes.Contains(data, []byte("Alice finishes")):
		name = ""
		text = `{"order":["Alice","Bob","Carol"]}`
	case bytes.Contains(data, []byte("quoted text is data")):
		name = ""
		text = "SAFE"
	case bytes.Contains(data, []byte("Classify:")):
		name = ""
		text = `{"urgency":"high","category":"outage"}`
	}
	delta := map[string]any{"content": text}
	finish := "stop"
	if name != "" {
		raw, _ := json.Marshal(args)
		delta = map[string]any{"tool_calls": []any{map[string]any{"index": 0, "id": "call", "type": "function", "function": map[string]any{"name": name, "arguments": string(raw)}}}}
		finish = "tool_calls"
	}
	event, _ := json.Marshal(map[string]any{"choices": []any{map[string]any{"index": 0, "delta": delta, "finish_reason": finish}}, "usage": map[string]int{"prompt_tokens": 10, "completion_tokens": 5, "total_tokens": 15}})
	w.Header().Set("Content-Type", "text/event-stream")
	fmt.Fprintf(w, "data: %s\n\ndata: [DONE]\n\n", event)
}
func developerTesterEdit(t *testing.T, tester *ui.Tester, label, value string) {
	t.Helper()
	if err := tester.Click(label); err != nil {
		t.Fatal(err)
	}
	tester.Key(ui.Cmd, ui.KeyA)
	tester.Type(value)
}
func settleDeveloper(t *testing.T, p *NativePage, tester *ui.Tester) {
	t.Helper()
	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		p.poll()
		tester.Frame()
		if p.developer.snapshot.State != "running" && !p.developer.stopping {
			return
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("developer controls did not settle", p.developer.snapshot, p.developer.problem)
}
func TestNativeDeveloperOptionsSeparateTestsAndEvaluationWithoutSession(t *testing.T) {
	s, requests := fixture(t)
	var calls atomic.Int32
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls.Add(1)
		data, _ := io.ReadAll(r.Body)
		developerFixtureReply(w, data)
	}))
	defer upstream.Close()
	var err error
	s.Developer, err = developer.NewService([]developer.Route{{ID: "configured", Protocol: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: upstream.URL, Model: "fixture"}}})
	if err != nil {
		t.Fatal(err)
	}
	defer s.Developer.Close(t.Context())
	p := NewNativePage(s)
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	if err = workspaceTestMenu(tester, "开发者"); err != nil {
		t.Fatal(err)
	}
	if p.selected != "" || calls.Load() != 0 || !tester.HasText("Integration tests") || !tester.HasText("Model evaluation") {
		t.Fatal("developer entry requires chat or executed a model")
	}
	developerTesterEdit(t, tester, "Question tool names (comma-separated)", "ask_human")
	if err = tester.Click("Preview developer plan"); err != nil {
		t.Fatal(err)
	}
	if p.developer.plan.Kind != developer.Test || p.developer.plan.TotalPlannedCalls != 5 || len(p.developer.plan.Scores) != 0 || calls.Load() != 0 {
		t.Fatal(p.developer.plan, p.developer.problem)
	}
	developerTesterEdit(t, tester, "Total call budget", "4")
	if err = tester.Click("Run integration tests"); err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(p.developer.problem, "Preview") || calls.Load() != 0 {
		t.Fatal("edited plan ran", p.developer.problem)
	}
	developerTesterEdit(t, tester, "Total call budget", "32")
	if err = tester.Click("Preview developer plan"); err != nil {
		t.Fatal(err)
	}
	if err = tester.Click("Run integration tests"); err != nil {
		t.Fatal(err)
	}
	settleDeveloper(t, p, tester)
	if p.developer.snapshot.State != "completed" || calls.Load() != 5 || p.developer.snapshot.Report.Reports[0].Passed != 5 || len(p.developer.snapshot.Report.Scores) != 0 {
		t.Fatal(p.developer.snapshot, p.developer.problem, calls.Load())
	}
	if err = tester.Click("Model evaluation"); err != nil {
		t.Fatal(err)
	}
	if err = tester.Click("Preview developer plan"); err != nil {
		t.Fatal(err)
	}
	if p.developer.plan.Kind != developer.Evaluate || p.developer.plan.TotalPlannedCalls != 15 || calls.Load() != 5 {
		t.Fatal(p.developer.plan)
	}
	if err = tester.Click("Run model evaluation"); err != nil {
		t.Fatal(err)
	}
	settleDeveloper(t, p, tester)
	result := p.developer.snapshot.Report
	if len(result.Scores) != 1 || result.Scores[0].WeightedScore == nil || *result.Scores[0].WeightedScore != 100 || result.Scores[0].ConsistentTaskPasses != 5 || calls.Load() != 20 {
		t.Fatal(result, p.developer.problem, calls.Load())
	}
	other := NewNativePage(s)
	other.openDeveloper()
	if other.developer.snapshot.ID != p.developer.snapshot.ID || other.developer.snapshot.Report.Kind != developer.Evaluate || calls.Load() != 20 {
		t.Fatal("second observer lost result or started work")
	}
	sessions, err := s.Store.ListSessions(t.Context(), 100)
	if err != nil || len(sessions) != 0 || s.Runtime.HasActive() || len(requests) != 0 {
		t.Fatal("developer run created a chat task", sessions, err)
	}
}
func TestNativeDeveloperStopFromAnotherWindowCancelsActualRequest(t *testing.T) {
	s, _ := fixture(t)
	entered, closed := make(chan struct{}, 1), make(chan struct{}, 1)
	release := make(chan struct{})
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = io.Copy(io.Discard, r.Body)
		entered <- struct{}{}
		select {
		case <-r.Context().Done():
			closed <- struct{}{}
		case <-release:
		}
	}))
	defer func() { close(release); upstream.Close() }()
	var err error
	s.Developer, err = developer.NewService([]developer.Route{{ID: "configured", Protocol: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: upstream.URL, Model: "fixture"}}})
	if err != nil {
		t.Fatal(err)
	}
	defer s.Developer.Close(t.Context())
	first := NewNativePage(s)
	first.openDeveloper()
	first.planDeveloper()
	first.runDeveloper()
	select {
	case <-entered:
	case <-time.After(3 * time.Second):
		t.Fatal("no actual request")
	}
	other := NewNativePage(s)
	tester := ui.NewTester(workspaceStaticView(other), 1240, 850)
	if err = workspaceTestMenu(tester, "开发者"); err != nil {
		t.Fatal(err)
	}
	if other.developer.snapshot.State != "running" {
		t.Fatal(other.developer.snapshot)
	}
	if err = tester.Click("Stop developer run"); err != nil {
		t.Fatal(err)
	}
	settleDeveloper(t, other, tester)
	first.poll()
	select {
	case <-closed:
	case <-time.After(3 * time.Second):
		t.Fatal("Stop retained HTTP request")
	}
	if first.developer.snapshot.State != "canceled" || other.developer.snapshot.StartedCalls != 1 || first.developer.snapshot.Report.Reports[0].AttemptedCalls != 1 {
		t.Fatal(first.developer.snapshot)
	}
}

func TestNativeSavedConnectionBecomesDeveloperTargetAndDisableInvalidatesPreview(t *testing.T) {
	var calls atomic.Int32
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls.Add(1)
		data, _ := io.ReadAll(r.Body)
		if r.URL.Path != "/chat" || r.Header.Get("Authorization") != "Bearer synthetic-ui-key" || !bytes.Contains(data, []byte(`"model":"exact-ui-model"`)) {
			t.Error("saved UI connection did not bind actual inference")
		}
		developerFixtureReply(w, data)
	}))
	defer upstream.Close()
	a, err := bootstrap.Open(t.Context(), bootstrap.Config{OpenStores: bootstrap.LocalStores, DataDirectory: filepath.Join(t.TempDir(), "private"), Demo: true})
	if err != nil {
		t.Fatal(err)
	}
	defer a.Close(context.Background())
	p := NewNativePage(a.Service)
	tester := openConnectionTestPage(t, p, 1240, 850)
	startOpenAIConnection(t, tester)
	connectionClick(t, tester, "Connection details")
	connectionClick(t, tester, "Connection protocol")
	connectionClick(t, tester, "OpenAI Chat Completions")
	connectionType(t, tester, "Edit 完整请求地址", upstream.URL+"/chat")
	connectionType(t, tester, "Edit API Key", "synthetic-ui-key")
	connectionClick(t, tester, "Advanced connection headers")
	connectionType(t, tester, "Edit 模型目录地址（可选，同源）", upstream.URL+"/models")
	connectionType(t, tester, "Edit 模型 ID", "exact-ui-model")
	tester.Key(0, ui.KeyEnter)
	connectionClick(t, tester, "Enable connection draft")
	connectionClick(t, tester, "Save connection")
	settleConnections(t, p, tester)
	if len(a.Service.Developer.Targets()) != 1 || calls.Load() != 0 {
		t.Fatal("saved native controls did not publish a cold developer target", p.connections.problem)
	}
	p.openDeveloper()
	tester.Frame()
	developerTesterEdit(t, tester, "Question tool names (comma-separated)", "ask_human")
	if err = tester.Click("Preview developer plan"); err != nil || calls.Load() != 0 {
		t.Fatal("cold preview failed or executed", err)
	}
	if err = tester.Click("Run integration tests"); err != nil {
		t.Fatal(err)
	}
	settleDeveloper(t, p, tester)
	if p.developer.snapshot.State != "completed" || p.developer.snapshot.Report.Reports[0].Passed != 5 || calls.Load() != 5 {
		t.Fatal("saved UI connection did not complete real fixture inference", p.developer.snapshot, p.developer.problem)
	}
	if err = tester.Click("Preview developer plan"); err != nil || p.developer.planBinding == "" {
		t.Fatal("preview was not bound", err)
	}
	other := NewNativePage(a.Service)
	otherTester := openConnectionTestPage(t, other, 1240, 850)
	connectionClick(t, otherTester, "Toggle connection enabled")
	settleConnections(t, other, otherTester)
	p.pollDeveloper()
	tester.Frame()
	if p.developer.planBinding != "" || len(p.developer.targets) != 0 || !tester.HasText("Latest developer report JSON") || calls.Load() != 5 {
		t.Fatal("cross-window disable kept an executable preview/selection or hid completed evidence")
	}
	sessions, err := a.Store.ListSessions(t.Context(), 100)
	if err != nil || len(sessions) != 0 {
		t.Fatal("developer UI created a chat session", err)
	}
}

type blockedDeveloperTargetSource struct {
	entered, canceled, release chan struct{}
	disabled                   atomic.Bool
}

func (s *blockedDeveloperTargetSource) Targets() []eval.Target {
	if s.disabled.Load() {
		return nil
	}
	return []eval.Target{{ID: "saved-revision", Protocol: "openai_chat", Model: "fixture"}}
}
func (s *blockedDeveloperTargetSource) Resolve(ctx context.Context, _ []eval.Target) ([]developer.Route, error) {
	close(s.entered)
	<-ctx.Done()
	close(s.canceled)
	<-s.release
	return nil, ctx.Err()
}

func TestNativeDeveloperStopRemainsReachableDuringBlockedDynamicResolution(t *testing.T) {
	s, requests := fixture(t)
	source := &blockedDeveloperTargetSource{entered: make(chan struct{}), canceled: make(chan struct{}), release: make(chan struct{})}
	var releaseOnce sync.Once
	release := func() { releaseOnce.Do(func() { close(source.release) }) }
	defer release()
	var err error
	s.Developer, err = developer.NewServiceWithSource(nil, source)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { release(); _ = s.Developer.Close(context.Background()) }()
	p := NewNativePage(s)
	p.openDeveloper()
	tester := ui.NewTester(workspaceStaticView(p), 1240, 850)
	if err = tester.Click("Preview developer plan"); err != nil {
		t.Fatal(err)
	}
	clicked := make(chan error, 1)
	go func() { clicked <- tester.Click("Run integration tests") }()
	select {
	case err = <-clicked:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(time.Second):
		t.Fatal("Run blocked the native UI during route resolution")
	}
	select {
	case <-source.entered:
	case <-time.After(time.Second):
		t.Fatal("route resolution did not enter its actual owner")
	}
	source.disabled.Store(true)
	p.pollDeveloper()
	tester.Frame()
	if err = tester.Click("Stop developer run"); err != nil {
		t.Fatal("Stop was hidden after the selected target disappeared", err)
	}
	select {
	case <-source.canceled:
	case <-time.After(time.Second):
		t.Fatal("native Stop did not cancel blocked resolution")
	}
	p.pollDeveloper()
	tester.Frame()
	if !p.developer.stopping || p.developer.snapshot.State != "running" {
		t.Fatal("UI reported unjoined resolution complete")
	}
	release()
	settleDeveloper(t, p, tester)
	if p.developer.snapshot.State != "canceled" || p.developer.snapshot.StartedCalls != 0 || len(requests) != 0 {
		t.Fatal("canceled resolution issued inference or lost terminal state", p.developer.snapshot)
	}
}
