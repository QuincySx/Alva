// INPUT: Real SDK streams, atomic MemorySession and targeted audit storage failures
// OUTPUT: Bounded metadata-only audit records, true stream failures and no tool execution after audit failure
// POS: [L2.5 SDK assembly] Generic audit persistence remains paired with actual model outcomes and frozen authority.
package core_test

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
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
		err := build(t, core.NewBuilder(m, s).SystemPrompt("private-system-marker")).Run(context.Background(), []wire.Message{wire.TextMessage(wire.RoleUser, "private-input-marker")}, discard)
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
func TestLLMAuditFailureSettlesModelAndPreventsTools(t *testing.T) {
	failure := errors.New("audit persistence fixture")
	s := &llmAuditFailureLog{MemorySession: core.NewMemorySession("llm-audit-fail"), failure: failure}
	called := false
	m := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		if err := emit(callChunk("call")); err != nil {
			return err
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	tool := &testTool{execute: func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
		called = true
		return wire.ToolResult{}, nil
	}}
	err := build(t, core.NewBuilder(m, s).Tool(tool)).Run(context.Background(), []wire.Message{wire.TextMessage(wire.RoleUser, "input")}, discard)
	var typed *abi.SessionError
	if !errors.Is(err, failure) || !errors.As(err, &typed) || called {
		t.Fatal("audit failure disappeared or authorized execution", err, called)
	}
	settled := false
	for _, r := range records(t, s) {
		if r.ToolIntent != nil {
			t.Fatal("tool intent authorized despite audit failure")
		}
		if r.ModelOutcome != nil {
			settled = true
			if r.ModelOutcome.Status != abi.OutcomeFailed {
				t.Fatal("audit-failed model falsely succeeded")
			}
		}
	}
	if !settled {
		t.Fatal("audit failure left an unsettled model request")
	}
}
