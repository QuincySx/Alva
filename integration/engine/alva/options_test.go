// INPUT: Actual local demo SDK loop and explicitly option-aware harness factory
// OUTPUT: Real iteration exhaustion/failure and conservative ordinary factory validation regressions
// POS: [L7 Alva product adapter] Supported request options configure the actual loop rather than changing only descriptor metadata.
package alva

import (
	"context"
	"errors"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/harness"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

func TestConfiguredFactoryActualIterationLimitAndUnsupportedPreflight(t *testing.T) {
	var log *core.MemorySession
	builds := 0
	workspace := t.TempDir()
	factory := func(ctx context.Context, id string, request engine.Request, _ bool) (harness.Runner, error) {
		builds++
		log = core.NewMemorySession(id)
		return harness.NewDemoBuilder(log).Workspace(workspace).SystemPrompt(request.SystemPrompt).MaxIterations(request.MaxTurns).Build(ctx)
	}
	request := engine.Request{Prompt: "exercise one actual model/tool iteration", SystemPrompt: "Keep the original configured system role.", MaxTurns: 1}
	if err := New(factory).Execute(t.Context(), request, func(engine.Event) error { return nil }); !errors.Is(err, engine.ErrUnsupported) || builds != 0 {
		t.Fatal("ordinary factory ignored declared options", builds, err)
	}
	a := NewConfigured(factory, nil, FactoryOptions{SystemPrompt: true, MaxTurns: true})
	var terminal engine.Outcome
	var failureText string
	limited := false
	err := a.Execute(t.Context(), request, func(event engine.Event) error {
		if event.Kind == engine.Error {
			failureText = event.Text
		}
		if event.Kind == engine.Error && strings.Contains(event.Text, "maximum agent iterations reached") {
			limited = true
		}
		if event.Kind == engine.Completed {
			terminal = event.Outcome
		}
		return nil
	})
	if err != nil || builds != 1 || terminal != engine.Failed || !limited {
		t.Fatal("real loop limit was falsely successful", terminal, builds, failureText, err)
	}
	records, err := log.Records(t.Context(), 0)
	if err != nil {
		t.Fatal(err)
	}
	models, tools := 0, 0
	for _, record := range records {
		if record.Kind == abi.RecordModelIntent {
			models++
		}
		if record.Kind == abi.RecordToolOutcome {
			tools++
		}
	}
	if models != 1 || tools != 1 {
		t.Fatal("limit did not bind actual SDK iterations", models, tools)
	}
	request.MaxTurns = 1025
	if err = a.Execute(t.Context(), request, func(engine.Event) error { return nil }); err == nil || builds != 1 {
		t.Fatal("invalid options reached factory", builds, err)
	}
}
