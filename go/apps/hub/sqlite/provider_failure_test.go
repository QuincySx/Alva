// INPUT: Actual Alva harness/provider HTTP evidence and durable Hub authorized fallback routing
// OUTPUT: Safe rate/startup fallback and mixed storage/auth/prepare failure non-replay regressions
// POS: [L7 Hub product] Product fallback consumes complete typed engine evidence, never message-string heuristics
package sqlite

import (
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"

	"github.com/QuincySx/Alva/go/apps/hub"
	"github.com/QuincySx/Alva/go/harness"
	"github.com/QuincySx/Alva/go/integration/engine"
	"github.com/QuincySx/Alva/go/integration/engine/alva"
	"github.com/QuincySx/Alva/go/integration/llm/provider"
	agentabi "github.com/QuincySx/Alva/go/sdk/agent/abi"
	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
)

func TestTypedAlvaProviderFallbackRequiresSafeCompleteEvidence(t *testing.T) {
	for _, test := range []struct {
		name     string
		status   int
		mixed    bool
		activity bool
		safe     bool
		failure  engine.FailureKind
	}{
		{"real 429", 429, false, false, true, engine.FailureRateLimit},
		{"real 503", 503, false, false, true, engine.FailureStartup},
		{"real 401", 401, false, false, false, engine.FailureAuth},
		{"real 400", 400, false, false, false, engine.FailureExecution},
		{"429 plus native flush failure", 429, true, false, false, engine.FailureExecution},
		{"429 after native prepare admission", 429, false, true, false, engine.FailureRateLimit},
	} {
		t.Run(test.name, func(t *testing.T) {
			ctx := context.Background()
			s := newStore(t)
			session, err := s.CreateSession(ctx, "provider fallback fixture", t.TempDir(), target("a"))
			if err != nil {
				t.Fatal(err)
			}
			fallback := target("b")
			fallback.AutomaticAuthorized = true
			if err = s.SetFallback(ctx, session.ID, []hub.Target{fallback}); err != nil {
				t.Fatal(err)
			}
			var requests atomic.Int32
			server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
				requests.Add(1)
				w.WriteHeader(test.status)
			}))
			defer server.Close()
			runtime := hub.New(s)
			defer runtime.Close(ctx)
			adapter := alva.New(func(ctx context.Context, id string, request engine.Request, _ bool) (harness.Runner, error) {
				if test.activity {
					if err := request.ReportActivity(engine.Activity{ID: "fixture-native", Name: "explicit native preparation", Effect: engine.EffectUnknown}); err != nil {
						return nil, err
					}
				}
				if test.mixed {
					return nil, errors.Join(&agentabi.ProviderError{Kind: agentabi.ProviderRateLimited, StatusCode: &test.status, Retryable: true}, &abi.SessionError{Operation: "flush", SessionID: id, Err: context.DeadlineExceeded})
				}
				model, err := provider.Build(provider.OpenAIChatKind, provider.Config{Endpoint: server.URL, Model: "fixture"})
				if err != nil {
					return nil, err
				}
				return harness.NewBuilder(model, core.NewMemorySession(id)).Build(ctx)
			})
			if err = runtime.Register("a", adapter); err != nil {
				t.Fatal(err)
			}
			calls := 0
			if err = runtime.Register("b", runtimeFunc{control: true, run: func(_ context.Context, _ engine.Request, emit func(engine.Event) error) error {
				calls++
				if err := emit(engine.Event{Kind: engine.SessionStarted, SessionID: "authorized-fallback-native"}); err != nil {
					return err
				}
				return emit(engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded})
			}}); err != nil {
				t.Fatal(err)
			}
			err = runtime.Run(ctx, session.ID, "local fixture only")
			if test.safe && (err != nil || calls != 1) || !test.safe && (err == nil || calls != 0) {
				t.Fatal("fallback did not respect actual safe evidence", err, calls)
			}
			wantRequests := int32(1)
			if test.mixed {
				wantRequests = 0
			}
			if requests.Load() != wantRequests {
				t.Fatal("provider request replayed", requests.Load())
			}
			rows, err := s.History(ctx, session.ID, 0, 128)
			if err != nil {
				t.Fatal(err)
			}
			found := false
			for _, row := range rows {
				if row.Event.Kind == engine.Error {
					found = true
					if row.Event.Failure != test.failure {
						t.Fatal("durable attempt classification differs", row.Event.Failure, test.failure)
					}
				}
			}
			if !found {
				t.Fatal("missing actual fatal failure evidence")
			}
		})
	}
}
