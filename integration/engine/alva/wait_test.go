// INPUT: Optional committed waiting runners and actual adapter ownership/error boundaries.
// OUTPUT: Exclusive waiting receipts only after cleanup, strict no-prompt continuation and cancellation regressions.
// POS: [L7 adapter acceptance] Fault injection verifies the engine protocol; full Hub tests exercise real Agents.
package alva

import (
	"context"
	"errors"
	"testing"

	"github.com/QuincySx/Alva/harness"
	"github.com/QuincySx/Alva/integration/engine"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type waitingFixture struct {
	wait     *engine.WaitCheckpoint
	closed   bool
	closeErr error
	cancel   context.CancelFunc
	input    []wire.Message
}

func (r *waitingFixture) Run(_ context.Context, input []wire.Message, _ func(abi.Event) error) error {
	r.input = input
	if r.cancel != nil {
		r.cancel()
	}
	return nil
}
func (r *waitingFixture) Close(context.Context) error { r.closed = true; return r.closeErr }
func (r *waitingFixture) WaitCheckpoint() *engine.WaitCheckpoint {
	if !r.closed {
		panic("waiting read before physical cleanup")
	}
	return r.wait
}
func TestWaitingReceiptFollowsActualCleanupAndCannotHideFailure(t *testing.T) {
	for _, mode := range []string{"waiting", "resume", "invalid", "unsupported", "cleanup-failed", "cancelled"} {
		t.Run(mode, func(t *testing.T) {
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			runner := &waitingFixture{wait: &engine.WaitCheckpoint{Revision: 3, Requests: 1, ExpiresAt: 1000}}
			if mode == "invalid" {
				runner.wait.Revision = 0
			}
			if mode == "cleanup-failed" {
				runner.closeErr = errors.New("cleanup failed")
			}
			if mode == "cancelled" {
				runner.cancel = cancel
			}
			adapter := NewConfigured(func(context.Context, string, engine.Request, bool) (harness.Runner, error) { return runner, nil }, nil, FactoryOptions{DurableWaits: mode != "unsupported"})
			request := engine.Request{Prompt: "original input"}
			if mode == "resume" {
				request = engine.Request{ResumeSession: "native", WaitRevision: 3}
			}
			var terminal engine.Event
			var contract engine.StreamContract
			if err := adapter.Execute(ctx, request, func(e engine.Event) error {
				if e.Kind == engine.Completed {
					if !runner.closed {
						t.Error("completion preceded cleanup")
					}
					terminal = e
				}
				return contract.Accept(e)
			}); err != nil {
				t.Fatal(err)
			}
			want := engine.Failed
			if mode == "waiting" || mode == "resume" {
				want = engine.Waiting
			}
			if mode == "cancelled" {
				want = engine.Cancelled
			}
			if terminal.Outcome != want || (terminal.Wait != nil) != (want == engine.Waiting) {
				t.Fatal(terminal)
			}
			if mode == "resume" && len(runner.input) != 0 {
				t.Fatal("continuation replayed a prompt", runner.input)
			}
		})
	}
}
func TestDurableResumeCannotLaunchOrdinaryOrFreshFactory(t *testing.T) {
	for _, request := range []engine.Request{
		{WaitRevision: 1, Prompt: "replayed", ResumeSession: "native"},
		{WaitRevision: 1},
		{WaitRevision: 1, ResumeSession: "native"},
	} {
		called := false
		adapter := New(func(context.Context, string, engine.Request, bool) (harness.Runner, error) {
			called = true
			return nil, errors.New("must not launch")
		})
		if err := adapter.Execute(t.Context(), request, func(engine.Event) error { return nil }); err == nil || called {
			t.Fatal(request, called, err)
		}
	}
}
