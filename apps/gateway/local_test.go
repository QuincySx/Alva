// INPUT: Real loopback Gateway, local upstream HTTP and a deliberately uncooperative transport
// OUTPUT: Cold construction, separate secret/account consent, token rotation and truthful stop/retry ownership
// POS: [L6 application] Embedding reuses real protocol handlers and waits for actual upstream handlers.
package gateway

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/integration/llm/provider"
)

type localTransportFunc func(*http.Request) (*http.Response, error)

func (f localTransportFunc) RoundTrip(r *http.Request) (*http.Response, error) { return f(r) }
func localRequest(t *testing.T, c Credentials, path, raw string) *http.Response {
	t.Helper()
	method := http.MethodGet
	if raw != "" {
		method = http.MethodPost
	}
	r, err := http.NewRequest(method, c.Address+path, strings.NewReader(raw))
	if err != nil {
		t.Fatal(err)
	}
	r.Header.Set("Authorization", "Bearer "+c.Token)
	if raw != "" {
		r.Header.Set("Content-Type", "application/json")
	}
	client := http.Client{Timeout: 5 * time.Second}
	response, err := client.Do(r)
	if err != nil {
		t.Fatal(err)
	}
	return response
}
func TestEmbeddedGatewayColdConsentSecretRotationAndActualModelRequest(t *testing.T) {
	var calls atomic.Int32
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls.Add(1)
		w.Header().Set("Content-Type", "application/json")
		io.WriteString(w, `{"id":"fixture","model":"actual","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"real gateway response"},"finish_reason":"stop"}]}`)
	}))
	defer upstream.Close()
	l, err := NewLocal(map[string]Route{"explicit": {Kind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: upstream.URL, Model: "actual"}}})
	if err != nil {
		t.Fatal(err)
	}
	defer l.Close(context.Background())
	if l.Snapshot().Running || calls.Load() != 0 {
		t.Fatal("constructor started work")
	}
	if _, err = l.Start(context.Background(), false); err == nil {
		t.Fatal("model API consent inferred")
	}
	c, err := l.Start(context.Background(), true)
	if err != nil {
		t.Fatal(err)
	}
	status, _ := json.Marshal(l.Snapshot())
	if c.Generation == 0 || l.Snapshot().Generation != c.Generation || l.Snapshot().Closed || l.Snapshot().Stopping {
		t.Fatal("cold status lost the active listener identity")
	}
	if strings.Contains(string(status), c.Token) {
		t.Fatal("cold status exposed token")
	}
	response := localRequest(t, c, "/v1/models", "")
	response.Body.Close()
	if response.StatusCode != 200 || calls.Load() != 0 {
		t.Fatal("listing called model")
	}
	response = localRequest(t, c, "/v1/chat/completions", `{"model":"explicit","messages":[{"role":"user","content":"actual request"}]}`)
	raw, _ := io.ReadAll(response.Body)
	response.Body.Close()
	if response.StatusCode != 200 || !strings.Contains(string(raw), "real gateway response") || calls.Load() != 1 {
		t.Fatal("actual model API failed", response.StatusCode, string(raw))
	}
	if err = l.Stop(context.Background()); err != nil || l.Snapshot().Running || l.Snapshot().Generation != 0 || l.Snapshot().Stopping || l.Snapshot().Closed {
		t.Fatal("Stop did not reclaim listener", err)
	}
	next, err := l.Start(context.Background(), true)
	if err != nil || next.Token == c.Token || next.Generation <= c.Generation {
		t.Fatal("restart reused bearer", err)
	}
	old := next
	old.Token = c.Token
	response = localRequest(t, old, "/v1/models", "")
	response.Body.Close()
	if response.StatusCode != 401 {
		t.Fatal("previous bearer granted new listener")
	}
	if err = l.Close(context.Background()); err != nil {
		t.Fatal(err)
	}
	if !l.Snapshot().Closed || l.Snapshot().Running {
		t.Fatal("closed owner was shown as available")
	}
	if _, err = l.Start(context.Background(), true); err == nil {
		t.Fatal("closed owner restarted")
	}
}
func TestEmbeddedGatewayStopTimeoutRetainsExactOwnerAndSnapshotStaysResponsive(t *testing.T) {
	started, release := make(chan struct{}), make(chan struct{})
	var released sync.Once
	releaseTransport := func() { released.Do(func() { close(release) }) }
	defer releaseTransport()
	transport := localTransportFunc(func(r *http.Request) (*http.Response, error) {
		close(started)
		<-release
		return &http.Response{StatusCode: 200, Header: http.Header{"Content-Type": []string{"application/json"}}, Body: io.NopCloser(strings.NewReader(`{"id":"fixture","choices":[{"index":0,"message":{"role":"assistant","content":"late"},"finish_reason":"stop"}]}`)), Request: r}, nil
	})
	l, err := NewLocal(map[string]Route{"explicit": {Kind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: "https://fixture.invalid/chat", Model: "actual", HTTPClient: &http.Client{Transport: transport}}}})
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		releaseTransport()
		cleanup, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		if err := l.Close(cleanup); err != nil {
			t.Error(err)
		}
	}()
	c, err := l.Start(context.Background(), true)
	if err != nil {
		t.Fatal(err)
	}
	finished := make(chan struct{})
	go func() {
		defer close(finished)
		request, err := http.NewRequest(http.MethodPost, c.Address+"/v1/chat/completions", strings.NewReader(`{"model":"explicit","messages":[{"role":"user","content":"request"}]}`))
		if err != nil {
			t.Error(err)
			return
		}
		request.Header.Set("Authorization", "Bearer "+c.Token)
		request.Header.Set("Content-Type", "application/json")
		client := http.Client{Timeout: 5 * time.Second}
		response, err := client.Do(request)
		if err != nil {
			t.Error(err)
			return
		}
		response.Body.Close()
	}()
	select {
	case <-started:
	case <-time.After(3 * time.Second):
		t.Fatal("transport never entered")
	}
	deadline, cancel := context.WithTimeout(context.Background(), 50*time.Millisecond)
	defer cancel()
	if err = l.StopGeneration(deadline, c.Generation); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal("hung upstream falsely reaped", err)
	}
	if status := l.Snapshot(); !status.Running || !status.Stopping || status.Generation != c.Generation || status.Closed {
		t.Fatal("timeout forgot the stopped-admission cleanup owner", status)
	}
	response := localRequest(t, c, "/v1/models", "")
	response.Body.Close()
	if response.StatusCode != http.StatusServiceUnavailable {
		t.Fatal("timed-out Stop left server admission open", response.StatusCode)
	}
	if _, err = l.Start(context.Background(), true); err == nil {
		t.Fatal("second owner admitted during old cleanup")
	}
	releaseTransport()
	if err = l.StopGeneration(context.Background(), c.Generation); err != nil {
		t.Fatal("real cleanup retry failed", err)
	}
	select {
	case <-finished:
	case <-time.After(3 * time.Second):
		t.Fatal("request not joined")
	}
}

func TestEmbeddedGatewayGenerationStopRejectsDelayedAndUnknownOwners(t *testing.T) {
	l, err := NewLocal(map[string]Route{"explicit": {Kind: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: "http://127.0.0.1:1", Model: "actual"}}})
	if err != nil {
		t.Fatal(err)
	}
	defer l.Close(context.Background())
	previous, err := l.Start(t.Context(), true)
	if err != nil {
		t.Fatal(err)
	}
	if err = l.StopGeneration(t.Context(), 0); err == nil {
		t.Fatal("zero generation stopped a listener")
	}
	if err = l.StopGeneration(t.Context(), previous.Generation+1); err == nil || l.Snapshot().Stopping {
		t.Fatal("unknown generation changed listener admission", err)
	}
	// A rendered Stop can be waiting to reach its background owner while a
	// different window completes Stop and explicit Start.
	queued, release, finished := make(chan struct{}), make(chan struct{}), make(chan error, 1)
	var releaseOnce sync.Once
	releaseStop := func() { releaseOnce.Do(func() { close(release) }) }
	defer releaseStop()
	go func() {
		close(queued)
		<-release
		finished <- l.StopGeneration(t.Context(), previous.Generation)
	}()
	<-queued
	if err = l.Stop(t.Context()); err != nil {
		t.Fatal(err)
	}
	next, err := l.Start(t.Context(), true)
	if err != nil {
		t.Fatal(err)
	}
	releaseStop()
	select {
	case err = <-finished:
		if err == nil {
			t.Fatal("delayed Stop canceled a replacement listener")
		}
	case <-time.After(3 * time.Second):
		t.Fatal("delayed Stop did not settle")
	}
	if status := l.Snapshot(); !status.Running || status.Stopping || status.Generation != next.Generation {
		t.Fatal("stale Stop changed the replacement owner", status)
	}
	response := localRequest(t, next, "/v1/models", "")
	response.Body.Close()
	if response.StatusCode != http.StatusOK {
		t.Fatal("replacement server admission closed after stale Stop", response.StatusCode)
	}
	if err = l.StopGeneration(t.Context(), next.Generation); err != nil || l.Snapshot().Running {
		t.Fatal("current observed Stop did not join its listener", err)
	}
}
