// INPUT: Actual local HTTP endpoints, explicit raw transport configuration and failing SSE readers.
// OUTPUT: Gemini operation/auth parity, raw completion bounds and terminal/error preservation regressions.
// POS: [integration / model transport] Validates shared transport behavior independently of model conversion and paid provider accounts.
package provider

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/go/integration/llm/codec"
)

func TestGeminiRawTransportSelectsActualOperation(t *testing.T) {
	for _, configured := range []string{":generateContent", ":streamGenerateContent"} {
		t.Run(configured, func(t *testing.T) {
			var calls []string
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				if r.Header.Get("X-Goog-Api-Key") != "explicit-private" || r.Header.Get("Authorization") != "" {
					t.Error("wrong native authentication")
				}
				calls = append(calls, r.URL.RequestURI())
				if strings.HasSuffix(r.URL.Path, ":streamGenerateContent") {
					if r.URL.RawQuery != "alt=sse" || r.Header.Get("Accept") != "text/event-stream" {
						t.Error("wrong stream mode")
					}
					w.Header().Set("Content-Type", "text/event-stream")
					io.WriteString(w, "data: {\"candidates\":[{\"index\":0,\"finishReason\":\"STOP\"}]}\n\n")
				} else {
					if r.URL.RawQuery != "" || r.Header.Get("Accept") != "application/json" {
						t.Error("wrong complete mode")
					}
					w.Header().Set("Content-Type", "application/json")
					io.WriteString(w, `{"candidates":[{"index":0,"finishReason":"STOP"}]}`)
				}
			}))
			defer server.Close()
			transport, err := BuildTransport(GeminiKind, Config{Endpoint: server.URL + "/v1beta/models/actual" + configured, Model: "actual", APIKey: "explicit-private"})
			if err != nil {
				t.Fatal(err)
			}
			if _, err = transport.CompleteRaw(context.Background(), json.RawMessage(`{"contents":[]}`)); err != nil {
				t.Fatal(err)
			}
			if err = transport.StreamRaw(context.Background(), json.RawMessage(`{"contents":[]}`), func(codec.Event) (bool, error) { return true, nil }); err != nil {
				t.Fatal(err)
			}
			if len(calls) != 2 || calls[0] != "/v1beta/models/actual:generateContent" || calls[1] != "/v1beta/models/actual:streamGenerateContent?alt=sse" {
				t.Fatalf("operations changed authority/model: %#v", calls)
			}
		})
	}
}
func TestRawCompletionRejectsArrayWrongTypeAndOversize(t *testing.T) {
	for _, fixture := range []struct{ name, kind, body string }{{"array", "application/json", `[]`}, {"wrong type", "text/plain", `{}`}, {"oversize", "application/json", `{"x":"` + strings.Repeat("x", responseLimit) + `"}`}} {
		t.Run(fixture.name, func(t *testing.T) {
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				w.Header().Set("Content-Type", fixture.kind)
				io.WriteString(w, fixture.body)
			}))
			defer server.Close()
			transport, _ := BuildTransport(OpenAIChatKind, Config{Endpoint: server.URL, Model: "actual"})
			if _, err := transport.CompleteRaw(context.Background(), json.RawMessage(`{}`)); err == nil {
				t.Fatal("invalid native completion accepted")
			}
		})
	}
}
func TestRawStreamDoneDoesNotEraseCallbackOrAlreadyObservedReadFailure(t *testing.T) {
	callbackErr := errors.New("consumer unavailable")
	if err := readSSE(context.Background(), strings.NewReader("data: {}\n\n"), 1024, func(codec.Event) (bool, error) { return true, errors.Join(context.Canceled, callbackErr) }); !errors.Is(err, callbackErr) || !errors.Is(err, context.Canceled) {
		t.Fatal("terminal erased callback error", err)
	}
	readErr := errors.New("underlying read unavailable")
	reader := &lastReadFailure{data: []byte("data: {}\n\n"), err: readErr}
	if err := readSSE(context.Background(), reader, 1024, func(codec.Event) (bool, error) { return true, nil }); !errors.Is(err, readErr) {
		t.Fatal("terminal erased real read failure", err)
	}
}

type lastReadFailure struct {
	data []byte
	err  error
}

func (r *lastReadFailure) Read(p []byte) (int, error) {
	if len(r.data) == 0 {
		return 0, io.EOF
	}
	n := copy(p, r.data)
	r.data = r.data[n:]
	if len(r.data) == 0 {
		return n, r.err
	}
	return n, nil
}
func TestSSECountsExactRawCRLFBytesAndAcceptsStandaloneCR(t *testing.T) {
	frames := 0
	if err := readSSE(context.Background(), strings.NewReader("data: {}\r\rdata: {}\r\n\r\n"), 1024, func(codec.Event) (bool, error) { frames++; return false, nil }); err != nil || frames != 2 {
		t.Fatalf("CR framing failed: %d %v", frames, err)
	}
	stop := "data: {\"candidates\":[{\"index\":0,\"finishReason\":\"STOP\"}]}\r\n\r\n"
	oversize := stop + strings.Repeat(":\r\n", responseLimit/3+1)
	if err := readSSE(context.Background(), strings.NewReader(oversize), 1024, func(codec.Event) (bool, error) { return false, nil }); err == nil || !strings.Contains(err.Error(), "8 MiB") {
		t.Fatal("CRLF oversized tail accepted", err)
	}
	if err := readSSE(context.Background(), strings.NewReader("data: {}\n"), 1024, func(codec.Event) (bool, error) { t.Fatal("incomplete frame dispatched"); return true, nil }); err == nil {
		t.Fatal("unterminated frame accepted")
	}
}

func TestProviderRejectsCaseAliasedCredentialHeaders(t *testing.T) {
	_, err := BuildTransport(OpenAIChatKind, Config{Endpoint: "https://fixture.invalid/v1/chat/completions", Model: "fixture", Headers: map[string]string{"Authorization": "Bearer first", "authorization": "Bearer second"}})
	if err == nil {
		t.Fatal("ambiguous credential header accepted; map iteration can choose a different credential")
	}
}

func TestDefaultProviderDoesNotUseAmbientProxy(t *testing.T) {
	server := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		w.Write([]byte(`{"ok":true}`))
	}))
	defer server.Close()
	original := http.DefaultTransport
	fixtureTransport := server.Client().Transport.(*http.Transport).Clone()
	var proxyCalls atomic.Int32
	fixtureTransport.Proxy = func(*http.Request) (*url.URL, error) { proxyCalls.Add(1); return nil, nil }
	fixtureTransport.DialContext = func(ctx context.Context, _ string, _ string) (net.Conn, error) {
		return (&net.Dialer{}).DialContext(ctx, "tcp", server.Listener.Addr().String())
	}
	http.DefaultTransport = fixtureTransport
	defer func() { http.DefaultTransport = original; fixtureTransport.CloseIdleConnections() }()
	for _, client := range []*http.Client{nil, {Timeout: time.Minute}} {
		transport, err := BuildTransport(OpenAIChatKind, Config{Endpoint: "https://example.com/v1/chat/completions", Model: "fixture", HTTPClient: client})
		if err != nil {
			t.Fatal(err)
		}
		if _, err := transport.CompleteRaw(t.Context(), json.RawMessage(`{}`)); err != nil {
			t.Fatal(err)
		}
		if proxyCalls.Load() != 0 {
			t.Fatal("default request consulted an ambient proxy instead of explicit transport", proxyCalls.Load())
		}
		transport.client.CloseIdleConnections()
	}
}
