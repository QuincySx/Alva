// INPUT: Actual Hub supervisor, durable activity events and explicit fallback account routes
// OUTPUT: Build/prepare activity risk blocks unsafe fallback before any ToolStart exists
// POS: [L7 Hub product] ToolIntent side-effect facts do not erase independent native activity evidence
package sqlite

import (
	"context"
	"github.com/QuincySx/Alva/go/apps/hub"
	"github.com/QuincySx/Alva/go/integration/engine"
	"testing"
)

func TestExecutionActivityRiskPersistsBeforeStartupFallback(t *testing.T) {
	for _, effect := range []engine.Effect{engine.EffectUnknown, engine.EffectWrite, engine.EffectExecute, engine.EffectNetwork, engine.EffectRead, engine.EffectNone} {
		t.Run(string(effect), func(t *testing.T) {
			s := newStore(t)
			ctx := context.Background()
			session, err := s.CreateSession(ctx, "activity", t.TempDir(), target("a"))
			if err != nil {
				t.Fatal(err)
			}
			fallback := target("b")
			fallback.AutomaticAuthorized = true
			if err = s.SetFallback(ctx, session.ID, []hub.Target{fallback}); err != nil {
				t.Fatal(err)
			}
			r := hub.New(s)
			defer r.Close(ctx)
			calls := 0
			_ = r.Register("a", runtimeFunc{control: true, run: func(_ context.Context, _ engine.Request, emit func(engine.Event) error) error {
				if err := emit(engine.Event{Kind: engine.ExecutionActivity, Activity: &engine.Activity{ID: "prepare", Name: "native prepare", Effect: effect}}); err != nil {
					return err
				}
				// Read actual committed evidence before the hypothetical native work.
				rows, err := s.History(ctx, session.ID, 0, 128)
				if err != nil {
					return err
				}
				found := false
				for _, e := range rows {
					found = found || e.Event.Kind == engine.ExecutionActivity
				}
				if !found {
					t.Fatal("activity start preceded its durable evidence")
				}
				if err = emit(engine.Event{Kind: engine.Error, Text: "startup rate limited after preparation", Failure: engine.FailureRateLimit}); err != nil {
					return err
				}
				return emit(engine.Event{Kind: engine.Completed, Outcome: engine.Failed})
			}})
			_ = r.Register("b", runtimeFunc{control: true, run: func(_ context.Context, _ engine.Request, emit func(engine.Event) error) error {
				calls++
				if err := emit(engine.Event{Kind: engine.SessionStarted, SessionID: "fallback-native"}); err != nil {
					return err
				}
				return emit(engine.Event{Kind: engine.Completed, Outcome: engine.Succeeded})
			}})
			err = r.Run(ctx, session.ID, "fixture")
			safe := effect == engine.EffectRead || effect == engine.EffectNone
			if safe && (err != nil || calls != 1) {
				t.Fatal("read-only activity incorrectly blocked authorized fallback", err)
			}
			if !safe && (err == nil || calls != 0) {
				t.Fatal("possible activity side effects allowed a duplicate fallback", err)
			}
		})
	}
}
