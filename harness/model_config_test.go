// INPUT: Explicit generic model request configuration, a real SDK loop and assembly-time recipe mutation.
// OUTPUT: Actual request sampling propagation and detached setter/build recipe ownership regressions.
// POS: [L5 harness testing] Keeps product/provider defaults out of the generic SDK configuration bridge.
package harness

import (
	"context"
	"testing"

	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestHarnessModelConfigReachesActualRequestAndFreezesRecipe(t *testing.T) {
	temperature, maxTokens, effort := 0.4, uint32(321), wire.ReasoningHigh
	config := wire.ModelConfig{Temperature: &temperature, MaxTokens: &maxTokens, ReasoningEffort: &effort, StopSequences: []string{"original-stop"}}
	var requests []wire.ModelConfig
	model := modelFunc(func(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		requests = append(requests, wire.CloneModelConfig(request.Config))
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	builder := NewBuilder(model, core.NewMemorySession("model-config-recipe")).Workspace(t.TempDir()).ModelConfig(config)
	temperature, maxTokens, effort = 9, 999, wire.ReasoningLow
	config.StopSequences[0] = "caller-mutated"
	nextTemperature := 0.8
	builder.Plugin(&recipeMutationPlugin{once: func() { builder.ModelConfig(wire.ModelConfig{Temperature: &nextTemperature}) }})
	for i := 0; i < 2; i++ {
		agent, err := builder.Build(t.Context())
		if err != nil {
			t.Fatal(err)
		}
		if err = agent.Run(t.Context(), []wire.Message{wire.TextMessage(wire.RoleUser, "explicit request")}, func(abi.Event) error { return nil }); err != nil {
			t.Fatal(err)
		}
	}
	first, second := requests[0], requests[1]
	if first.Temperature == nil || *first.Temperature != 0.4 || first.MaxTokens == nil || *first.MaxTokens != 321 || first.ReasoningEffort == nil || *first.ReasoningEffort != wire.ReasoningHigh || len(first.StopSequences) != 1 || first.StopSequences[0] != "original-stop" {
		t.Fatal("caller or assembly callback changed current model recipe", first)
	}
	if second.Temperature == nil || *second.Temperature != 0.8 || second.MaxTokens != nil {
		t.Fatal("explicit later recipe was not applied", second)
	}
	invalid := uint32(0)
	if _, err := NewBuilder(model, core.NewMemorySession("invalid-model-config")).Workspace(t.TempDir()).ModelConfig(wire.ModelConfig{MaxTokens: &invalid}).Build(t.Context()); err == nil {
		t.Fatal("invalid model configuration reached runtime")
	}
}
