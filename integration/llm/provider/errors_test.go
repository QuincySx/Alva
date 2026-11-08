// INPUT: Real loopback status responses and bounded HTTP retry metadata fixtures
// OUTPUT: Four-protocol typed status/identity and no-private-body/no-retry regressions
// POS: [integration / model transport] Transport classifications without live accounts or automatic fallback/replay
package provider

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	agentabi "github.com/QuincySx/Alva/sdk/agent/abi"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestAllActualTransportsPreserveTypedFailureWithoutRetryOrSecrets(t *testing.T) {
	for _, kind := range []Kind{OpenAIChatKind, OpenAIResponsesKind, AnthropicKind, GeminiKind} {
		for _, status := range []int{401, 403, 400, 413, 429, 503, 529} {
			t.Run(fmt.Sprintf("%s/%d", kind, status), func(t *testing.T) {
				var calls atomic.Int32
				upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
					calls.Add(1)
					w.Header().Set("Retry-After", "7")
					w.WriteHeader(status)
					_, _ = w.Write([]byte(`{"api_key":"PRIVATE-PROVIDER-BODY"}`))
				}))
				defer upstream.Close()
				model, err := Build(kind, Config{Endpoint: upstream.URL, Model: "actual-model"})
				if err != nil {
					t.Fatal(err)
				}
				err = model.Stream(context.Background(), abi.ModelRequest{Messages: []wire.Message{wire.TextMessage(wire.RoleUser, "hello")}}, func(wire.StreamChunk) error { return nil })
				var typed *agentabi.ProviderError
				if !errors.As(err, &typed) || typed.StatusCode == nil || *typed.StatusCode != status {
					t.Fatal("lost actual provider status", err)
				}
				if strings.Contains(err.Error(), "PRIVATE-PROVIDER-BODY") || typed.ResponseBody != "" || typed.URL != "" || calls.Load() != 1 {
					t.Fatal("secret body retained/echoed or request retried")
				}
				if status == 429 && (typed.Kind != agentabi.ProviderRateLimited || !typed.Retryable || typed.RetryAfter == nil || *typed.RetryAfter != 7*time.Second) {
					t.Fatal("rate-limit evidence lost")
				}
				if (status == 401 || status == 403) && (typed.Kind != agentabi.ProviderCredential || typed.Retryable) {
					t.Fatal("authentication made retryable")
				}
				identity, ok := model.(abi.ModelIdentity)
				if !ok || identity.ProviderID() != string(kind) || identity.ModelID() != "actual-model" {
					t.Fatal("analytics identity is not actual configured model")
				}
			})
		}
	}
}

func TestRetryAfterBoundedSecondsAndHTTPDate(t *testing.T) {
	now := time.Date(2026, 10, 16, 0, 0, 0, 0, time.UTC)
	for _, text := range []string{"-1", "999999999999999999999999999", "86401", "nonsense", strings.Repeat("x", 129)} {
		if retryAfter(text, now) != nil {
			t.Fatalf("invalid/unbounded retry-after accepted %q", text)
		}
	}
	when := now.Add(8 * time.Second).Format(http.TimeFormat)
	if value := retryAfter(when, now); value == nil || *value != 8*time.Second {
		t.Fatal("HTTP date not preserved")
	}
	if value := retryAfter(now.Add(-time.Second).Format(http.TimeFormat), now); value == nil || *value != 0 {
		t.Fatal("past retry date not clamped")
	}
}

// Context-length rejection is classified only from OpenAI's documented
// structured error code; Anthropic/Gemini message text is never interpreted.
func TestContextLengthRejectionUsesOnlyTheStructuredCode(t *testing.T) {
	openAIBody := `{"error":{"message":"This model's maximum context length is 8192 tokens. PRIVATE-PROVIDER-BODY","type":"invalid_request_error","param":"messages","code":"context_length_exceeded"}}`
	cases := []struct {
		name string
		kind Kind
		body string
		want agentabi.ProviderErrorKind
	}{
		{"chat code", OpenAIChatKind, openAIBody, agentabi.ProviderContextLength},
		{"responses code", OpenAIResponsesKind, openAIBody, agentabi.ProviderContextLength},
		{"chat other code", OpenAIChatKind, `{"error":{"message":"context length exceeded","code":"invalid_value"}}`, agentabi.ProviderInvalidArgument},
		{"chat message only", OpenAIChatKind, `{"error":{"message":"maximum context length exceeded"}}`, agentabi.ProviderInvalidArgument},
		{"chat numeric code", OpenAIChatKind, `{"error":{"code":400}}`, agentabi.ProviderInvalidArgument},
		{"chat oversized body", OpenAIChatKind, `{"error":{"code":"context_length_exceeded","pad":"` + strings.Repeat("x", 70<<10) + `"}}`, agentabi.ProviderInvalidArgument},
		{"anthropic message", AnthropicKind, `{"type":"error","error":{"type":"invalid_request_error","message":"prompt is too long: 210000 tokens > 200000 maximum"}}`, agentabi.ProviderInvalidArgument},
		{"anthropic borrowed code", AnthropicKind, openAIBody, agentabi.ProviderInvalidArgument},
		{"gemini message", GeminiKind, `{"error":{"code":400,"message":"The input token count exceeds the maximum","status":"INVALID_ARGUMENT"}}`, agentabi.ProviderInvalidArgument},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			upstream := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				w.WriteHeader(http.StatusBadRequest)
				_, _ = w.Write([]byte(c.body))
			}))
			defer upstream.Close()
			model, err := Build(c.kind, Config{Endpoint: upstream.URL, Model: "actual-model"})
			if err != nil {
				t.Fatal(err)
			}
			err = model.Stream(context.Background(), abi.ModelRequest{Messages: []wire.Message{wire.TextMessage(wire.RoleUser, "hello")}}, func(wire.StreamChunk) error { return nil })
			var typed *agentabi.ProviderError
			if !errors.As(err, &typed) || typed.Kind != c.want || typed.Retryable {
				t.Fatal("wrong context-length classification", err)
			}
			if strings.Contains(err.Error(), "PRIVATE-PROVIDER-BODY") || typed.ResponseBody != "" {
				t.Fatal("provider body leaked into the error")
			}
		})
	}
}
