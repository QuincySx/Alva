// INPUT: context, encoding/json, errors, strings, sync/atomic, testing, agent/core, kernel/abi/bus, llm/wire
// OUTPUT: Invocation binding, final admission, serial continuation and published failure regressions
// POS: [L2.5 SDK acceptance] Exercises composed middleware through real loop settlement and actual models/tools.
package core_test

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"sync/atomic"
	"testing"

	"github.com/QuincySx/Alva/go/sdk/agent/core"
	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/kernel/bus"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

type modelLayer struct {
	name string
	wrap func(context.Context, abi.RuntimeContext, abi.ModelRequest, func(wire.StreamChunk) error, abi.ModelFunc) error
}

func (layer modelLayer) Name() string { return layer.name }
func (layer modelLayer) WrapModel(ctx context.Context, runtime abi.RuntimeContext, request abi.ModelRequest, emit func(wire.StreamChunk) error, next abi.ModelFunc) error {
	return layer.wrap(ctx, runtime, request, emit, next)
}

type requestGate struct {
	name  string
	check func(abi.RuntimeContext, abi.ModelRequest) error
}

func (gate requestGate) Name() string { return gate.name }
func (gate requestGate) AdmitModel(_ context.Context, runtime abi.RuntimeContext, request abi.ModelRequest) error {
	return gate.check(runtime, request)
}

func TestModelContinuationRestoresActualRuntimeAndBoundSession(t *testing.T) {
	first, nextLog := core.NewMemorySession("bound"), core.NewMemorySession("next")
	var calls, fakeCalls, checks int
	actual := modelFunc(func(_ context.Context, _ abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		calls++
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	fake := modelFunc(func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error { fakeCalls++; return nil })
	fakeState, err := abi.NewAgentState(fake, nextLog, nil)
	if err != nil {
		t.Fatal(err)
	}
	_, fakeBus := bus.New()
	var initial abi.RuntimeContext
	check := func(runtime abi.RuntimeContext) {
		t.Helper()
		if runtime.InvocationModel == nil || runtime.State != initial.State || runtime.RunID != initial.RunID || runtime.Workspace != "/actual" || runtime.InvocationSessionID != "bound" {
			t.Fatal("runtime-owned binding rewritten", runtime)
		}
		if runtime.State.Session().ID() != "next" {
			t.Fatal("fixture did not exercise mutable session")
		}
		checks++
	}
	outer := modelLayer{"outer", func(ctx context.Context, runtime abi.RuntimeContext, request abi.ModelRequest, emit func(wire.StreamChunk) error, next abi.ModelFunc) error {
		initial = runtime
		if err := runtime.State.SetSession(nextLog); err != nil {
			return err
		}
		runtime.InvocationModel, runtime.State = fake, fakeState
		runtime.InvocationSessionID, runtime.RunID, runtime.Workspace, runtime.Capabilities = "forged", "forged", "/forged", fakeBus
		return next(ctx, runtime, request, emit)
	}}
	inner := modelLayer{"inner", func(ctx context.Context, runtime abi.RuntimeContext, request abi.ModelRequest, emit func(wire.StreamChunk) error, next abi.ModelFunc) error {
		check(runtime)
		return next(ctx, runtime, request, emit)
	}}
	gate := requestGate{"gate", func(runtime abi.RuntimeContext, _ abi.ModelRequest) error { check(runtime); return nil }}
	agent := build(t, core.NewBuilder(actual, first).Workspace("/actual").Middleware(outer).Middleware(inner).Middleware(gate))
	if err := agent.Run(t.Context(), nil, discard); err != nil || calls != 1 || fakeCalls != 0 || checks != 2 {
		t.Fatal(err, calls, fakeCalls, checks)
	}
	for _, r := range records(t, nextLog) {
		if r.ModelIntent != nil || r.ModelOutcome != nil {
			t.Fatal("in-flight facts moved to next session")
		}
	}
}

func TestToolReplacementDuringModelStreamBindsNextRoundOnly(t *testing.T) {
	var firstCalls, replacementCalls, round int
	var first *testTool
	first = &testTool{execute: func(_ context.Context, x abi.ToolExecutionContext, c wire.ToolCall) (wire.ToolResult, error) {
		firstCalls++
		if x.ActualTool != first {
			t.Fatal("actual binding changed")
		}
		return wire.ToolResult{CallID: c.ID}, nil
	}}
	replacement := &testTool{execute: func(_ context.Context, _ abi.ToolExecutionContext, c wire.ToolCall) (wire.ToolResult, error) {
		replacementCalls++
		return wire.ToolResult{CallID: c.ID}, nil
	}}
	var agent *core.Agent
	model := modelFunc(func(_ context.Context, _ abi.ModelRequest, e func(wire.StreamChunk) error) error {
		round++
		if round == 1 {
			if _, err := agent.State().Tools().Replace(replacement); err != nil {
				return err
			}
		}
		if round <= 2 {
			if err := e(callChunk(string(rune('a' + round)))); err != nil {
				return err
			}
		}
		return e(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	agent = build(t, core.NewBuilder(model, core.NewMemorySession("hot-tool")).Tool(first))
	if err := agent.Run(t.Context(), nil, discard); err != nil || firstCalls != 1 || replacementCalls != 1 {
		t.Fatal(err, firstCalls, replacementCalls)
	}
}

func TestFinalAdmissionSeesLateRequestAndUsesIndependentCopies(t *testing.T) {
	checks, calls := 0, 0
	model := modelFunc(func(_ context.Context, request abi.ModelRequest, emit func(wire.StreamChunk) error) error {
		calls++
		if request.Messages[0].Text() != "late" || string(request.Tools[0].Parameters) != `{"type":"object"}` || request.Config.AllowedTools[0] != "test" {
			t.Fatal("validator changed sent snapshot", request)
		}
		return emit(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	late := modelLayer{"late", func(ctx context.Context, r abi.RuntimeContext, q abi.ModelRequest, e func(wire.StreamChunk) error, n abi.ModelFunc) error {
		q.Messages = []wire.Message{wire.TextMessage(wire.RoleUser, "late")}
		q.Config.AllowedTools = []string{"test"}
		return n(ctx, r, q, e)
	}}
	mutating := requestGate{"mutating", func(_ abi.RuntimeContext, q abi.ModelRequest) error {
		checks++
		q.Messages[0].Content[0].Text = "changed"
		q.Tools[0].Parameters[0] = '!'
		q.Config.AllowedTools[0] = "changed"
		return nil
	}}
	checking := requestGate{"checking", func(_ abi.RuntimeContext, q abi.ModelRequest) error {
		checks++
		if q.Messages[0].Text() != "late" || !json.Valid(q.Tools[0].Parameters) || q.Config.AllowedTools[0] != "test" {
			t.Fatal("validator copies alias")
		}
		return nil
	}}
	if err := build(t, core.NewBuilder(model, core.NewMemorySession("copies")).Tool(&testTool{}).Middleware(mutating).Middleware(late).Middleware(checking)).Run(t.Context(), nil, discard); err != nil || calls != 1 || checks != 2 {
		t.Fatal(err, calls, checks)
	}
}

func TestAdmissionRejectThenRepairRechecksWithoutModelIO(t *testing.T) {
	denied := errors.New("request rejected")
	checks, calls := 0, 0
	gate := requestGate{"gate", func(_ abi.RuntimeContext, q abi.ModelRequest) error {
		checks++
		if q.Messages[0].Text() != "repaired" {
			return denied
		}
		return nil
	}}
	model := modelFunc(func(_ context.Context, q abi.ModelRequest, e func(wire.StreamChunk) error) error {
		calls++
		if q.Messages[0].Text() != "repaired" {
			t.Fatal("rejected request reached model")
		}
		return e(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	repair := modelLayer{"repair", func(ctx context.Context, r abi.RuntimeContext, q abi.ModelRequest, e func(wire.StreamChunk) error, n abi.ModelFunc) error {
		if err := n(ctx, r, q, e); !errors.Is(err, denied) {
			return errors.New("first request did not fail admission")
		}
		q.Messages = []wire.Message{wire.TextMessage(wire.RoleUser, "repaired")}
		return n(ctx, r, q, e)
	}}
	if err := build(t, core.NewBuilder(model, core.NewMemorySession("repair")).Middleware(repair).Middleware(gate)).Run(t.Context(), []wire.Message{wire.TextMessage(wire.RoleUser, "too large")}, discard); err != nil || checks != 2 || calls != 1 {
		t.Fatal(err, checks, calls)
	}
}

func TestPublishedAttemptFailureCannotBecomeSuccessfulRetry(t *testing.T) {
	for _, kind := range []wire.ChunkKind{wire.ChunkText, wire.ChunkReasoning, wire.ChunkUsage, wire.ChunkContentBlock, wire.ChunkToolCall} {
		t.Run(string(kind), func(t *testing.T) {
			failure := errors.New("interrupted attempt")
			calls, commits := 0, 0
			model := modelFunc(func(_ context.Context, _ abi.ModelRequest, e func(wire.StreamChunk) error) error {
				calls++
				chunk := wire.StreamChunk{Kind: kind}
				switch kind {
				case wire.ChunkText, wire.ChunkReasoning:
					chunk.Text = "abandoned"
				case wire.ChunkUsage:
					chunk.Usage = &wire.Usage{InputTokens: 1}
				case wire.ChunkContentBlock:
					chunk.ContentBlock = &wire.ContentBlock{Kind: wire.ContentText, Text: "abandoned"}
				case wire.ChunkToolCall:
					chunk = callChunk("abandoned")
				}
				if err := e(chunk); err != nil {
					return err
				}
				return failure
			})
			retry := modelLayer{"retry", func(ctx context.Context, r abi.RuntimeContext, q abi.ModelRequest, e func(wire.StreamChunk) error, n abi.ModelFunc) error {
				_ = n(ctx, r, q, e)
				_ = n(ctx, r, q, e)
				_ = e(wire.StreamChunk{Kind: wire.ChunkFinish})
				return nil
			}}
			session := core.NewMemorySession("published")
			err := build(t, core.NewBuilder(model, session).Tool(&testTool{}).Middleware(retry)).Run(t.Context(), nil, func(event abi.Event) error {
				if event.Kind == abi.EventMessageCommitted {
					commits++
				}
				return nil
			})
			if !errors.Is(err, failure) || calls != 1 || commits != 0 {
				t.Fatal("published failure swallowed", err, calls, commits)
			}
			for _, r := range records(t, session) {
				if r.ToolIntent != nil || (r.ModelOutcome != nil && r.ModelOutcome.Status == abi.OutcomeSucceeded) {
					t.Fatal("failure authorized execution or success")
				}
			}
		})
	}
}

func TestBufferedPartialAttemptCanRetryAndOldCallbackIsClosed(t *testing.T) {
	failure := errors.New("private attempt failure")
	calls := 0
	var old func(wire.StreamChunk) error
	model := modelFunc(func(_ context.Context, _ abi.ModelRequest, e func(wire.StreamChunk) error) error {
		calls++
		if calls == 1 {
			old = e
			if err := e(wire.StreamChunk{Kind: wire.ChunkText, Text: "private"}); err != nil {
				return err
			}
			return failure
		}
		if err := old(wire.StreamChunk{Kind: wire.ChunkText, Text: "late"}); err == nil {
			t.Fatal("old callback leaked into new attempt")
		}
		if err := e(wire.StreamChunk{Kind: wire.ChunkText, Text: "selected"}); err != nil {
			return err
		}
		return e(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	retry := modelLayer{"buffer", func(ctx context.Context, r abi.RuntimeContext, q abi.ModelRequest, e func(wire.StreamChunk) error, n abi.ModelFunc) error {
		var private []wire.StreamChunk
		if err := n(ctx, r, q, func(chunk wire.StreamChunk) error {
			private = append(private, wire.CloneStreamChunk(chunk))
			return nil
		}); !errors.Is(err, failure) {
			return errors.New("private fixture did not fail")
		}
		if len(private) != 1 {
			t.Fatal("private buffering changed")
		}
		return n(ctx, r, q, e)
	}}
	session := core.NewMemorySession("buffered")
	if err := build(t, core.NewBuilder(model, session).Middleware(retry)).Run(t.Context(), nil, discard); err != nil || calls != 2 {
		t.Fatal(err, calls)
	}
	for _, m := range mustMessages(t, session) {
		if m.Role == wire.RoleAssistant && m.Text() != "selected" {
			t.Fatal("buffered failure mixed with selected reply", m.Text())
		}
	}
}

func TestPublishedWrapperPostprocessingFailureCannotBeSwallowed(t *testing.T) {
	failure := errors.New("wrapper postprocessing failed")
	calls, commits := 0, 0
	model := modelFunc(func(_ context.Context, _ abi.ModelRequest, e func(wire.StreamChunk) error) error {
		calls++
		if err := e(wire.StreamChunk{Kind: wire.ChunkText, Text: "partial"}); err != nil {
			return err
		}
		return e(wire.StreamChunk{Kind: wire.ChunkFinish})
	})
	inner := modelLayer{"postprocess", func(ctx context.Context, r abi.RuntimeContext, q abi.ModelRequest, e func(wire.StreamChunk) error, n abi.ModelFunc) error {
		if err := n(ctx, r, q, func(chunk wire.StreamChunk) error {
			if chunk.Kind == wire.ChunkFinish {
				return nil
			}
			return e(chunk)
		}); err != nil {
			return err
		}
		return failure
	}}
	outer := modelLayer{"retry", func(ctx context.Context, r abi.RuntimeContext, q abi.ModelRequest, e func(wire.StreamChunk) error, n abi.ModelFunc) error {
		_ = n(ctx, r, q, e)
		_ = n(ctx, r, q, e)
		_ = e(wire.StreamChunk{Kind: wire.ChunkFinish})
		return nil
	}}
	err := build(t, core.NewBuilder(model, core.NewMemorySession("postprocess")).Middleware(outer).Middleware(inner)).Run(t.Context(), nil, func(event abi.Event) error {
		if event.Kind == abi.EventMessageCommitted {
			commits++
		}
		return nil
	})
	if !errors.Is(err, failure) || calls != 1 || commits != 0 {
		t.Fatal("published wrapper error hidden", err, calls, commits)
	}
}

func TestConcurrentModelNextCannotReenterShortCircuitingWrapper(t *testing.T) {
	var enters atomic.Int32
	entered, release := make(chan struct{}), make(chan struct{})
	inner := modelLayer{"inner", func(_ context.Context, _ abi.RuntimeContext, _ abi.ModelRequest, _ func(wire.StreamChunk) error, _ abi.ModelFunc) error {
		enters.Add(1)
		close(entered)
		<-release
		return nil
	}}
	outer := modelLayer{"outer", func(ctx context.Context, r abi.RuntimeContext, q abi.ModelRequest, e func(wire.StreamChunk) error, n abi.ModelFunc) error {
		done := make(chan struct{})
		go func() { defer close(done); _ = n(ctx, r, q, e) }()
		<-entered
		_ = n(ctx, r, q, e)
		close(release)
		<-done
		return nil
	}}
	err := build(t, core.NewBuilder(textModel("must not run"), core.NewMemorySession("short-circuit")).Middleware(outer).Middleware(inner)).Run(t.Context(), nil, discard)
	if err == nil || !strings.Contains(err.Error(), "concurrent model next") || enters.Load() != 1 {
		t.Fatal("parallel continuation entered inner twice", err, enters.Load())
	}
}

func mustMessages(t *testing.T, s abi.ConversationReader) []wire.Message {
	t.Helper()
	m, e := s.Messages(t.Context())
	if e != nil {
		t.Fatal(e)
	}
	return m
}

func TestToolRepeatedContinuationDoesNotReenterInnerWrapper(t *testing.T) {
	for _, concurrent := range []bool{false, true} {
		t.Run(map[bool]string{false: "serial", true: "concurrent"}[concurrent], func(t *testing.T) {
			var enters atomic.Int32
			entered, release := make(chan struct{}), make(chan struct{})
			inner := wrapperFixture{"inner", func(_ context.Context, _ abi.ToolExecutionContext, c wire.ToolCall, _ abi.ToolFunc) (wire.ToolResult, error) {
				enters.Add(1)
				close(entered)
				if concurrent {
					<-release
				}
				return wire.ToolResult{CallID: c.ID}, nil
			}}
			outer := wrapperFixture{"outer", func(ctx context.Context, x abi.ToolExecutionContext, c wire.ToolCall, n abi.ToolFunc) (wire.ToolResult, error) {
				if !concurrent {
					_, _ = n(ctx, x, c)
					_, _ = n(ctx, x, c)
				} else {
					done := make(chan struct{})
					go func() { defer close(done); _, _ = n(ctx, x, c) }()
					<-entered
					_, _ = n(ctx, x, c)
					close(release)
					<-done
				}
				return wire.ToolResult{CallID: c.ID}, nil
			}}
			err := build(t, core.NewBuilder(batchModel([]string{"one"}), core.NewMemorySession("single-use")).Tool(&testTool{}).Middleware(outer).Middleware(inner)).Run(t.Context(), nil, discard)
			if err == nil || !strings.Contains(err.Error(), "only be called once") || enters.Load() != 1 {
				t.Fatal("duplicate continuation entered wrapper or lost violation", err, enters.Load())
			}
		})
	}
}
