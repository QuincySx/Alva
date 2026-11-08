// INPUT: httptest local Streamable HTTP fixtures, a recording Observer and a scripted Authorizer
// OUTPUT: JSON/SSE replies, session and version headers, 202 notifications, expiry, legacy, auth retry, redirect and bound regressions
// POS: [integration / external protocol] Remote MCP acceptance on loopback only; no external servers or credentials
package mcp

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

type httpFixture struct {
	t           *testing.T
	mu          sync.Mutex
	mode        string
	calls       atomic.Int32
	deletes     atomic.Int32
	headers     []http.Header
	repliedPing atomic.Bool
}

func (f *httpFixture) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	f.mu.Lock()
	f.headers = append(f.headers, r.Header.Clone())
	mode := f.mode
	f.mu.Unlock()
	if r.Method == http.MethodDelete {
		f.deletes.Add(1)
		w.WriteHeader(http.StatusNoContent)
		return
	}
	var message struct {
		ID     json.RawMessage            `json:"id"`
		Method string                     `json:"method"`
		Params map[string]json.RawMessage `json:"params"`
		Result json.RawMessage            `json:"result"`
	}
	if err := json.NewDecoder(r.Body).Decode(&message); err != nil {
		http.Error(w, "bad", http.StatusBadRequest)
		return
	}
	if mode == "legacy" {
		http.Error(w, "use SSE", http.StatusMethodNotAllowed)
		return
	}
	if mode == "auth" && r.Header.Get("Authorization") != "Bearer fresh" {
		w.Header().Set("WWW-Authenticate", `Bearer resource_metadata="http://`+r.Host+`/.well-known/oauth-protected-resource"`)
		w.WriteHeader(http.StatusUnauthorized)
		return
	}
	if message.Method == "" {
		// A client response (to our ping) must be accepted with 202.
		if string(message.Result) == "{}" {
			f.repliedPing.Store(true)
		}
		w.WriteHeader(http.StatusAccepted)
		return
	}
	if message.Method != "initialize" && r.Header.Get("Mcp-Session-Id") != "session-1" {
		http.Error(w, "missing session", http.StatusBadRequest)
		return
	}
	var result any
	switch message.Method {
	case "initialize":
		w.Header().Set("Mcp-Session-Id", "session-1")
		result = map[string]any{"protocolVersion": "2025-06-18", "capabilities": map[string]any{"tools": map[string]any{}}, "serverInfo": map[string]string{"name": "http-fixture", "version": "1"}}
	case "notifications/initialized":
		w.WriteHeader(http.StatusAccepted)
		return
	case "tools/list":
		if _, ok := message.Params["cursor"]; !ok {
			result = map[string]any{"tools": []any{map[string]any{"name": "echo", "description": "first", "inputSchema": map[string]string{"type": "object"}}}, "nextCursor": "p2"}
		} else {
			result = map[string]any{"tools": []any{map[string]any{"name": "wait", "inputSchema": map[string]string{"type": "object"}}}}
		}
	case "tools/call":
		f.calls.Add(1)
		switch mode {
		case "expired":
			http.Error(w, "gone", http.StatusNotFound)
			return
		case "sse", "sse-huge", "sse-truncated":
			w.Header().Set("Content-Type", "text/event-stream")
			flusher := w.(http.Flusher)
			fmt.Fprint(w, "event: message\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\",\"params\":{\"p\":1}}\n\n")
			fmt.Fprint(w, "data: {\"jsonrpc\":\"2.0\",\"id\":\"srv-1\",\"method\":\"ping\"}\n\n")
			flusher.Flush()
			if mode == "sse-huge" {
				fmt.Fprint(w, "data: "+strings.Repeat("x", FrameLimit+10)+"\n\n")
				return
			}
			if mode == "sse-truncated" {
				return
			}
			fmt.Fprintf(w, "data: {\"jsonrpc\":\"2.0\",\"id\":%s,\n", message.ID)
			fmt.Fprint(w, "data: \"result\":{\"content\":[{\"type\":\"text\",\"text\":\"streamed\"}]}}\n\n")
			return
		}
		result = map[string]any{"content": []any{map[string]string{"type": "text", "text": "json"}}}
	default:
		http.Error(w, "unknown", http.StatusBadRequest)
		return
	}
	w.Header().Set("Content-Type", "application/json")
	json.NewEncoder(w).Encode(map[string]any{"jsonrpc": "2.0", "id": message.ID, "result": result})
}

type frameRecorder struct {
	mu     sync.Mutex
	frames []IOEvent
}

func (r *frameRecorder) Record(_ context.Context, event IOEvent) (string, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.frames = append(r.frames, event)
	return "", nil
}

func openFixture(t *testing.T, mode string, auth Authorizer) (*HTTPClient, *httpFixture, *frameRecorder, error) {
	t.Helper()
	fixture := &httpFixture{t: t, mode: mode}
	server := httptest.NewServer(fixture)
	t.Cleanup(server.Close)
	recorder := &frameRecorder{}
	client, err := OpenHTTP(context.Background(), Config{ID: "remote", HTTP: &HTTPConfig{URL: server.URL + "/mcp", Headers: map[string]string{"X-Api-Key": "header-secret-value"}, Auth: auth}, RequestTimeout: 5 * time.Second}, recorder)
	if client != nil {
		t.Cleanup(func() { _ = client.Close(context.Background()) })
	}
	return client, fixture, recorder, err
}

func TestStreamableHTTPJSONSessionAndHeaders(t *testing.T) {
	client, fixture, recorder, err := openFixture(t, "json", nil)
	if err != nil {
		t.Fatal(err)
	}
	tools, err := client.ListTools(context.Background())
	if err != nil || len(tools) != 2 || tools[0].Name != "echo" || tools[1].Name != "wait" {
		t.Fatal("paginated catalog failed", tools, err)
	}
	result, err := client.CallTool(context.Background(), "echo", json.RawMessage(`{}`))
	if err != nil || !strings.Contains(string(result), "json") {
		t.Fatal("JSON call failed", string(result), err)
	}
	fixture.mu.Lock()
	headers := fixture.headers
	fixture.mu.Unlock()
	for i, header := range headers {
		if header.Get("Accept") != "application/json, text/event-stream" || header.Get("Content-Type") != "application/json" || header.Get("X-Api-Key") != "header-secret-value" {
			t.Fatal("request lacks Streamable HTTP headers", i, header)
		}
		if i > 0 && (header.Get("Mcp-Session-Id") != "session-1" || header.Get("Mcp-Protocol-Version") != "2025-06-18") {
			t.Fatal("follow-up request lacks session or negotiated version", i, header)
		}
	}
	if err = client.Close(context.Background()); err != nil || fixture.deletes.Load() != 1 || !client.CleanupComplete() {
		t.Fatal("close did not end the server session", err)
	}
	recorder.mu.Lock()
	defer recorder.mu.Unlock()
	if len(recorder.frames) == 0 {
		t.Fatal("observer saw no frames")
	}
	for _, frame := range recorder.frames {
		if strings.Contains(string(frame.Frame), "header-secret-value") {
			t.Fatal("header value reached the I/O journal")
		}
	}
}

func TestStreamableHTTPEventStreamAnswersServerRequests(t *testing.T) {
	client, fixture, _, err := openFixture(t, "sse", nil)
	if err != nil {
		t.Fatal(err)
	}
	result, err := client.CallTool(context.Background(), "echo", json.RawMessage(`{}`))
	if err != nil || !strings.Contains(string(result), "streamed") {
		t.Fatal("event-stream response failed", string(result), err)
	}
	if !fixture.repliedPing.Load() {
		t.Fatal("server ping on the stream was not answered")
	}
}

func TestStreamableHTTPFailuresAreNotReplayed(t *testing.T) {
	for _, mode := range []string{"expired", "sse-huge", "sse-truncated"} {
		t.Run(mode, func(t *testing.T) {
			client, fixture, _, err := openFixture(t, mode, nil)
			if err != nil {
				t.Fatal(err)
			}
			if _, err = client.CallTool(context.Background(), "echo", json.RawMessage(`{}`)); err == nil {
				t.Fatal("failure reported success")
			}
			if _, err = client.CallTool(context.Background(), "echo", json.RawMessage(`{}`)); err == nil || fixture.calls.Load() != 1 {
				t.Fatal("failed worker accepted or replayed a call", fixture.calls.Load(), err)
			}
			if client.Connected() {
				t.Fatal("failed worker still reports connected")
			}
		})
	}
}

func TestStreamableHTTPRejectsLegacyTransport(t *testing.T) {
	if _, _, _, err := openFixture(t, "legacy", nil); !errors.Is(err, ErrLegacyTransport) {
		t.Fatal("legacy SSE server not reported", err)
	}
}

type scriptedAuth struct {
	header     atomic.Value
	challenged atomic.Int32
	refuse     bool
}

func (a *scriptedAuth) Header(context.Context) (string, error) {
	value, _ := a.header.Load().(string)
	return value, nil
}

func (a *scriptedAuth) Unauthorized(_ context.Context, status int, challenge []string) error {
	a.challenged.Add(1)
	if a.refuse || status != http.StatusUnauthorized || len(challenge) == 0 {
		return errors.New("re-authorization required in settings")
	}
	a.header.Store("Bearer fresh")
	return nil
}

func TestStreamableHTTPAuthorizationRetriesOnceThroughAuthorizer(t *testing.T) {
	if _, _, _, err := openFixture(t, "auth", nil); !errors.Is(err, ErrAuthorizationRequired) {
		t.Fatal("401 without authorizer did not ask for authorization", err)
	} else {
		var challenge *AuthorizationError
		if !errors.As(err, &challenge) || len(challenge.Challenge) == 0 || !strings.Contains(challenge.Challenge[0], "resource_metadata") {
			t.Fatal("challenge was not kept for discovery", err)
		}
	}
	auth := &scriptedAuth{}
	auth.header.Store("Bearer stale")
	client, _, _, err := openFixture(t, "auth", auth)
	if err != nil || auth.challenged.Load() != 1 {
		t.Fatal("refreshed credential was not retried exactly once", err, auth.challenged.Load())
	}
	if _, err = client.CallTool(context.Background(), "echo", json.RawMessage(`{}`)); err != nil {
		t.Fatal(err)
	}
	refusing := &scriptedAuth{refuse: true}
	refusing.header.Store("Bearer stale")
	if _, _, _, err = openFixture(t, "auth", refusing); !errors.Is(err, ErrAuthorizationRequired) || refusing.challenged.Load() != 1 {
		t.Fatal("refused authorization was retried or hidden", err)
	}
}

func TestStreamableHTTPDoesNotFollowRedirects(t *testing.T) {
	var reached atomic.Int32
	target := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { reached.Add(1) }))
	defer target.Close()
	redirect := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, target.URL+"/mcp", http.StatusTemporaryRedirect)
	}))
	defer redirect.Close()
	_, err := OpenHTTP(context.Background(), Config{ID: "redirect", HTTP: &HTTPConfig{URL: redirect.URL + "/mcp", Headers: map[string]string{"Authorization": "Bearer secret"}}}, nil)
	if err == nil || !strings.Contains(err.Error(), "redirect") || reached.Load() != 0 {
		t.Fatal("redirect was followed with credentials", err, reached.Load())
	}
}

func TestStreamableHTTPRejectsInvalidEndpoints(t *testing.T) {
	withUser := (&url.URL{Scheme: "https", Host: "example.test", Path: "/mcp", User: url.UserPassword("someone", "pw")}).String()
	for _, address := range []string{"ftp://example.test/mcp", withUser, "https://example.test/mcp#x", "not a url"} {
		if _, err := OpenHTTP(context.Background(), Config{ID: "bad", HTTP: &HTTPConfig{URL: address}}, nil); err == nil {
			t.Fatal("invalid endpoint accepted", address)
		}
	}
}
