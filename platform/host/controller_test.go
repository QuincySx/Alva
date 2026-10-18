// INPUT: context, errors, strings, testing, app.Runner contracts, kernel/abi, llm/wire.
// OUTPUT: Root execution, cancellation, bounded replay and observer ownership regressions.
// POS: [platform / native execution host] Deterministic no-network host lifecycle tests.
package host

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type runnerFunc func(context.Context, []wire.Message, func(abi.Event) error) error

func (f runnerFunc) Run(ctx context.Context, input []wire.Message, emit func(abi.Event) error) error {
	return f(ctx, input, emit)
}
func waitRun(t *testing.T, c *Controller, id string) RunSnapshot {
	t.Helper()
	c.mu.Lock()
	done := c.runs[id].done
	c.mu.Unlock()
	select {
	case <-done:
	case <-time.After(time.Second):
		t.Fatal("run did not exit")
	}
	snapshot, err := c.Snapshot(id)
	if err != nil {
		t.Fatal(err)
	}
	return snapshot
}

func TestAdmissionContextAndDetachedObserverDoNotOwnRun(t *testing.T) {
	started, release := make(chan struct{}), make(chan struct{})
	c := NewController(runnerFunc(func(ctx context.Context, input []wire.Message, emit func(abi.Event) error) error {
		close(started)
		select {
		case <-release:
			return emit(abi.Event{Kind: abi.EventTextDelta, Text: "completed independently"})
		case <-ctx.Done():
			return ctx.Err()
		}
	}), 8)
	admission, cancel := context.WithCancel(context.Background())
	id, err := c.StartPrompt(admission, "hello")
	if err != nil {
		t.Fatal(err)
	}
	<-started
	_, unsubscribe, err := c.Subscribe(id, 0)
	if err != nil {
		t.Fatal(err)
	}
	unsubscribe()
	unsubscribe()
	cancel()
	if _, err = c.StartPrompt(context.Background(), "concurrent"); !errors.Is(err, ErrRunActive) {
		t.Fatalf("concurrent run accepted: %v", err)
	}
	close(release)
	snapshot := waitRun(t, c, id)
	if snapshot.Status != RunCompleted || snapshot.Text != "completed independently" {
		t.Fatalf("observer cancelled run: %+v", snapshot)
	}
}

func TestTerminalUsesRunnerResultNotLateStopBit(t *testing.T) {
	for _, test := range []struct {
		name   string
		result error
		want   RunStatus
	}{{"success", nil, RunCompleted}, {"independent failure", errors.New("fixture failed"), RunFailed}, {"cancelled", context.Canceled, RunCancelled}, {"cancelled and settlement failed", errors.Join(context.Canceled, errors.New("commit failed")), RunFailed}, {"session timeout is not pure cancel", &abi.SessionError{Operation: "flush", SessionID: "actual", Err: context.DeadlineExceeded}, RunFailed}} {
		t.Run(test.name, func(t *testing.T) {
			started, release := make(chan struct{}), make(chan struct{})
			c := NewController(runnerFunc(func(context.Context, []wire.Message, func(abi.Event) error) error {
				close(started)
				<-release
				return test.result
			}), 4)
			id, _ := c.StartPrompt(context.Background(), "fixture")
			<-started
			if err := c.Stop(id); err != nil {
				t.Fatal(err)
			}
			close(release)
			snapshot := waitRun(t, c, id)
			if snapshot.Status != test.want {
				t.Fatalf("late stop relabelled result: %+v", snapshot)
			}
			if strings.Contains(test.name, "settlement") && !strings.Contains(snapshot.Error, "commit failed") {
				t.Fatalf("settlement evidence lost: %+v", snapshot)
			}
		})
	}
}

func TestStopAndCloseWaitForActualExit(t *testing.T) {
	cancelled, release := make(chan struct{}), make(chan struct{})
	c := NewController(runnerFunc(func(ctx context.Context, _ []wire.Message, _ func(abi.Event) error) error {
		<-ctx.Done()
		close(cancelled)
		<-release
		return ctx.Err()
	}), 4)
	id, _ := c.StartPrompt(context.Background(), "fixture")
	deadline, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
	defer cancel()
	if err := c.Close(deadline); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("Close did not wait: %v", err)
	}
	<-cancelled
	if snapshot, _ := c.Snapshot(id); snapshot.Status != RunRunning {
		t.Fatalf("Stop declared exit early: %+v", snapshot)
	}
	if _, err := c.StartPrompt(context.Background(), "another"); !errors.Is(err, ErrControllerClosed) {
		t.Fatal(err)
	}
	close(release)
	if err := c.Close(context.Background()); err != nil {
		t.Fatal(err)
	}
	if snapshot := waitRun(t, c, id); snapshot.Status != RunCancelled {
		t.Fatalf("wrong status: %+v", snapshot)
	}
}

func TestSlowObserverAndExpiredReplayAreExplicit(t *testing.T) {
	started, release := make(chan struct{}), make(chan struct{})
	c := NewController(runnerFunc(func(ctx context.Context, _ []wire.Message, emit func(abi.Event) error) error {
		close(started)
		<-release
		for index := 0; index < 20; index++ {
			if err := emit(abi.Event{Kind: abi.EventTextDelta, Text: "x"}); err != nil {
				return err
			}
		}
		return nil
	}), 2)
	id, _ := c.StartPrompt(context.Background(), "fixture")
	<-started
	events, unsubscribe, err := c.Subscribe(id, 0)
	if err != nil {
		t.Fatal(err)
	}
	defer unsubscribe()
	close(release)
	snapshot := waitRun(t, c, id)
	if snapshot.Status != RunCompleted || len(snapshot.Text) != 20 {
		t.Fatalf("slow observer blocked run: %+v", snapshot)
	}
	count := 0
	for range events {
		count++
	}
	if count > subscriberBuffer {
		t.Fatalf("subscriber buffer not bounded: %d", count)
	}
	if _, _, err = c.Subscribe(id, 0); !errors.Is(err, ErrHistoryExpired) {
		t.Fatalf("expired cursor accepted: %v", err)
	}
	if _, _, err = c.Subscribe(id, snapshot.HighWater+1); err == nil {
		t.Fatal("future cursor accepted")
	}
}

func TestReplayByteBudgetAndOversizedEventFailExplicitly(t *testing.T) {
	t.Run("oversized", func(t *testing.T) {
		c := NewController(runnerFunc(func(_ context.Context, _ []wire.Message, emit func(abi.Event) error) error {
			return emit(abi.Event{Kind: abi.EventToolCompleted, ToolResult: &wire.ToolResult{CallID: "c", Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: strings.Repeat("x", maxEventBytes)}}}})
		}), 4096)
		id, _ := c.StartPrompt(context.Background(), "fixture")
		snapshot := waitRun(t, c, id)
		if snapshot.Status != RunFailed || !strings.Contains(snapshot.Error, "presentation limit") {
			t.Fatalf("oversize falsely succeeded: %+v", snapshot)
		}
	})
	t.Run("ignored callback failure", func(t *testing.T) {
		c := NewController(runnerFunc(func(_ context.Context, _ []wire.Message, emit func(abi.Event) error) error {
			_ = emit(abi.Event{Kind: abi.EventTextDelta, Text: strings.Repeat("x", maxEventBytes+1)})
			return nil
		}), 4)
		id, _ := c.StartPrompt(context.Background(), "fixture")
		snapshot := waitRun(t, c, id)
		if snapshot.Status != RunFailed {
			t.Fatalf("ignored callback overflow became success: %+v", snapshot)
		}
	})
	t.Run("replay byte bound", func(t *testing.T) {
		c := NewController(runnerFunc(func(_ context.Context, _ []wire.Message, emit func(abi.Event) error) error {
			for index := 0; index < 10; index++ {
				if err := emit(abi.Event{Kind: abi.EventToolStarted, Text: strings.Repeat("x", 200<<10)}); err != nil {
					return err
				}
			}
			return nil
		}), 4096)
		id, _ := c.StartPrompt(context.Background(), "fixture")
		snapshot := waitRun(t, c, id)
		c.mu.Lock()
		retained := c.runs[id].replayBytes
		count := len(c.runs[id].events)
		c.mu.Unlock()
		if snapshot.Status != RunCompleted || retained > maxReplayBytes || count >= 11 {
			t.Fatalf("replay was not byte bounded: %d %d %+v", retained, count, snapshot)
		}
	})
}

func TestSubscriberCannotMutateRetainedEvents(t *testing.T) {
	c := NewController(runnerFunc(func(_ context.Context, _ []wire.Message, emit func(abi.Event) error) error {
		return emit(abi.Event{Kind: abi.EventToolStarted, ToolCall: &wire.ToolCall{ID: "call", Name: "fixture", Arguments: []byte(`{"value":1}`)}})
	}), 8)
	id, _ := c.StartPrompt(context.Background(), "fixture")
	snapshot := waitRun(t, c, id)
	first, _, _ := c.Subscribe(id, 0)
	event := <-first
	event.Event.ToolCall.Arguments[0] = 'X'
	second, _, _ := c.Subscribe(id, 0)
	retained := <-second
	if retained.Event.ToolCall.Arguments[0] != '{' || snapshot.Status != RunCompleted {
		t.Fatal("subscriber mutated shared history")
	}
}

func TestSnapshotUsesActualCommittedResponseNotDraftReasoningOrToolPreface(t *testing.T) {
	c := NewController(runnerFunc(func(_ context.Context, _ []wire.Message, emit func(abi.Event) error) error {
		for _, event := range []abi.Event{{Kind: abi.EventTextDelta, Text: "tool preface"}, {Kind: abi.EventMessageCommitted, Message: &wire.Message{Role: wire.RoleAssistant, Content: []wire.ContentBlock{{Kind: wire.ContentText, Text: "tool preface"}, {Kind: wire.ContentToolCall, ToolCall: &wire.ToolCall{ID: "call", Name: "fixture", Arguments: []byte(`{}`)}}}}}, {Kind: abi.EventTextDelta, Text: "streamed draft"}, {Kind: abi.EventMessageCommitted, Message: &wire.Message{Role: wire.RoleAssistant, Content: []wire.ContentBlock{{Kind: wire.ContentReasoning, Text: "not answer"}, {Kind: wire.ContentText, Text: "AfterModel actual committed answer"}}}}} {
			if err := emit(event); err != nil {
				return err
			}
		}
		return nil
	}), 8)
	id, _ := c.StartPrompt(context.Background(), "fixture")
	snapshot := waitRun(t, c, id)
	if snapshot.Status != RunCompleted || snapshot.Text != "AfterModel actual committed answer" {
		t.Fatal("draft/intermediate content became final", snapshot)
	}
	first, _, err := c.Subscribe(id, 0)
	if err != nil {
		t.Fatal(err)
	}
	for event := range first {
		if event.Event.Message != nil {
			event.Event.Message.Content[0].Text = "mutated"
		}
	}
	second, _, _ := c.Subscribe(id, 0)
	for event := range second {
		if event.Event.Message != nil && event.Event.Message.Content[0].Text == "mutated" {
			t.Fatal("subscriber mutated committed message replay")
		}
	}
}

func TestCancelledAdmissionNeverStartsRunner(t *testing.T) {
	called := false
	c := NewController(runnerFunc(func(context.Context, []wire.Message, func(abi.Event) error) error { called = true; return nil }), 4)
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := c.StartPrompt(ctx, "fixture"); !errors.Is(err, context.Canceled) || called {
		t.Fatalf("cancelled admission started runner: %v", err)
	}
}

// A nil-capable implementation can be safe to invoke itself; admission still
// rejects it consistently with nil pointers/functions before starting a run.
type nilMapRunner map[string]string

func (nilMapRunner) Run(context.Context, []wire.Message, func(abi.Event) error) error { return nil }
func TestControllerRejectsTypedNilRunner(t *testing.T) {
	c := NewController(nilMapRunner(nil), 4)
	defer c.Close(context.Background())
	if id, err := c.StartPrompt(context.Background(), "fixture"); err == nil || id != "" {
		t.Fatalf("typed-nil runner admitted: id=%q err=%v", id, err)
	}
}
