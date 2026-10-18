// INPUT: Real local provider HTTP errors, deterministic harnesses and joined execution failures
// OUTPUT: Typed fallback classifications, startup terminal facts and mixed-cause/cancel regressions
// POS: [L7 Alva product adapter] Offline adapter safety tests; no paid requests, automatic retries or external credentials
package alva

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"net/url"
	"sync/atomic"
	"testing"

	"github.com/QuincySx/Alva/go/harness"
	"github.com/QuincySx/Alva/go/integration/engine"
	"github.com/QuincySx/Alva/go/integration/llm/provider"
	agentabi "github.com/QuincySx/Alva/go/sdk/agent/abi"
	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

func statusProvider(kind agentabi.ProviderErrorKind, status int, retryable bool) *agentabi.ProviderError {
	return &agentabi.ProviderError{Kind: kind, StatusCode: &status, Retryable: retryable, Message: "fixture provider evidence"}
}

type cyclicFailure struct{}

func (e *cyclicFailure) Error() string { return "cyclic fixture" }
func (e *cyclicFailure) Unwrap() error { return e }

func TestProviderFailureRequiresUnmixedEvidence(t *testing.T) {
	limited := statusProvider(agentabi.ProviderRateLimited, 429, true)
	network := &agentabi.ProviderError{Kind: agentabi.ProviderNetwork, Retryable: true, Err: &url.Error{Op: "Post", Err: errors.New("offline fixture")}}
	storage := &abi.SessionError{Operation: "flush", SessionID: "native", Err: context.DeadlineExceeded}
	for _, test := range []struct {
		name string
		err  error
		want engine.FailureKind
	}{
		{"429", limited, engine.FailureRateLimit},
		{"401", statusProvider(agentabi.ProviderCredential, 401, false), engine.FailureAuth},
		{"403", statusProvider(agentabi.ProviderCredential, 403, false), engine.FailureAuth},
		{"pre-response 503", statusProvider(agentabi.ProviderAPICall, 503, true), engine.FailureStartup},
		{"pre-response network", network, engine.FailureStartup},
		{"wrapper", fmt.Errorf("provider failed: %w", limited), engine.FailureRateLimit},
		{"same joined category", errors.Join(limited, statusProvider(agentabi.ProviderRateLimited, 429, true)), engine.FailureRateLimit},
		{"mixed categories", errors.Join(limited, network), engine.FailureExecution},
		{"flush sibling", errors.Join(limited, storage), engine.FailureExecution},
		{"cleanup sibling", errors.Join(network, errors.New("resource close failed")), engine.FailureExecution},
		{"approval sibling", errors.Join(limited, errors.New("approval failed")), engine.FailureExecution},
		{"cancel sibling", errors.Join(limited, context.Canceled), engine.FailureExecution},
		{"session wrapper", &abi.SessionError{Operation: "append", SessionID: "native", Err: limited}, engine.FailureExecution},
		{"network hides aggregate", &agentabi.ProviderError{Kind: agentabi.ProviderNetwork, Retryable: true, Err: errors.Join(errors.New("transport"), errors.New("approval"))}, engine.FailureExecution},
		{"network hides session", &agentabi.ProviderError{Kind: agentabi.ProviderNetwork, Retryable: true, Err: fmt.Errorf("transport: %w", storage)}, engine.FailureExecution},
		{"network hides cancellation", &agentabi.ProviderError{Kind: agentabi.ProviderNetwork, Retryable: true, Err: context.Canceled}, engine.FailureExecution},
		{"nonretryable 503", statusProvider(agentabi.ProviderAPICall, 503, false), engine.FailureExecution},
		{"wrong 429 kind", statusProvider(agentabi.ProviderAPICall, 429, true), engine.FailureExecution},
		{"unknown API status", statusProvider(agentabi.ProviderAPICall, 409, true), engine.FailureExecution},
		{"SSE parse", &agentabi.ProviderError{Kind: agentabi.ProviderJSONParse}, engine.FailureExecution},
		{"plain error", errors.New("429 is not typed evidence"), engine.FailureExecution},
		{"cyclic wrapper", &cyclicFailure{}, engine.FailureExecution},
	} {
		t.Run(test.name, func(t *testing.T) {
			outcome, got := executionFailure(context.Background(), test.err, false)
			if outcome != engine.Failed || got != test.want {
				t.Fatalf("got %s/%s, want failed/%s", outcome, got, test.want)
			}
		})
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	for _, test := range []struct {
		err     error
		outcome engine.Outcome
		failure engine.FailureKind
	}{
		{context.Canceled, engine.Cancelled, engine.FailureCancelled},
		{errors.Join(context.Canceled, fmt.Errorf("wait: %w", context.DeadlineExceeded)), engine.Cancelled, engine.FailureCancelled},
		{errors.Join(context.Canceled, storage), engine.Failed, engine.FailureExecution},
	} {
		if outcome, failure := executionFailure(ctx, test.err, false); outcome != test.outcome || failure != test.failure {
			t.Fatal("cancellation erased strong failure", outcome, failure)
		}
	}
	if _, failure := executionFailure(context.Background(), network, true); failure != engine.FailureExecution {
		t.Fatal("observed model response falsely classified as startup")
	}
}

func TestFactoryFailureEmitsRealFatalAndTerminalWithoutSessionStarted(t *testing.T) {
	fixture := statusProvider(agentabi.ProviderRateLimited, 429, true)
	adapter := New(func(context.Context, string, engine.Request, bool) (harness.Runner, error) { return nil, fixture })
	var events []engine.Event
	var contract engine.StreamContract
	err := adapter.Execute(context.Background(), engine.Request{Prompt: "fixture"}, func(event engine.Event) error {
		events = append(events, event)
		return contract.Accept(event)
	})
	if !errors.Is(err, fixture) || len(events) != 2 || events[0].Kind != engine.Error || events[0].Failure != engine.FailureRateLimit || events[0].Recoverable || events[1].Kind != engine.Completed || events[1].Outcome != engine.Failed || contract.Started || !contract.Terminal {
		t.Fatal("factory evidence lost or successful startup fabricated", events, err)
	}
}

func TestActualHTTPProviderErrorsReachEngineClassification(t *testing.T) {
	for _, status := range []int{429, 401, 403, 503, 400} {
		t.Run(fmt.Sprint(status), func(t *testing.T) {
			var requests atomic.Int32
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				requests.Add(1)
				w.WriteHeader(status)
				_, _ = w.Write([]byte(`{"error":"private-response-must-not-reach-engine"}`))
			}))
			defer server.Close()
			adapter := New(func(ctx context.Context, id string, _ engine.Request, _ bool) (harness.Runner, error) {
				model, err := provider.Build(provider.OpenAIChatKind, provider.Config{Endpoint: server.URL, Model: "fixture", APIKey: "local-not-a-credential"})
				if err != nil {
					return nil, err
				}
				return harness.NewBuilder(model, core.NewMemorySession(id)).Build(ctx)
			})
			want := engine.FailureExecution
			switch status {
			case 429:
				want = engine.FailureRateLimit
			case 401, 403:
				want = engine.FailureAuth
			case 503:
				want = engine.FailureStartup
			}
			var failure engine.FailureKind
			var outcome engine.Outcome
			err := adapter.Execute(context.Background(), engine.Request{Prompt: "local only"}, func(event engine.Event) error {
				if event.Kind == engine.Error {
					failure = event.Failure
				}
				if event.Kind == engine.Completed {
					outcome = event.Outcome
				}
				return nil
			})
			if err != nil || failure != want || outcome != engine.Failed || requests.Load() != 1 {
				t.Fatal("real typed HTTP error lost/no-retry violated", failure, outcome, requests.Load(), err)
			}
		})
	}
}

func TestResponseThenProviderFailureIsNeverStartup(t *testing.T) {
	adapter := New(func(ctx context.Context, id string, _ engine.Request, _ bool) (harness.Runner, error) {
		return harness.NewBuilder(modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
			if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: "already visible"}); err != nil {
				return err
			}
			return statusProvider(agentabi.ProviderAPICall, 503, true)
		}), core.NewMemorySession(id)).Build(ctx)
	})
	var got engine.FailureKind
	if err := adapter.Execute(context.Background(), engine.Request{Prompt: "fixture"}, func(event engine.Event) error {
		if event.Kind == engine.Error {
			got = event.Failure
		}
		return nil
	}); err != nil || got != engine.FailureExecution {
		t.Fatal("partial response became safe startup", got, err)
	}
}
