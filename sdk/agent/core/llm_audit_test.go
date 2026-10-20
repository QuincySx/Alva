// INPUT: Real SDK streams, atomic MemorySession and targeted audit storage failures
// OUTPUT: Bounded metadata-only audit records, true stream failures and no tool execution after audit failure
// POS: [L2.5 SDK assembly] Generic audit persistence remains paired with actual model outcomes and frozen authority.
package core_test

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
	"strings"
	"testing"
)

func TestLLMStreamAuditIsBoundedMetadataAndRetainsFailure(t *testing.T) {
	for _, fail := range []bool{false, true} {
		s := core.NewMemorySession("llm-audit")
		private := errors.New("private-provider-error-marker")
		m := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
			for range 2000 {
				if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: "private-output-marker"}); err != nil {
					return err
				}
			}
			if fail {
				return private
			}
			zero := uint64(0)
			if err := emit(wire.StreamChunk{Kind: wire.ChunkUsage, Usage: &wire.Usage{InputTokens: 5, CachedInputTokens: &zero}}); err != nil {
				return err
			}
			return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
		})
		err := build(t, auditedBuilder(m, s).SystemPrompt("private-system-marker")).Run(context.Background(), []wire.Message{wire.TextMessage(wire.RoleUser, "private-input-marker")}, discard)
		if fail && !errors.Is(err, private) || !fail && err != nil {
			t.Fatal("stream outcome changed", err)
		}
		counts := map[string]int{}
		for _, r := range records(t, s) {
			if r.Namespace != "alva.kernel.audit" {
				continue
			}
			counts[r.Type]++
			for _, marker := range []string{"private-output-marker", "private-provider-error-marker", "private-system-marker", "private-input-marker"} {
				if strings.Contains(string(r.Payload), marker) {
					t.Fatal("audit leaked body/private error", r.Type)
				}
			}
			var value map[string]any
			if json.Unmarshal(r.Payload, &value) != nil {
				t.Fatal("invalid audit JSON")
			}
			if r.Type == "llm_stream_summary" {
				if value["event_count"].(float64) < 2000 || value["time_to_first_output_ms"] == nil {
					t.Fatal("real stream observation missing")
				}
				if fail && value["outcome"] != "error" {
					t.Fatal("failed stream falsely audited success")
				}
			}
			if r.Type == "llm_call_end" && !fail && value["cache_outcome"] != "miss" {
				t.Fatal("explicit zero cache report lost")
			}
		}
		for _, kind := range []string{"llm_call_start", "llm_first_output", "llm_stream_summary", "llm_call_end"} {
			if counts[kind] != 1 {
				t.Fatal("per-token audit writes or missing evidence", counts)
			}
		}
	}
}

type llmAuditFailureLog struct {
	*core.MemorySession
	failure error
	failed  bool
}

func (s *llmAuditFailureLog) AppendRecords(ctx context.Context, batch []abi.SessionRecord) ([]abi.SessionRecord, error) {
	for _, r := range batch {
		if r.Type == "llm_stream_summary" && !s.failed {
			s.failed = true
			return nil, s.failure
		}
	}
	return s.MemorySession.AppendRecords(ctx, batch)
}
func TestLLMAuditFailureDoesNotChangeExecutionFacts(t *testing.T) {
	failure := errors.New("audit persistence fixture")
	s := &llmAuditFailureLog{MemorySession: core.NewMemorySession("llm-audit-fail"), failure: failure}
	called := 0
	m := batchModel([]string{"call"})
	tool := &testTool{execute: func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
		called++
		return wire.ToolResult{}, nil
	}}
	err := build(t, auditedBuilder(m, s).Tool(tool)).Run(t.Context(), []wire.Message{wire.TextMessage(wire.RoleUser, "input")}, discard)
	if err != nil || called != 1 || !s.failed {
		t.Fatal("optional audit affected execution", err, called)
	}
	for _, r := range records(t, s) {
		if r.ModelOutcome != nil && r.ModelOutcome.Status != abi.OutcomeSucceeded {
			t.Fatal("diagnostic relabelled model", r.ModelOutcome)
		}
		if r.ToolOutcome != nil && r.ToolOutcome.Status != abi.OutcomeSucceeded {
			t.Fatal("diagnostic relabelled tool", r.ToolOutcome)
		}
	}
}
func TestPureSDKDoesNotRequireOptionalTelemetry(t *testing.T) {
	s := core.NewMemorySession("pure-sdk")
	err := build(t, core.NewBuilder(textModel("answer"), s)).Run(t.Context(), nil, discard)
	if err != nil {
		t.Fatal(err)
	}
	for _, r := range records(t, s) {
		if r.Namespace == "alva.kernel.audit" {
			t.Fatal("SDK installed a telemetry policy")
		}
	}
}
