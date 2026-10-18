// INPUT: Explicit local upstream fixtures, real Magpie codec and authenticated HTTP requests
// OUTPUT: Gateway native/cross-protocol, terminal/loss, cancellation and admission regression coverage
// POS: [L6 application] Verify actual model transport without paid accounts, ambient credentials or native session data
package gateway

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/integration/llm/provider"
	"github.com/QuincySx/Alva/platform/system/loopback"
)

type gatewayFixture struct {
	server  *httptest.Server
	handler *Server
	token   string
}

func newGateway(t *testing.T, kind provider.Kind, upstream *httptest.Server, observer func(context.Context, Observation) error) *gatewayFixture {
	t.Helper()
	token, err := loopback.NewToken()
	if err != nil {
		t.Fatal(err)
	}
	server := httptest.NewUnstartedServer(nil)
	handler, err := New(Config{Authority: server.Listener.Addr().String(), Token: token,
		Routes: map[string]Route{"alias": {Kind: kind, Provider: provider.Config{Endpoint: upstream.URL + "/model", Model: "actual-model", APIKey: "private-upstream-key", MaxOutputTokens: 4096}}}, Observe: observer})
	if err != nil {
		server.Close()
		t.Fatal(err)
	}
	server.Config.Handler = handler
	server.Start()
	f := &gatewayFixture{server, handler, token}
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
		defer cancel()
		if err := handler.Close(ctx); err != nil {
			t.Error(err)
		}
		server.Close()
	})
	return f
}

func (f *gatewayFixture) request(t *testing.T, path, body string) *http.Response {
	t.Helper()
	r, err := http.NewRequest(http.MethodPost, f.server.URL+path, strings.NewReader(body))
	if err != nil {
		t.Fatal(err)
	}
	r.Header.Set("Authorization", "Bearer "+f.token)
	r.Header.Set("Content-Type", "application/json")
	r.Header.Set("X-Caller-Secret", "never-forward")
	response, err := f.server.Client().Do(r)
	if err != nil {
		t.Fatal(err)
	}
	return response
}

func gatewayBody(t *testing.T, response *http.Response, status int) []byte {
	t.Helper()
	defer response.Body.Close()
	body, err := io.ReadAll(response.Body)
	if err != nil {
		t.Fatal(err)
	}
	if response.StatusCode != status {
		t.Fatalf("HTTP %d, want %d: %s", response.StatusCode, status, body)
	}
	return body
}

func TestNativeProtocolsKeepOpaqueFieldsAndConfiguredIdentity(t *testing.T) {
	tests := []struct {
		kind                    provider.Kind
		path, request, response string
	}{
		{provider.OpenAIChatKind, "/v1/chat/completions", `{"model":"alias","messages":[{"role":"user","content":"hi"}],"opaque_vendor_field":{"retain":true}}`, `{"id":"chat-1","model":"actual-model","choices":[{"index":0,"message":{"role":"assistant","content":"hello"},"finish_reason":"stop"}],"opaque_response":{"retain":true}}`},
		{provider.OpenAIResponsesKind, "/v1/responses", `{"model":"alias","input":"hi","opaque_vendor_field":{"retain":true}}`, `{"id":"response-1","model":"actual-model","object":"response","status":"completed","output":[{"type":"message","id":"m1","role":"assistant","status":"completed","content":[{"type":"output_text","text":"hello","annotations":[]}]}],"opaque_response":{"retain":true}}`},
		{provider.AnthropicKind, "/v1/messages", `{"model":"alias","max_tokens":32,"messages":[{"role":"user","content":"hi"}],"opaque_vendor_field":{"retain":true}}`, `{"id":"message-1","model":"actual-model","type":"message","role":"assistant","content":[{"type":"text","text":"hello"}],"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":2},"opaque_response":{"retain":true}}`},
		{provider.GeminiKind, "/v1beta/models/alias:generateContent", `{"contents":[{"role":"user","parts":[{"text":"hi"}]}],"opaque_vendor_field":{"retain":true}}`, `{"modelVersion":"actual-native-version","candidates":[{"index":0,"content":{"role":"model","parts":[{"text":"hello"}]},"finishReason":"STOP"}],"opaque_response":{"retain":true}}`},
	}
	for _, test := range tests {
		t.Run(string(test.kind), func(t *testing.T) {
			var calls atomic.Int32
			upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				calls.Add(1)
				if r.Header.Get("X-Caller-Secret") != "" {
					t.Error("forwarded caller header")
				}
				if r.Header.Get("Accept") != "application/json" {
					t.Error("wrong nonstream accept")
				}
				keyHeader, key := "Authorization", "Bearer private-upstream-key"
				if test.kind == provider.AnthropicKind {
					keyHeader, key = "X-Api-Key", "private-upstream-key"
				}
				if test.kind == provider.GeminiKind {
					keyHeader, key = "X-Goog-Api-Key", "private-upstream-key"
				}
				if r.Header.Get(keyHeader) != key {
					t.Error("route credentials not used")
				}
				body, _ := io.ReadAll(r.Body)
				var value map[string]json.RawMessage
				if json.Unmarshal(body, &value) != nil || string(value["opaque_vendor_field"]) != `{"retain":true}` {
					t.Errorf("native fields lost: %s", body)
				}
				if test.kind != provider.GeminiKind && string(value["model"]) != `"actual-model"` {
					t.Errorf("model route not pinned: %s", body)
				}
				w.Header().Set("Content-Type", "application/json")
				_, _ = io.WriteString(w, test.response)
			}))
			t.Cleanup(upstream.Close)
			var mu sync.Mutex
			var observations []Observation
			f := newGateway(t, test.kind, upstream, func(_ context.Context, value Observation) error {
				mu.Lock()
				defer mu.Unlock()
				observations = append(observations, value)
				return nil
			})
			response := f.request(t, test.path, test.request)
			body := gatewayBody(t, response, 200)
			if !bytes.Contains(body, []byte(`"opaque_response":{"retain":true}`)) {
				t.Fatalf("opaque response lost: %s", body)
			}
			if test.kind == provider.GeminiKind {
				if !bytes.Contains(body, []byte(`"modelVersion":"actual-native-version"`)) {
					t.Fatalf("model provenance overwritten: %s", body)
				}
			} else if !bytes.Contains(body, []byte(`"model":"alias"`)) {
				t.Fatalf("route alias missing: %s", body)
			}
			if calls.Load() != 1 || response.Header.Get("X-Alva-Request-Converter") != "native_identity" || response.Header.Get("X-Alva-Request-Steps") != "0" {
				t.Fatal("identity route accidentally used a conversion hop")
			}
			mu.Lock()
			defer mu.Unlock()
			if len(observations) != 2 || observations[0].Stage != "request" || observations[1].Stage != "response" {
				t.Fatalf("bad observations: %+v", observations)
			}
			for _, value := range observations {
				if len(value.SHA256) != 64 || value.Converter != "native_identity" || value.Alias != "alias" || len(value.Steps) != 0 {
					t.Fatalf("bad metadata evidence: %+v", value)
				}
			}
		})
	}
}

func TestMagpieToolRoundTripUsesActualIDsWithoutExecutingTools(t *testing.T) {
	var calls atomic.Int32
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		n := calls.Add(1)
		body, _ := io.ReadAll(r.Body)
		var request map[string]any
		if json.Unmarshal(body, &request) != nil || request["model"] != "actual-model" {
			t.Errorf("bad actual Claude request: %s", body)
		}
		if n == 1 && !bytes.Contains(body, []byte(`"input_schema"`)) {
			t.Errorf("tool schema not converted by Magpie: %s", body)
		}
		if n == 2 && (!bytes.Contains(body, []byte(`"tool_use_id":"call-actual-1"`)) || !bytes.Contains(body, []byte(`"type":"tool_result"`))) {
			t.Errorf("tool result identity lost: %s", body)
		}
		w.Header().Set("Content-Type", "application/json")
		if n == 1 {
			_, _ = io.WriteString(w, `{"id":"m1","type":"message","role":"assistant","model":"actual-model","content":[{"type":"tool_use","id":"call-actual-1","name":"lookup","input":{"q":"hello"}}],"stop_reason":"tool_use","usage":{"input_tokens":1,"output_tokens":2}}`)
		} else {
			_, _ = io.WriteString(w, `{"id":"m2","type":"message","role":"assistant","model":"actual-model","content":[{"type":"text","text":"done"}],"stop_reason":"end_turn","usage":{"input_tokens":3,"output_tokens":4}}`)
		}
	}))
	t.Cleanup(upstream.Close)
	f := newGateway(t, provider.AnthropicKind, upstream, nil)
	first := f.request(t, "/v1/chat/completions", `{"model":"alias","messages":[{"role":"user","content":"lookup"}],"tools":[{"type":"function","function":{"name":"lookup","description":"lookup text","parameters":{"type":"object","properties":{"q":{"type":"string"}}}}}]}`)
	body := gatewayBody(t, first, 200)
	if first.Header.Get("X-Alva-Request-Steps") == "0" || first.Header.Get("X-Alva-Request-Converter") == "native_identity" {
		t.Fatal("cross-protocol route did not use Magpie")
	}
	var response struct {
		Choices []struct {
			Message struct {
				Calls []struct {
					ID       string
					Function struct{ Name, Arguments string }
				} `json:"tool_calls"`
			}
		}
	}
	if json.Unmarshal(body, &response) != nil || len(response.Choices) != 1 || len(response.Choices[0].Message.Calls) != 1 {
		t.Fatalf("bad tool response: %s", body)
	}
	call := response.Choices[0].Message.Calls[0]
	if call.ID != "call-actual-1" || call.Function.Name != "lookup" || call.Function.Arguments != `{"q":"hello"}` {
		t.Fatalf("tool identity/arguments changed: %s", body)
	}
	second := f.request(t, "/v1/chat/completions", `{"model":"alias","messages":[{"role":"user","content":"lookup"},{"role":"assistant","content":null,"tool_calls":[{"id":"call-actual-1","type":"function","function":{"name":"lookup","arguments":"{\"q\":\"hello\"}"}}]},{"role":"tool","tool_call_id":"call-actual-1","content":"found"}]}`)
	if !bytes.Contains(gatewayBody(t, second, 200), []byte(`"done"`)) || calls.Load() != 2 {
		t.Fatal("tool result round trip failed")
	}
}

func chatSSE(terminal bool) string {
	text := "data: {\"id\":\"c1\",\"model\":\"actual-model\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"hello\"},\"finish_reason\":null}]}\n\n"
	text += "data: {\"id\":\"c1\",\"model\":\"actual-model\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n"
	if terminal {
		text += "data: [DONE]\n\n"
	}
	return text
}

func TestStreamRequiresRealTerminalAndKeepsGeminiUsageTail(t *testing.T) {
	for _, terminal := range []bool{false, true} {
		t.Run(fmt.Sprintf("chat-terminal-%t", terminal), func(t *testing.T) {
			upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				w.Header().Set("Content-Type", "text/event-stream")
				_, _ = io.WriteString(w, chatSSE(terminal))
			}))
			t.Cleanup(upstream.Close)
			f := newGateway(t, provider.OpenAIChatKind, upstream, nil)
			body := gatewayBody(t, f.request(t, "/v1/chat/completions", `{"model":"alias","stream":true,"messages":[{"role":"user","content":"hi"}]}`), 200)
			if !bytes.Contains(body, []byte("hello")) {
				t.Fatalf("partial text lost: %s", body)
			}
			if bytes.Contains(body, []byte("[DONE]")) != terminal {
				t.Fatalf("fabricated/lost DONE: %s", body)
			}
			if !terminal && (bytes.Contains(body, []byte(`"finish_reason":"stop"`)) || !bytes.Contains(body, []byte("Bad Gateway"))) {
				t.Fatalf("truncation presented as success: %s", body)
			}
		})
	}
	t.Run("Gemini-STOP-then-usage", func(t *testing.T) {
		upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			w.Header().Set("Content-Type", "text/event-stream")
			_, _ = io.WriteString(w, "data: {\"modelVersion\":\"native-version\",\"candidates\":[{\"index\":0,\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"hello\"}]},\"finishReason\":\"STOP\"}]}\n\n")
			_, _ = io.WriteString(w, "data: {\"responseId\":\"tail-id\",\"usageMetadata\":{\"promptTokenCount\":5,\"candidatesTokenCount\":2,\"cachedContentTokenCount\":0}}\n\n")
		}))
		t.Cleanup(upstream.Close)
		f := newGateway(t, provider.GeminiKind, upstream, nil)
		body := gatewayBody(t, f.request(t, "/v1beta/models/alias:streamGenerateContent?alt=sse", `{"contents":[{"role":"user","parts":[{"text":"hi"}]}]}`), 200)
		if !bytes.Contains(body, []byte(`"responseId":"tail-id"`)) || !bytes.Contains(body, []byte(`"cachedContentTokenCount":0`)) || bytes.Contains(body, []byte("Bad Gateway")) {
			t.Fatalf("Gemini tail discarded: %s", body)
		}
	})
}

func TestLossAndPrivateUpstreamFailuresAreNotSuccessfulOrDisclosed(t *testing.T) {
	tests := []struct {
		name     string
		kind     provider.Kind
		response string
		status   int
	}{
		{"signed-reasoning", provider.AnthropicKind, `{"id":"m1","type":"message","role":"assistant","model":"actual-model","content":[{"type":"thinking","thinking":"private-thinking","signature":"private-signature"},{"type":"text","text":"hello"}],"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":2}}`, 200},
		{"signature-case-alias", provider.AnthropicKind, `{"id":"m1","type":"message","role":"assistant","model":"actual-model","content":[{"Type":"thinking","thinking":"private-thinking","Signature":"private-signature"},{"type":"text","text":"hello"}],"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":2}}`, 200},
		{"large-tool-integer", provider.AnthropicKind, `{"id":"m1","type":"message","role":"assistant","model":"actual-model","content":[{"type":"tool_use","id":"call1","name":"lookup","input":{"order_id":9007199254740993}}],"stop_reason":"tool_use","usage":{"input_tokens":1,"output_tokens":2}}`, 200},
		{"not-completed", provider.OpenAIChatKind, `{"id":"c1","choices":[{"index":0,"message":{"role":"assistant","content":"private-partial"},"finish_reason":"length"}]}`, 200},
		{"upstream-error", provider.OpenAIChatKind, `{"error":{"message":"private-upstream-key private-prompt"}}`, 429},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				w.Header().Set("Content-Type", "application/json")
				w.WriteHeader(test.status)
				_, _ = io.WriteString(w, test.response)
			}))
			t.Cleanup(upstream.Close)
			f := newGateway(t, test.kind, upstream, nil)
			body := gatewayBody(t, f.request(t, "/v1/chat/completions", `{"model":"alias","messages":[{"role":"user","content":"hi"}]}`), 502)
			if bytes.Contains(body, []byte("private-")) || bytes.Contains(body, []byte(upstream.URL)) {
				t.Fatalf("private diagnostic leaked: %s", body)
			}
		})
	}
}

func TestCrossProtocolSSEUsesNativeResponseEventsAndNoSyntheticSuccess(t *testing.T) {
	for _, terminal := range []bool{false, true} {
		t.Run(fmt.Sprint(terminal), func(t *testing.T) {
			upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				w.Header().Set("Content-Type", "text/event-stream")
				_, _ = io.WriteString(w, chatSSE(terminal))
			}))
			t.Cleanup(upstream.Close)
			f := newGateway(t, provider.OpenAIChatKind, upstream, nil)
			body := gatewayBody(t, f.request(t, "/v1/responses", `{"model":"alias","stream":true,"input":"hi"}`), 200)
			if !bytes.Contains(body, []byte("event: response.output_text.delta")) || !bytes.Contains(body, []byte("hello")) {
				t.Fatalf("wrong native destination events: %s", body)
			}
			if bytes.Contains(body, []byte("event: response.completed")) != terminal || bytes.Contains(body, []byte(`"Payload"`)) {
				t.Fatalf("internal wrapper or fabricated completion: %s", body)
			}
			if !terminal && !bytes.Contains(body, []byte("event: error")) {
				t.Fatalf("lost interrupted-stream error: %s", body)
			}
		})
	}
}

func TestCrossProtocolContinuationRequestFailsBeforeBilling(t *testing.T) {
	var calls atomic.Int32
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { calls.Add(1) }))
	t.Cleanup(upstream.Close)
	f := newGateway(t, provider.OpenAIChatKind, upstream, nil)
	body := gatewayBody(t, f.request(t, "/v1/responses", `{"model":"alias","input":"hi","previous_response_id":"private-state"}`), 400)
	if calls.Load() != 0 || bytes.Contains(body, []byte("private-state")) {
		t.Fatal("lost-state request called model or disclosed details")
	}
	// A raw JSON argument string must not become a rounded float64 before the
	// upstream request. This is real transport admission, not only a codec unit test.
	f = newGateway(t, provider.AnthropicKind, upstream, nil)
	body = gatewayBody(t, f.request(t, "/v1/chat/completions", `{"model":"alias","messages":[{"role":"assistant","tool_calls":[{"id":"call1","type":"function","function":{"name":"lookup","arguments":"{\"order_id\":9007199254740993}"}}]},{"role":"tool","tool_call_id":"call1","content":"found"}]}`), 400)
	if calls.Load() != 0 || bytes.Contains(body, []byte("9007199254740993")) {
		t.Fatal("rounded argument request called model or disclosed data")
	}
}

func TestAdmissionAndObservationFailureDoNotCallModel(t *testing.T) {
	var calls atomic.Int32
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		calls.Add(1)
		t.Error("inadmissible request reached upstream")
	}))
	t.Cleanup(upstream.Close)
	f := newGateway(t, provider.OpenAIChatKind, upstream, nil)
	for _, body := range []string{
		`{"model":"alias","model":"alias","messages":[]}`,
		`{"model":"alias","stream":null,"messages":[]}`,
		`{"model":"alias","Stream":true,"messages":[]}`,
		`{"model":"alias","ſtream":true,"messages":[]}`,
		`{"model":"alias","Model":"another-route","messages":[]}`,
		`{"model":"unknown","messages":[]}`,
		`{"model":"alias","messages":[],"x":"\ud800"}`,
	} {
		response := f.request(t, "/v1/chat/completions", body)
		if response.StatusCode < 400 {
			t.Errorf("invalid body admitted: %s", body)
		}
		response.Body.Close()
	}
	request, _ := http.NewRequest(http.MethodGet, f.server.URL+"/v1/models", nil)
	request.Header.Set("Authorization", "Bearer wrong")
	response, err := f.server.Client().Do(request)
	if err != nil {
		t.Fatal(err)
	}
	_ = gatewayBody(t, response, 401)
	request.Header.Set("Authorization", "Bearer "+f.token)
	request.Header.Set("Origin", "https://untrusted.invalid")
	response, err = f.server.Client().Do(request)
	if err != nil {
		t.Fatal(err)
	}
	_ = gatewayBody(t, response, 403)
	failing := newGateway(t, provider.OpenAIChatKind, upstream, func(context.Context, Observation) error { return fmt.Errorf("private-observer-failure") })
	body := gatewayBody(t, failing.request(t, "/v1/chat/completions", `{"model":"alias","messages":[]}`), 500)
	if bytes.Contains(body, []byte("private-")) || calls.Load() != 0 {
		t.Fatal("observer failure leaked or called model")
	}
}

func TestCloseCancelsActualUpstreamAndStopsAdmission(t *testing.T) {
	entered, exited := make(chan struct{}), make(chan struct{})
	upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/event-stream")
		_, _ = io.WriteString(w, "data: {\"id\":\"c1\",\"model\":\"actual-model\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"started\"},\"finish_reason\":null}]}\n\n")
		w.(http.Flusher).Flush()
		close(entered)
		<-r.Context().Done()
		close(exited)
	}))
	t.Cleanup(upstream.Close)
	f := newGateway(t, provider.OpenAIChatKind, upstream, nil)
	response := f.request(t, "/v1/chat/completions", `{"model":"alias","stream":true,"messages":[{"role":"user","content":"hi"}]}`)
	defer response.Body.Close()
	reader := bufio.NewReader(response.Body)
	if line, err := reader.ReadString('\n'); err != nil || !strings.Contains(line, "started") {
		t.Fatalf("no active upstream: %q %v", line, err)
	}
	<-entered
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err := f.handler.Close(ctx); err != nil {
		t.Fatal(err)
	}
	select {
	case <-exited:
	case <-ctx.Done():
		t.Fatal("actual upstream request not canceled")
	}
	_ = gatewayBody(t, f.request(t, "/v1/chat/completions", `{"model":"alias","messages":[]}`), 503)
}
