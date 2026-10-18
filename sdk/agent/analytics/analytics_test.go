// INPUT: context, encoding/json, errors, sync, testing, time, agent/abi, agent/core, kernel/abi, llm/wire
// OUTPUT: Nonblocking bounded telemetry and real lifecycle/usage regressions
// POS: [L3 capability library] Proves optional telemetry is live without becoming authority or affecting model/tool results.
package analytics

import (
	"context"
	"encoding/json"
	"errors"
	"sync"
	"testing"
	"time"

	agentabi "github.com/QuincySx/Alva/sdk/agent/abi"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestBufferOwnershipCapacityAndNonblockingLoss(t *testing.T) {
	buffer, _ := NewBuffer(1)
	cached, written := uint64(0), uint64(2)
	usage := &wire.Usage{InputTokens: 12, CachedInputTokens: &cached, CacheWriteInputTokens: &written}
	event := agentabi.AnalyticsEvent{Type: agentabi.AnalyticsLLMCall, SessionID: "s", Time: time.Now(), LLMCall: &agentabi.AnalyticsModelCall{Usage: usage}}
	buffer.Record(event)
	usage.InputTokens = 999
	cached, written = 999, 999
	var wait sync.WaitGroup
	for i := 0; i < 16; i++ {
		wait.Add(1)
		go func() {
			defer wait.Done()
			for n := 0; n < 100; n++ {
				buffer.Record(agentabi.AnalyticsEvent{})
			}
		}()
	}
	wait.Wait()
	buffer.Record(event)
	events, err := buffer.Drain(10)
	if err != nil || len(events) != 1 || events[0].LLMCall.Usage.InputTokens != 12 || events[0].LLMCall.Usage.CachedInputTokens == nil || *events[0].LLMCall.Usage.CachedInputTokens != 0 || events[0].LLMCall.Usage.CacheWriteInputTokens == nil || *events[0].LLMCall.Usage.CacheWriteInputTokens != 2 || buffer.Dropped() != 1601 {
		t.Fatal(events, buffer.Dropped(), err)
	}
}

type modelFixture struct {
	turns    int
	noFinish bool
	failure  error
}

func (*modelFixture) ProviderID() string { return "fixture" }
func (*modelFixture) ModelID() string    { return "model" }
func (model *modelFixture) Stream(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	model.turns++
	if model.failure != nil {
		return model.failure
	}
	if model.noFinish {
		return nil
	}
	if model.turns == 1 {
		if err := emit(wire.StreamChunk{Kind: wire.ChunkToolCall, ToolCall: &wire.ToolCall{ID: "call", Name: "tool", Arguments: json.RawMessage(`{}`)}}); err != nil {
			return err
		}
	}
	cached, written := uint64(3), uint64(2)
	if err := emit(wire.StreamChunk{Kind: wire.ChunkUsage, Usage: &wire.Usage{InputTokens: 10, CachedInputTokens: &cached, CacheWriteInputTokens: &written, ReasoningTokens: 1}}); err != nil {
		return err
	}
	return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
}

type toolFixture struct{}

func (toolFixture) Name() string                            { return "tool" }
func (toolFixture) Description() string                     { return "fixture" }
func (toolFixture) ParametersSchema() json.RawMessage       { return json.RawMessage(`{"type":"object"}`) }
func (toolFixture) Scheduling(wire.ToolCall) abi.Scheduling { return abi.Scheduling{} }
func (toolFixture) DescribeEffect(wire.ToolCall) abi.Effect { return abi.Effect{Kind: abi.EffectRead} }
func (toolFixture) IsReadOnly(wire.ToolCall) bool           { return true }
func (toolFixture) Execute(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
	return wire.ToolResult{}, nil
}

func TestPluginObservesActualModelToolLifecycleAndUsage(t *testing.T) {
	buffer, _ := NewBuffer(20)
	model := &modelFixture{}
	agent, err := core.NewBuilder(model, core.NewMemorySession("actual")).Tool(toolFixture{}).Plugin(Plugin{Sink: buffer}).Build(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if err := agent.Run(context.Background(), nil, func(abi.Event) error { return nil }); err != nil {
		t.Fatal(err)
	}
	events, _ := buffer.Drain(20)
	counts := make(map[agentabi.AnalyticsKind]int)
	for _, event := range events {
		counts[event.Type]++
		if event.SessionID != "actual" || event.RunID == "" {
			t.Fatal("wrong owner", event)
		}
		if event.LLMCall != nil && (event.LLMCall.Provider != "fixture" || event.LLMCall.Model != "model" || event.LLMCall.Usage.CachedInputTokens == nil || *event.LLMCall.Usage.CachedInputTokens != 3 || event.LLMCall.Usage.CacheWriteInputTokens == nil || *event.LLMCall.Usage.CacheWriteInputTokens != 2 || event.LLMCall.CostUSD != nil) {
			t.Fatal("metadata fabricated or dropped", event)
		}
	}
	if counts[agentabi.AnalyticsSessionStart] != 1 || counts[agentabi.AnalyticsSessionEnd] != 1 || counts[agentabi.AnalyticsToolStart] != 1 || counts[agentabi.AnalyticsToolEnd] != 1 || counts[agentabi.AnalyticsLLMCall] != 2 {
		t.Fatal(counts)
	}
}

func TestAnalyticsDoesNotReportFailedOrIncompleteStreamAsSuccess(t *testing.T) {
	for _, model := range []*modelFixture{{failure: errors.New("transport failed")}, {noFinish: true}} {
		buffer, _ := NewBuffer(10)
		agent, err := core.NewBuilder(model, core.NewMemorySession("actual")).Plugin(Plugin{Sink: buffer}).Build(context.Background())
		if err != nil {
			t.Fatal(err)
		}
		if err := agent.Run(context.Background(), nil, func(abi.Event) error { return nil }); err == nil {
			t.Fatal("stream failure hidden")
		}
		events, _ := buffer.Drain(10)
		for _, event := range events {
			if event.LLMCall != nil {
				t.Fatal("failed stream reported successful")
			}
		}
	}
}

type identityModel struct{ provider, model string }

func (m identityModel) ProviderID() string { return m.provider }
func (m identityModel) ModelID() string    { return m.model }
func (identityModel) Stream(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
	return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
}

type replaceOuter struct{ next abi.LanguageModel }

type replaceSessionOuter struct{ next abi.ConversationLog }

func (replaceSessionOuter) Name() string { return "replace-session-outer" }
func (m replaceSessionOuter) WrapModel(ctx context.Context, r abi.RuntimeContext, q abi.ModelRequest, emit func(wire.StreamChunk) error, next abi.ModelFunc) error {
	if err := r.State.SetSession(m.next); err != nil {
		return err
	}
	return next(ctx, r, q, emit)
}

func TestAnalyticsUsesBoundInvocationSessionAfterStateSwitch(t *testing.T) {
	buffer, _ := NewBuffer(10)
	bound, later := core.NewMemorySession("bound"), core.NewMemorySession("later")
	agent, err := core.NewBuilder(identityModel{"provider", "model"}, bound).Middleware(replaceSessionOuter{later}, -100).Plugin(Plugin{Sink: buffer}).Build(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	if err := agent.Run(t.Context(), nil, func(abi.Event) error { return nil }); err != nil {
		t.Fatal(err)
	}
	events, err := buffer.Drain(10)
	if err != nil {
		t.Fatal(err)
	}
	found := false
	for _, event := range events {
		if event.LLMCall != nil {
			found = true
			if event.SessionID != "bound" {
				t.Fatal("next session received this invocation's telemetry", event.SessionID)
			}
		}
	}
	boundRecords, err := bound.Records(t.Context(), 0)
	if err != nil {
		t.Fatal(err)
	}
	laterRecords, err := later.Records(t.Context(), 0)
	if err != nil {
		t.Fatal(err)
	}
	boundCalls, laterCalls := 0, 0
	for _, record := range boundRecords {
		if record.ModelIntent != nil || record.ModelOutcome != nil {
			boundCalls++
		}
	}
	for _, record := range laterRecords {
		if record.ModelIntent != nil || record.ModelOutcome != nil {
			laterCalls++
		}
	}
	if !found || boundCalls != 2 || laterCalls != 0 || agent.Session().ID() != "later" {
		t.Fatal("fixture did not switch live state independently of actual invocation")
	}
}

func (replaceOuter) Name() string { return "replace-outer" }
func (m replaceOuter) WrapModel(ctx context.Context, r abi.RuntimeContext, q abi.ModelRequest, emit func(wire.StreamChunk) error, next abi.ModelFunc) error {
	if err := r.State.SetModel(m.next); err != nil {
		return err
	}
	return next(ctx, r, q, emit)
}
func TestAnalyticsUsesCapturedInvocationIdentityNotNextState(t *testing.T) {
	buffer, _ := NewBuffer(10)
	a, err := core.NewBuilder(identityModel{"actual-provider", "actual-model"}, core.NewMemorySession("identity")).Middleware(replaceOuter{identityModel{"next-provider", "next-model"}}, -100).Plugin(Plugin{Sink: buffer}).Build(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if err := a.Run(context.Background(), nil, func(abi.Event) error { return nil }); err != nil {
		t.Fatal(err)
	}
	events, _ := buffer.Drain(10)
	found := false
	for _, e := range events {
		if e.LLMCall != nil {
			found = true
			if e.LLMCall.Provider != "actual-provider" || e.LLMCall.Model != "actual-model" {
				t.Fatal("recorded next invocation as current", e)
			}
		}
	}
	if !found {
		t.Fatal("missing actual model call")
	}
}
