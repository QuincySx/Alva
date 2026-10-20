// INPUT: context, typed session records, real SDK model/tool loop and injected audit append errors
// OUTPUT: Stage-batched onion timing/order/classification and settle-before-audit-failure regressions
// POS: [L2.5 SDK assembly] Audit proves actual hooks without exposing input/result/private-error text or altering execution authority.
package core_test

import (
	"context"
	"encoding/json"
	"errors"
	"github.com/QuincySx/Alva/extension/telemetry"
	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
	"reflect"
	"strings"
	"testing"
	"time"
)

type auditedLayer struct {
	name        string
	block       bool
	beforeError error
}

func (m auditedLayer) Name() string                                       { return m.name }
func (auditedLayer) OnRunStart(context.Context, abi.RuntimeContext) error { return nil }
func (auditedLayer) InputCommitted(context.Context, abi.RuntimeContext, wire.Message) error {
	return nil
}
func (m auditedLayer) BeforeModel(context.Context, abi.RuntimeContext, *abi.ModelRequest) error {
	return m.beforeError
}
func (auditedLayer) WrapModel(ctx context.Context, r abi.RuntimeContext, q abi.ModelRequest, emit func(wire.StreamChunk) error, next abi.ModelFunc) error {
	return next(ctx, r, q, emit)
}
func (auditedLayer) AfterModel(context.Context, abi.RuntimeContext, *wire.Message) error { return nil }
func (auditedLayer) PrepareTool(context.Context, abi.ToolPreparationContext, wire.ToolCall) (abi.ToolPreparation, error) {
	return abi.ToolPreparation{}, nil
}
func (m auditedLayer) BeforeTool(context.Context, abi.ToolExecutionContext, wire.ToolCall) error {
	if m.block {
		return &abi.ToolBlockedError{Reason: "private-veto-body"}
	}
	return nil
}
func (auditedLayer) WrapTool(ctx context.Context, e abi.ToolExecutionContext, c wire.ToolCall, next abi.ToolFunc) (wire.ToolResult, error) {
	return next(ctx, e, c)
}
func (auditedLayer) AfterTool(context.Context, abi.ToolExecutionContext, wire.ToolCall, *wire.ToolResult) error {
	return nil
}
func (auditedLayer) OnRunEnd(context.Context, abi.RuntimeContext, error) error { return nil }

type auditValue struct {
	RunID, Hook, ToolCallID string
	Calls                   []struct {
		MiddlewareName  string `json:"middleware_name"`
		LayerIndex      int    `json:"layer_index"`
		DurationMS      int64  `json:"duration_ms"`
		Outcome, Timing string
		ErrorSummary    string `json:"error_summary"`
	}
}

func auditRecords(t *testing.T, session abi.SessionStore) map[string][]auditValue {
	t.Helper()
	out := map[string][]auditValue{}
	for _, r := range records(t, session) {
		if r.Namespace != "alva.kernel.audit" || r.Type != "middleware_hook_batch" {
			continue
		}
		if r.Type != "middleware_hook_batch" || r.SchemaVersion != 1 {
			t.Fatal("bad audit envelope", r)
		}
		var raw struct {
			RunID      string          `json:"run_id"`
			Hook       string          `json:"hook"`
			ToolCallID string          `json:"tool_call_id"`
			Calls      json.RawMessage `json:"calls"`
		}
		if err := json.Unmarshal(r.Payload, &raw); err != nil {
			t.Fatal(err)
		}
		v := auditValue{RunID: raw.RunID, Hook: raw.Hook, ToolCallID: raw.ToolCallID}
		if err := json.Unmarshal(raw.Calls, &v.Calls); err != nil {
			t.Fatal(err)
		}
		if v.RunID == "" || len(v.Calls) == 0 {
			t.Fatal("empty/fake stage", string(r.Payload))
		}
		out[v.Hook] = append(out[v.Hook], v)
	}
	return out
}
func twoRoundModel() abi.LanguageModel {
	round := 0
	return modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		round++
		if round == 1 {
			if err := emit(callChunk("audit-call")); err != nil {
				return err
			}
		} else {
			if err := emit(wire.StreamChunk{Kind: wire.ChunkText, Text: "private-result-body"}); err != nil {
				return err
			}
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
}
func auditedBuilder(model abi.LanguageModel, log abi.ConversationLog) *core.Builder {
	return core.NewBuilder(model, log).Plugin(telemetry.New(nil))
}

func TestMiddlewareAuditBatchesWholeStagesAndPreservesOnionOrder(t *testing.T) {
	s := core.NewMemorySession("audit")
	tool := &testTool{execute: func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
		time.Sleep(3 * time.Millisecond)
		return wire.ToolResult{Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: "private-tool-body"}}}, nil
	}}
	a := build(t, auditedBuilder(twoRoundModel(), s).Tool(tool).Middleware(auditedLayer{name: "outer"}).Middleware(auditedLayer{name: "inner"}))
	if err := a.Run(context.Background(), []wire.Message{wire.TextMessage(wire.RoleUser, "private-input-body")}, discard); err != nil {
		t.Fatal(err)
	}
	values := auditRecords(t, s)
	for hook, want := range map[string][]string{"on_run_start": {"outer", "inner"}, "input_committed": {"outer", "inner"}, "before_model": {"outer", "inner"}, "after_model": {"inner", "outer"}, "prepare_tool": {"outer", "inner"}, "before_tool": {"outer", "inner"}, "wrap_tool": {"inner", "outer"}, "after_tool": {"inner", "outer"}, "wrap_model": {"inner", "outer"}, "on_run_end": {"inner", "outer"}} {
		if len(values[hook]) == 0 {
			t.Fatal("missing actual phase", hook)
		}
		for _, v := range values[hook] {
			var names []string
			for _, call := range v.Calls {
				names = append(names, call.MiddlewareName)
				if call.DurationMS < 0 || call.Outcome != "ok" || call.Timing != "inclusive" || call.ErrorSummary != "" {
					t.Fatal("bad success observation", hook, call)
				}
			}
			if !reflect.DeepEqual(names, want) {
				t.Fatal("not whole-stage onion order", hook, names, want)
			}
			if strings.Contains(hook, "tool") && v.ToolCallID != "audit-call" {
				t.Fatal("missing real call relation", v)
			}
		}
	}
	if len(values["before_model"]) != 2 || len(values["before_tool"]) != 1 {
		t.Fatal("per-layer writes or model/tool stages collapsed", values)
	}
	for _, r := range records(t, s) {
		if r.Namespace == "alva.kernel.audit" && (strings.Contains(string(r.Payload), "private-input-body") || strings.Contains(string(r.Payload), "private-result-body") || strings.Contains(string(r.Payload), "private-tool-body")) {
			t.Fatal("audit copied bodies")
		}
	}
	var toolOutcomeSeq uint64
	for _, r := range records(t, s) {
		if r.ToolOutcome != nil {
			toolOutcomeSeq = r.Sequence
		}
	}
	for _, r := range records(t, s) {
		if r.Namespace == "alva.kernel.audit" && strings.Contains(string(r.Payload), `"hook":"before_tool"`) && r.Sequence <= toolOutcomeSeq {
			t.Fatal("tool audit preceded actual settlement")
		}
	}
}

type auditFailureLog struct {
	*core.MemorySession
	hook    string
	failure error
	flushed bool
}

func (s *auditFailureLog) AppendRecords(ctx context.Context, batch []abi.SessionRecord) ([]abi.SessionRecord, error) {
	for _, r := range batch {
		if r.Namespace == "alva.kernel.audit" {
			var v struct {
				Hook string `json:"hook"`
			}
			_ = json.Unmarshal(r.Payload, &v)
			if v.Hook == s.hook {
				return nil, s.failure
			}
		}
	}
	return s.MemorySession.AppendRecords(ctx, batch)
}
func (s *auditFailureLog) Flush(ctx context.Context) error {
	s.flushed = true
	return s.MemorySession.Flush(ctx)
}
func TestToolAuditFailureReportsAndKeepsExecutingWithoutReplay(t *testing.T) {
	disk := errors.New("private-audit-disk-secret")
	s := &auditFailureLog{MemorySession: core.NewMemorySession("audit-failure"), hook: "wrap_tool", failure: disk}
	executed, models := 0, 0
	m := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		models++
		if models > 1 {
			return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
		}
		if err := emit(callChunk("first")); err != nil {
			return err
		}
		if err := emit(callChunk("second")); err != nil {
			return err
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	tool := &testTool{execute: func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
		executed++
		return wire.ToolResult{}, nil
	}}
	var reported []telemetry.Failure
	err := build(t, core.NewBuilder(m, s).Plugin(telemetry.New(func(_ context.Context, f telemetry.Failure) { reported = append(reported, f) })).Tool(tool).Middleware(auditedLayer{name: "layer"})).Run(context.Background(), nil, discard)
	if err != nil || len(reported) != 2 || !errors.Is(reported[0].Err, disk) || executed != 2 || models != 2 || !s.flushed {
		t.Fatal("audit failure swallowed/re-executed", err, executed, models, s.flushed)
	}
	var outcomes []abi.ToolOutcome
	for _, r := range records(t, s) {
		if r.ToolOutcome != nil {
			outcomes = append(outcomes, *r.ToolOutcome)
		}
	}
	if len(outcomes) != 2 || outcomes[0].CallID != "first" || outcomes[0].Status != abi.OutcomeSucceeded || outcomes[1].CallID != "second" || outcomes[1].Status != abi.OutcomeSucceeded {
		t.Fatal("real settled execution lost", outcomes)
	}
}
func TestCancelledHookReportsOptionalAuditFailureWithoutPrivateSummary(t *testing.T) {
	disk := errors.New("private-disk-detail")
	ctx, cancel := context.WithCancel(context.Background())
	s := &auditFailureLog{MemorySession: core.NewMemorySession("cancel-audit"), hook: "before_model", failure: disk}
	hook := &runtimeHooks{before: func(abi.RuntimeContext, *abi.ModelRequest) error { cancel(); return context.Canceled }}
	var reported []telemetry.Failure
	err := build(t, core.NewBuilder(textModel("never"), s).Plugin(telemetry.New(func(_ context.Context, f telemetry.Failure) { reported = append(reported, f) })).Middleware(hook)).Run(ctx, nil, discard)
	if !errors.Is(err, context.Canceled) || !abi.OnlyCancellation(err) || len(reported) != 1 || !errors.Is(reported[0].Err, disk) {
		t.Fatal("cancel hid storage failure", err)
	}
	s2 := core.NewMemorySession("fixed-summary")
	private := errors.New("api_key=must-not-copy-private-hook-error")
	err = build(t, auditedBuilder(textModel("never"), s2).Middleware(auditedLayer{name: "private", beforeError: private})).Run(context.Background(), nil, discard)
	if !errors.Is(err, private) {
		t.Fatal(err)
	}
	values := auditRecords(t, s2)
	call := values["before_model"][0].Calls[0]
	if call.ErrorSummary != "hook_failure" || call.Outcome != "error" {
		t.Fatal(call)
	}
	for _, r := range records(t, s2) {
		if r.Namespace == "alva.kernel.audit" && strings.Contains(string(r.Payload), "must-not-copy") {
			t.Fatal("private error body in audit", string(r.Payload))
		}
	}
}
func TestIntentionalVetoStillAuditsAfterHooksAndNoWrapperOrSideEffect(t *testing.T) {
	s := core.NewMemorySession("veto-audit")
	executed := 0
	tool := &testTool{execute: func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
		executed++
		return wire.ToolResult{}, nil
	}}
	if err := build(t, auditedBuilder(twoRoundModel(), s).Tool(tool).Middleware(auditedLayer{name: "outer", block: true}).Middleware(auditedLayer{name: "inner"})).Run(context.Background(), nil, discard); err != nil {
		t.Fatal(err)
	}
	values := auditRecords(t, s)
	if executed != 0 || len(values["wrap_tool"]) != 0 || len(values["before_tool"]) != 1 || len(values["before_tool"][0].Calls) != 1 || values["before_tool"][0].Calls[0].ErrorSummary != "intentional_tool_veto" || len(values["after_tool"][0].Calls) != 2 {
		t.Fatal("veto hook facts wrong", executed, values)
	}
}
