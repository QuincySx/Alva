// INPUT: Approved same-binary hook process emitting explicit control JSON and actual SDK tool preparation.
// OUTPUT: Duplicate/case/surrogate hook control rejection without executing the selected tool.
// POS: [L4 extension] Native hook control output must have one unambiguous typed meaning before mutation.
package hooks

import (
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"testing"
	"time"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestHookControlHelper(t *testing.T) {
	value := os.Getenv("ALVA_HOOK_CONTROL_FIXTURE")
	if value == "" {
		return
	}
	io.ReadAll(os.Stdin)
	fmt.Print(value)
	os.Exit(0)
}
func TestHookRejectsAmbiguousControlBeforeToolExecution(t *testing.T) {
	for _, value := range []string{`{"continue":false,"continue":true}`, `{"Permission_Decision":"allow"}`, `{"additional_context":"\ud800"}`, `{"updated_input":{"path":"first","path":"last"}}`} {
		t.Run(value, func(t *testing.T) {
			f := newFixture(t)
			entry := f.entry(t, "strict-output", PreToolUse, "unused")
			entry.Process.Args = []string{"-test.run=^TestHookControlHelper$", "--"}
			entry.Process.Env = map[string]string{"ALVA_HOOK_CONTROL_FIXTURE": value}
			p := f.plugin(t, []Entry{entry}, nil)
			executed := false
			tool := &fixtureTool{execute: func(context.Context, abi.ToolExecutionContext, wire.ToolCall) (wire.ToolResult, error) {
				executed = true
				return wire.ToolResult{CallID: "call"}, nil
			}}
			if err := build(t, f, p, toolModel(nil), tool).Run(t.Context(), nil, discard); err == nil {
				t.Error("ambiguous hook control accepted")
			}
			if executed {
				t.Fatal("ambiguous control reached actual tool")
			}
		})
	}
}

func TestHookRunEndCancelsAndJoinsNotifyAdmission(t *testing.T) {
	f := newFixture(t)
	entered, release, observed := make(chan struct{}), make(chan struct{}), make(chan struct{})
	p := f.plugin(t, []Entry{f.entry(t, "notify", Notification, "success")}, func(ctx context.Context, _ Activity) error {
		close(entered)
		<-release
		select {
		case <-ctx.Done():
			close(observed)
			return ctx.Err()
		default:
			return nil
		}
	})
	state, _ := abi.NewAgentState(modelFunc(func(context.Context, abi.ModelRequest, func(wire.StreamChunk) error) error { return nil }), f.log, nil)
	runtime := abi.RuntimeContext{State: state, RunID: "run", Workspace: f.cwd}
	if err := p.OnRunStart(t.Context(), runtime); err != nil {
		t.Fatal(err)
	}
	done := make(chan error, 1)
	go func() { done <- p.Notify(context.Background(), runtime, "actual notification") }()
	select {
	case <-entered:
	case <-time.After(time.Second):
		t.Fatal("notification did not enter")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 40*time.Millisecond)
	err := p.OnRunEnd(ctx, runtime, nil)
	cancel()
	active := p.runID
	close(release)
	notifyErr := <-done
	if !errors.Is(err, context.DeadlineExceeded) || !errors.Is(err, ErrFinalization) || active != "run" {
		t.Error("run end discarded outstanding hook ownership", err, active)
	}
	if !errors.Is(notifyErr, context.Canceled) {
		t.Error("run end did not cancel notification callback", notifyErr)
	}
	select {
	case <-observed:
	default:
		t.Error("callback did not observe run end")
	}
	if err := p.OnRunEnd(context.Background(), runtime, nil); err != nil {
		t.Fatal(err)
	}
}
