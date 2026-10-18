// INPUT: Real loopback HTTP, isolated Go product/native stores and local provider fixtures
// OUTPUT: Authentication, reference isolation, durable SSE resume and detached execution regressions
// POS: [L7 Hub product] Headless integration acceptance without user history, credentials, billing or native windows
package server

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/apps/hub"
	bootstrap "github.com/QuincySx/Alva/apps/hub/bootstrap"
	"github.com/QuincySx/Alva/apps/hub/product"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/integration/llm/provider"
	"github.com/QuincySx/Alva/platform/system/loopback"
	"github.com/QuincySx/Alva/sdk/agent/security"
)

type fixture struct {
	app   *bootstrap.Application
	http  *httptest.Server
	token string
}

func newFixture(t *testing.T, config bootstrap.Config) *fixture {
	t.Helper()
	config.DataDirectory = filepath.Join(t.TempDir(), "product")
	a, err := bootstrap.Open(context.Background(), config)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		if err := a.Close(ctx); err != nil {
			t.Error(err)
		}
	})
	token, err := loopback.NewToken()
	if err != nil {
		t.Fatal(err)
	}
	ts := httptest.NewUnstartedServer(nil)
	s, err := New(a.Service, Config{Authority: ts.Listener.Addr().String(), Token: token, PollInterval: 5 * time.Millisecond})
	if err != nil {
		t.Fatal(err)
	}
	ts.Config.Handler = s
	ts.Config.ReadHeaderTimeout = time.Second
	ts.Start()
	t.Cleanup(func() { _ = s.Close(); ts.Close() })
	return &fixture{a, ts, token}
}
func (f *fixture) request(t *testing.T, method, path string, data []byte, headers map[string]string) *http.Response {
	t.Helper()
	r, err := http.NewRequest(method, f.http.URL+path, bytes.NewReader(data))
	if err != nil {
		t.Fatal(err)
	}
	r.Header.Set("Authorization", "Bearer "+f.token)
	if data != nil {
		r.Header.Set("Content-Type", "application/json")
	}
	for key, value := range headers {
		if key == "Host" {
			r.Host = value
		} else {
			r.Header.Set(key, value)
		}
	}
	response, err := f.http.Client().Do(r)
	if err != nil {
		t.Fatal(err)
	}
	return response
}
func body(t *testing.T, response *http.Response, status int) []byte {
	t.Helper()
	defer response.Body.Close()
	data, err := io.ReadAll(io.LimitReader(response.Body, 9<<20))
	if err != nil {
		t.Fatal(err)
	}
	if response.StatusCode != status {
		t.Fatalf("status %d want %d: %s", response.StatusCode, status, data)
	}
	return data
}
func create(t *testing.T, f *fixture, title string) hub.Session {
	t.Helper()
	data, _ := json.Marshal(map[string]string{"title": title})
	raw := body(t, f.request(t, "POST", "/v1/sessions", data, nil), 201)
	var value hub.Session
	if json.Unmarshal(raw, &value) != nil || value.ID == "" {
		t.Fatalf("invalid session: %s", raw)
	}
	return value
}
func waitIdle(t *testing.T, f *fixture, id string) {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		v, err := f.app.Store.Detail(context.Background(), id)
		if err != nil {
			t.Fatal(err)
		}
		if v.Session.Status != hub.Running && !f.app.Runtime.HasActive() {
			if v.Session.Status != engine.Succeeded {
				t.Fatalf("terminal %s", v.Session.Status)
			}
			return
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("turn failed to settle")
}

func TestAuthenticationOriginAuthorityAndStrictBodies(t *testing.T) {
	f := newFixture(t, bootstrap.Config{Demo: true})
	for _, test := range []struct {
		path    string
		headers map[string]string
		status  int
	}{
		{"/v1/health", map[string]string{"Authorization": ""}, 401},
		{"/v1/health", map[string]string{"Origin": "https://evil.example"}, 403},
		{"/v1/health", map[string]string{"Sec-Fetch-Site": "cross-site"}, 403},
		{"/v1/health", map[string]string{"Host": "evil.example:1234"}, 403},
		{"/v1/health?token=secret", nil, 400},
		{"/v1/health?unknown=1", nil, 400},
		{"/v1/health", map[string]string{"Origin": f.http.URL}, 200},
	} {
		body(t, f.request(t, "GET", test.path, nil, test.headers), test.status)
	}
	for _, data := range []string{`{"title":"x","unexpected":true}`, `{"title":"\ud800"}`, `{"title":"x"} {}`, `null`, `{"title":"x","title":"y"}`, `{"Title":"x"}`} {
		response := f.request(t, "POST", "/v1/sessions", []byte(data), nil)
		// null yields empty required fields, never a successful mutation.
		if response.StatusCode >= 200 && response.StatusCode < 300 {
			t.Fatalf("invalid body accepted: %s", data)
		}
		_ = body(t, response, response.StatusCode)
	}
	response := f.request(t, "GET", "/v1/sessions", nil, nil)
	if response.Header.Get("Access-Control-Allow-Origin") != "" || response.Header.Get("Cache-Control") != "no-store" {
		t.Fatal("insecure cache/CORS")
	}
	var page struct {
		Sessions []hub.Session `json:"sessions"`
	}
	if json.Unmarshal(body(t, response, 200), &page) != nil || len(page.Sessions) != 0 {
		t.Fatal("rejected requests mutated store")
	}
	for _, authority := range []string{"0.0.0.0:8080", "localhost:8080", "127.0.0.1:0", "127.0.0.1:08080", "192.168.1.1:8080"} {
		if _, err := New(f.app.Service, Config{Authority: authority, Token: f.token}); err == nil {
			t.Fatalf("authority accepted %s", authority)
		}
	}
	if _, err := New(f.app.Service, Config{Authority: "127.0.0.1:8080", Token: "password"}); err == nil {
		t.Fatal("weak token accepted")
	}
}

func TestOriginalUploadReferenceColdInspectorAndForeignRead(t *testing.T) {
	f := newFixture(t, bootstrap.Config{Demo: true})
	a := create(t, f, "files")
	other := create(t, f, "other")
	raw := []byte("PRIVATE-RAW-LOG-正文-not-in-prompt\n")
	response := f.request(t, "POST", "/v1/sessions/"+a.ID+"/attachments", raw, map[string]string{"X-File-Name": "service.log", "Content-Type": "application/octet-stream"})
	var upload product.DraftAttachment
	if json.Unmarshal(body(t, response, 201), &upload) != nil {
		t.Fatal("bad upload DTO")
	}
	if upload.Reference.URI == "" || upload.Info.Bytes != int64(len(raw)) || !strings.Contains(upload.Info.Processing, "No compression") || f.app.Runtime.HasActive() {
		t.Fatal("upload lost provenance or started runtime")
	}
	path := "/v1/sessions/" + a.ID + "/artifact-bytes?branch=" + upload.BranchID + "&uri=" + url.QueryEscape(upload.Reference.URI)
	var page struct {
		Data []byte `json:"data"`
	}
	if json.Unmarshal(body(t, f.request(t, "GET", path, nil, nil), 200), &page) != nil || !bytes.Equal(page.Data, raw) {
		t.Fatal("not original bytes")
	}
	foreign := strings.Replace(path, a.ID, other.ID, 1)
	body(t, f.request(t, "GET", foreign, nil, nil), 404)
	body(t, f.request(t, "GET", "/v1/sessions/"+a.ID+"/inspector?branch="+upload.BranchID, nil, nil), 200)
	if f.app.Runtime.HasActive() {
		t.Fatal("cold inspector restored agent")
	}
	body(t, f.request(t, "POST", "/v1/sessions/"+a.ID+"/attachments", raw, map[string]string{"X-File-Name": "../../host-secret"}), 409)
	body(t, f.request(t, "POST", "/v1/sessions/"+a.ID+"/attachments?path=/etc/passwd", nil, map[string]string{"X-File-Name": "not-opened"}), 400)
	body(t, f.request(t, "POST", "/v1/sessions/"+a.ID+"/turns", []byte(`{"prompt":"Search the relevant file error"}`), nil), 202)
	waitIdle(t, f, a.ID)
	history := body(t, f.request(t, "GET", "/v1/sessions/"+a.ID+"/history?limit=64", nil, nil), 200)
	if bytes.Contains(history, raw) || !bytes.Contains(history, []byte(upload.Reference.URI)) {
		t.Fatal("raw upload entered model/timeline or missing reference")
	}
	if len(f.app.Service.DraftAttachments(a.ID)) != 0 {
		t.Fatal("admitted attachment draft not consumed")
	}
	body(t, f.request(t, "GET", "/v1/sessions/"+a.ID+"/history?limit=65", nil, nil), 400)
}

func readHistoryEvent(t *testing.T, r *bufio.Reader) (uint64, []byte) {
	t.Helper()
	var id uint64
	var data []byte
	for {
		line, err := r.ReadString('\n')
		if err != nil {
			t.Fatal(err)
		}
		line = strings.TrimSuffix(line, "\n")
		if strings.HasPrefix(line, "id: ") {
			id, err = strconv.ParseUint(strings.TrimPrefix(line, "id: "), 10, 64)
			if err != nil {
				t.Fatal(err)
			}
		}
		if strings.HasPrefix(line, "data: ") {
			data = []byte(strings.TrimPrefix(line, "data: "))
		}
		if line == "" && id != 0 {
			return id, data
		}
	}
}

func TestDurableSSEResumeAndDisconnectDoesNotStop(t *testing.T) {
	started := make(chan struct{})
	release := make(chan struct{})
	cancelled := make(chan struct{})
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/event-stream")
		_, _ = io.WriteString(w, "data: {\"id\":\"response-1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"working\"}}]}\n\n")
		w.(http.Flusher).Flush()
		close(started)
		select {
		case <-release:
			_, _ = io.WriteString(w, "data: {\"id\":\"response-1\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n")
		case <-r.Context().Done():
			close(cancelled)
		}
	}))
	defer upstream.Close()
	f := newFixture(t, bootstrap.Config{ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: upstream.URL, Model: "fixture"}})
	a := create(t, f, "observe")
	body(t, f.request(t, "POST", "/v1/sessions/"+a.ID+"/turns", []byte(`{"prompt":"wait"}`), nil), 202)
	select {
	case <-started:
	case <-time.After(3 * time.Second):
		t.Fatal("model not started")
	}
	stream := f.request(t, "GET", "/v1/sessions/"+a.ID+"/events", nil, nil)
	if stream.StatusCode != 200 || !strings.HasPrefix(stream.Header.Get("Content-Type"), "text/event-stream") {
		t.Fatal("not SSE")
	}
	sequence, _ := readHistoryEvent(t, bufio.NewReader(stream.Body))
	stream.Body.Close()
	select {
	case <-cancelled:
		t.Fatal("observer disconnect cancelled execution")
	case <-time.After(30 * time.Millisecond):
	}
	close(release)
	waitIdle(t, f, a.ID)
	stream = f.request(t, "GET", "/v1/sessions/"+a.ID+"/events", nil, map[string]string{"Last-Event-ID": fmt.Sprint(sequence)})
	next, data := readHistoryEvent(t, bufio.NewReader(stream.Body))
	stream.Body.Close()
	var entry hub.HistoryEntry
	if json.Unmarshal(data, &entry) != nil || next <= sequence || next != entry.Sequence {
		t.Fatal("SSE did not resume from committed authority")
	}
	body(t, f.request(t, "GET", "/v1/sessions/"+a.ID+"/events?after=0", nil, map[string]string{"Last-Event-ID": "0"}), 400)
	body(t, f.request(t, "GET", "/v1/sessions/"+a.ID+"/events", nil, map[string]string{"Last-Event-ID": "-1"}), 400)
}

func TestSettingsImportDisabledAndConfiguredRoutesOnly(t *testing.T) {
	f := newFixture(t, bootstrap.Config{Demo: true})
	a := create(t, f, "settings")
	prefix := "/v1/sessions/" + a.ID
	body(t, f.request(t, "PUT", prefix+"/settings/mcp", []byte(`{"servers":[{"id":"server","command":"never-run-fixture","enabled":true,"host_access_approved":true}]}`), nil), 200)
	var settings hub.Settings
	if json.Unmarshal(body(t, f.request(t, "GET", prefix+"/settings", nil, nil), 200), &settings) != nil || len(settings.MCP) != 1 || settings.MCP[0].Enabled || settings.MCP[0].HostAccessApproved {
		t.Fatal("import silently enabled host access")
	}
	body(t, f.request(t, "PUT", prefix+"/settings/mcp/server", []byte(`{"enabled":true}`), nil), 409)
	if f.app.Runtime.HasActive() {
		t.Fatal("settings started execution")
	}
	target := f.app.Service.DefaultTarget
	target.ModelID = "fabricated-model"
	data, _ := json.Marshal(map[string]any{"target": target, "reason": "invalid"})
	body(t, f.request(t, "POST", prefix+"/branch", data, nil), 400)
	body(t, f.request(t, "PUT", prefix+"/fallback", []byte(`{"targets":[]}`), nil), 200)
	if f.app.Runtime.HasActive() {
		t.Fatal("configuration executed a runtime")
	}
}

func TestColdReadDoesNotIssueModelRequests(t *testing.T) {
	var calls atomic.Int32
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { calls.Add(1); http.Error(w, "not permitted", 500) }))
	defer upstream.Close()
	f := newFixture(t, bootstrap.Config{ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: upstream.URL, Model: "fixture"}})
	a := create(t, f, "cold")
	for _, path := range []string{"/v1/health", "/v1/targets", "/v1/sessions", "/v1/sessions/" + a.ID, "/v1/sessions/" + a.ID + "/settings", "/v1/sessions/" + a.ID + "/history"} {
		body(t, f.request(t, "GET", path, nil, nil), 200)
	}
	if calls.Load() != 0 {
		t.Fatal("cold transport called provider")
	}
}

func TestHTTPApprovalBoundToActualRunningSession(t *testing.T) {
	var calls atomic.Int32
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/event-stream")
		if calls.Add(1) == 1 {
			call := map[string]any{"id": "response-approval", "choices": []any{map[string]any{"index": 0, "delta": map[string]any{"tool_calls": []any{map[string]any{"index": 0, "id": "call-write", "type": "function", "function": map[string]any{"name": "write_file", "arguments": `{"path":"approved.txt","content":"approved-content"}`}}}}, "finish_reason": "tool_calls"}}}
			data, _ := json.Marshal(call)
			_, _ = fmt.Fprintf(w, "data: %s\n\ndata: [DONE]\n\n", data)
		} else {
			_, _ = io.WriteString(w, "data: {\"id\":\"response-after\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"saved\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n")
		}
	}))
	defer upstream.Close()
	f := newFixture(t, bootstrap.Config{ProviderKind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: upstream.URL, Model: "fixture"}})
	a := create(t, f, "approval")
	foreign := create(t, f, "foreign")
	prefix := "/v1/sessions/" + a.ID
	body(t, f.request(t, "POST", prefix+"/turns", []byte(`{"prompt":"write the file"}`), nil), 202)
	var pending []security.ApprovalRequest
	deadline := time.Now().Add(3 * time.Second)
	for time.Now().Before(deadline) {
		data := body(t, f.request(t, "GET", prefix+"/approvals", nil, nil), 200)
		if json.Unmarshal(data, &pending) != nil {
			t.Fatal("invalid approvals")
		}
		if len(pending) > 0 {
			break
		}
		time.Sleep(time.Millisecond)
	}
	if len(pending) != 1 || pending[0].ToolName != "write_file" || pending[0].ArgumentsSHA256 == "" || !strings.Contains(pending[0].InputSummary, "approved.txt") {
		t.Fatal("missing actual argument approval")
	}
	path := filepath.Join(a.Workspace, "approved.txt")
	if _, err := os.Stat(path); !os.IsNotExist(err) {
		t.Fatal("write before approval", err)
	}
	body(t, f.request(t, "POST", prefix+"/approvals/"+pending[0].ID+"/decision", []byte(`{"allow":false,"allow":true}`), nil), 400)
	body(t, f.request(t, "POST", prefix+"/stop", []byte(`null`), nil), 400)
	body(t, f.request(t, "POST", "/v1/sessions/"+foreign.ID+"/approvals/"+pending[0].ID+"/decision", []byte(`{"allow":true}`), nil), 409)
	if _, err := os.Stat(path); !os.IsNotExist(err) {
		t.Fatal("foreign approval authorized write")
	}
	body(t, f.request(t, "POST", prefix+"/approvals/"+pending[0].ID+"/decision", []byte(`{"allow":true}`), nil), 200)
	waitIdle(t, f, a.ID)
	data, err := os.ReadFile(path)
	if err != nil || string(data) != "approved-content" {
		t.Fatal("authorized tool did not execute", err)
	}
	body(t, f.request(t, "POST", prefix+"/approvals/"+pending[0].ID+"/decision", []byte(`{"allow":true}`), nil), 409)
	if calls.Load() != 2 {
		t.Fatal("unexpected model/tool retry", calls.Load())
	}
}
