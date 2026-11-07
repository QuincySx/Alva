// INPUT: Durable product preference admission, conservative engine declarations and a request-recording runtime.
// OUTPUT: Frozen model options routed into actual requests and unsupported adapters rejected before Execute.
// POS: [L7 Hub testing] Product attempt snapshots drive execution without widening the six-method execution store.
package hub_test

import (
	"context"
	"errors"
	"testing"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

func TestRuntimePassesAdmittedModelOptionsAndRejectsUndeclaredSurface(t *testing.T) {
	temperature, tokens := 0.25, uint32(432)
	for _, tc := range []struct {
		name    string
		options hub.ModelOptions
		surface engine.ConfigSurface
		allowed bool
	}{
		{"all-supported", hub.ModelOptions{Generation: engine.GenerationOptions{Temperature: &temperature, MaxTokens: &tokens, ReasoningEffort: "high"}, SystemPrompt: "admitted system", MaxTurns: 3}, engine.ConfigSurface{SupportsGeneration: true, SupportsSystemPrompt: true, SupportsMaxTurns: true}, true},
		{"generation-unsupported", hub.ModelOptions{Generation: engine.GenerationOptions{Temperature: &temperature}}, engine.ConfigSurface{}, false},
		{"system-unsupported", hub.ModelOptions{SystemPrompt: "must retain role"}, engine.ConfigSurface{}, false},
		{"turn-limit-unsupported", hub.ModelOptions{MaxTurns: 1}, engine.ConfigSurface{}, false},
	} {
		t.Run(tc.name, func(t *testing.T) {
			store, session := identityStore(t)
			_, err := store.SaveModelPreference(t.Context(), hub.ModelPreference{Key: hub.ModelPreferenceKey(identityTarget("fixture")), Options: tc.options})
			if err != nil {
				t.Fatal(err)
			}
			calls := make(chan engine.Request, 1)
			runtime := hub.New(store)
			ownIdentityRuntime(t, runtime)
			adapter := identityEngine{run: func(_ context.Context, request engine.Request, emit func(engine.Event) error) error {
				calls <- request
				if err := emit(engine.Event{Kind: engine.SessionStarted, SessionID: "options-native"}); err != nil {
					return err
				}
				return emit(engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded})
			}}
			surface := tc.surface
			surface.ProviderProtocol = "fixture"
			if err = runtime.RegisterDescriptor("fixture", engine.Descriptor{Runtime: adapter, Config: surface}); err != nil {
				t.Fatal(err)
			}
			_, done, err := runtime.Start(t.Context(), session.ID, "actual admitted prompt")
			if err != nil {
				t.Fatal(err)
			}
			err = identityReceive(t, done)
			if !tc.allowed {
				if !errors.Is(err, engine.ErrUnsupported) || len(calls) != 0 {
					t.Fatal("undeclared options reached adapter Execute", err)
				}
				return
			}
			if err != nil {
				t.Fatal(err)
			}
			request := identityReceive(t, calls)
			if request.SystemPrompt != tc.options.SystemPrompt || request.MaxTurns != 3 || request.Generation.Temperature == nil || *request.Generation.Temperature != 0.25 || request.Generation.MaxTokens == nil || *request.Generation.MaxTokens != 432 || request.Generation.ReasoningEffort != "high" {
				t.Fatal("request omitted admitted option snapshot", request)
			}
			*request.Generation.Temperature = 8
			settings, err := store.Settings(t.Context(), session.ID)
			if err != nil || settings.ModelOptions.Generation.Temperature == nil || *settings.ModelOptions.Generation.Temperature != 0.25 {
				t.Fatal("request mutation changed frozen product recipe", err)
			}
		})
	}
}
