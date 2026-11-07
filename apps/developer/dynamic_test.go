// INPUT: Explicit dynamic target sources, blocked resolution and a local HTTP fixture.
// OUTPUT: Cold metadata, all-target fail-closed freezing and joined Stop/Close before inference.
// POS: [application developer testing] Verifies dynamic route admission owns real resolution without partial network execution.
package developer

import (
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/integration/llm/eval"
	"github.com/QuincySx/Alva/integration/llm/provider"
)

type testTargetSource struct {
	targets []eval.Target
	resolve func(context.Context, []eval.Target) ([]Route, error)
}

func (s testTargetSource) Targets() []eval.Target { return append([]eval.Target(nil), s.targets...) }
func (s testTargetSource) Resolve(ctx context.Context, selected []eval.Target) ([]Route, error) {
	return s.resolve(ctx, selected)
}

func TestDeveloperDynamicFreezeFailsBeforeAnySelectedRequest(t *testing.T) {
	var calls atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(http.ResponseWriter, *http.Request) { calls.Add(1) }))
	defer server.Close()
	for _, failure := range []string{"resolution", "invalid-later-provider", "changed-identity"} {
		t.Run(failure, func(t *testing.T) {
			var resolutions atomic.Int32
			source := testTargetSource{targets: []eval.Target{{ID: "first", Protocol: "openai_chat", Model: "fixture"}, {ID: "second", Protocol: "openai_chat", Model: "fixture"}}}
			source.resolve = func(_ context.Context, selected []eval.Target) ([]Route, error) {
				resolutions.Add(1)
				if len(selected) != 2 || selected[0] != source.targets[0] || selected[1] != source.targets[1] {
					t.Error("freeze lost exact selected metadata")
				}
				routes := []Route{{ID: "first", Protocol: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: server.URL, Model: "fixture"}}, {ID: "second", Protocol: provider.OpenAIChatKind, Provider: provider.Config{Endpoint: server.URL, Model: "fixture"}}}
				switch failure {
				case "resolution":
					return routes[:1], errors.New("synthetic-private-source-error")
				case "invalid-later-provider":
					routes[1].Provider.Endpoint = "invalid"
				case "changed-identity":
					routes[1].Provider.Model = "replacement"
				}
				return routes, nil
			}
			s, err := NewServiceWithSource(nil, source)
			if err != nil {
				t.Fatal(err)
			}
			defer s.Close(t.Context())
			request := Request{Mode: Test, Targets: []string{"first", "second"}}
			if len(s.Targets()) != 2 {
				t.Fatal("cold dynamic targets missing")
			}
			if _, err = s.Plan(request); err != nil || resolutions.Load() != 0 || calls.Load() != 0 {
				t.Fatal("plan resolved credentials or called inference", err)
			}
			if err = s.Start(request); err != nil {
				t.Fatal("valid cold request was not admitted", err)
			}
			if snapshot := awaitJob(t, s); snapshot.State != "failed" || snapshot.StartedCalls != 0 || calls.Load() != 0 {
				t.Fatal("partial matrix escaped preflight", snapshot)
			}
		})
	}
}

func TestDeveloperDynamicStopAndCloseOwnResolutionUntilItReturns(t *testing.T) {
	for _, closeService := range []bool{false, true} {
		name := "Stop"
		if closeService {
			name = "Close"
		}
		t.Run(name, func(t *testing.T) {
			entered, canceled, release := make(chan struct{}), make(chan struct{}), make(chan struct{})
			source := testTargetSource{targets: []eval.Target{{ID: "dynamic", Protocol: "openai_chat", Model: "fixture"}}}
			source.resolve = func(ctx context.Context, _ []eval.Target) ([]Route, error) {
				close(entered)
				<-ctx.Done()
				close(canceled)
				<-release // A real storage operation may still own cleanup after cancellation.
				return nil, ctx.Err()
			}
			s, err := NewServiceWithSource(nil, source)
			if err != nil {
				t.Fatal(err)
			}
			request := Request{Mode: Test, Targets: []string{"dynamic"}}
			if err = s.Start(request); err != nil {
				t.Fatal(err)
			}
			select {
			case <-entered:
			case <-time.After(3 * time.Second):
				t.Fatal("resolution did not start")
			}
			if err = s.Start(request); err == nil {
				t.Fatal("another job entered during resolution")
			}
			short, cancel := context.WithTimeout(t.Context(), 10*time.Millisecond)
			defer cancel()
			if closeService {
				err = s.Close(short)
			} else {
				err = s.Cancel(short)
			}
			if !errors.Is(err, context.DeadlineExceeded) || s.Snapshot().State != "running" {
				t.Fatal("unjoined resolution owner disappeared", err, s.Snapshot())
			}
			select {
			case <-canceled:
			default:
				t.Fatal("resolution did not receive cancellation")
			}
			close(release)
			if err = s.Close(t.Context()); err != nil || s.Snapshot().State != "canceled" || s.Snapshot().StartedCalls != 0 {
				t.Fatal("resolution did not finish its owner", err, s.Snapshot())
			}
		})
	}
}
