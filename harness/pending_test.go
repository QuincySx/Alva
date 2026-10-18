// INPUT: context, errors, testing, agent/core, kernel/abi, llm/wire.
// OUTPUT: Pending atomic boundary injection, uncertainty/no-retry, binding and queue-limit regressions.
// POS: [L5 harness] Optional harness steering preserves committed facts without pretending a model accepted them.
package harness

import (
	"context"
	"errors"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/sdk/agent/core"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

func pendingRuntime(t *testing.T, id string) abi.RuntimeContext {
	runtime := testRuntime(t, id)
	runtime.RunID = "run"
	return runtime
}
func TestPendingAtomicCommitCancelAndRepeatedBoundary(t *testing.T) {
	ctx := context.Background()
	plugin := NewPendingPlugin("actual")
	runtime := pendingRuntime(t, "actual")
	if _, err := plugin.Service().Enqueue(ctx, "too early"); !errors.Is(err, ErrPendingInactive) {
		t.Fatal(err)
	}
	if err := plugin.OnRunStart(ctx, runtime); err != nil {
		t.Fatal(err)
	}
	cancelled, err := plugin.Service().Enqueue(ctx, "cancel me")
	if err != nil {
		t.Fatal(err)
	}
	if ok, err := plugin.Service().Cancel(ctx, cancelled.ID); err != nil || !ok {
		t.Fatal("pending cancel failed", err)
	}
	queued, err := plugin.Service().Enqueue(ctx, "steer 日志🙂")
	if err != nil {
		t.Fatal(err)
	}
	request := abi.ModelRequest{}
	if err = plugin.BeforeModel(ctx, runtime, &request); err != nil {
		t.Fatal(err)
	}
	if len(request.Messages) != 2 || request.Messages[1].ID != queued.ID || request.Messages[1].Text() != queued.Text {
		t.Fatal("pending request has wrong committed message")
	}
	if ok, err := plugin.Service().Cancel(ctx, queued.ID); err != nil || ok {
		t.Fatal("committed message retracted", err)
	}
	state := plugin.Service().Snapshot(false)
	if len(state) != 1 || state[0].Status != PendingCommitted || state[0].CommittedAt == nil {
		t.Fatal("not marked committed", state)
	}
	*state[0].CommittedAt = state[0].CommittedAt.AddDate(1, 0, 0)
	if plugin.Service().Snapshot(false)[0].CommittedAt.Equal(*state[0].CommittedAt) {
		t.Fatal("snapshot aliases service")
	}
	if err = plugin.BeforeModel(ctx, runtime, &request); err != nil {
		t.Fatal(err)
	}
	messages, err := runtime.State.Session().Messages(ctx)
	if err != nil || len(messages) != 1 {
		t.Fatal("boundary replayed user message", err)
	}
	_ = plugin.OnRunEnd(ctx, runtime, nil)
	if _, err = plugin.Service().Enqueue(ctx, "after exit"); !errors.Is(err, ErrPendingInactive) {
		t.Fatal("finished run admits pending", err)
	}
}

type pendingFailLog struct {
	*core.MemorySession
	failure  error
	attempts int
}

func (log *pendingFailLog) AppendRecords(context.Context, []abi.SessionRecord) ([]abi.SessionRecord, error) {
	log.attempts++
	return nil, log.failure
}
func TestPendingCommitFailureRemainsUnknownAndNeverAutomaticallyRetries(t *testing.T) {
	ctx := context.Background()
	failure := errors.New("commit fixture failed")
	log := &pendingFailLog{MemorySession: core.NewMemorySession("actual"), failure: failure}
	runtime := pendingRuntime(t, "actual")
	_ = runtime.State.SetSession(log)
	plugin := NewPendingPlugin("actual")
	if err := plugin.OnRunStart(ctx, runtime); err != nil {
		t.Fatal(err)
	}
	_, _ = plugin.Service().Enqueue(ctx, "must not pretend delivered")
	request := abi.ModelRequest{}
	if err := plugin.BeforeModel(ctx, runtime, &request); !errors.Is(err, failure) {
		t.Fatal("commit error lost", err)
	}
	if len(request.Messages) != 0 || plugin.Service().Snapshot(false)[0].Status != PendingUnknown {
		t.Fatal("failed append claimed delivery")
	}
	if err := plugin.BeforeModel(ctx, runtime, &request); err != nil {
		t.Fatal(err)
	}
	if log.attempts != 1 {
		t.Fatal("uncertain commit automatically retried")
	}
}

func TestPendingBindingAndHardLimits(t *testing.T) {
	plugin := NewPendingPlugin("parent")
	runtime := pendingRuntime(t, "child")
	if err := plugin.OnRunStart(context.Background(), runtime); err == nil {
		t.Fatal("copied plugin granted parent queue to child")
	}
	runtime = pendingRuntime(t, "parent")
	if err := plugin.OnRunStart(context.Background(), runtime); err != nil {
		t.Fatal(err)
	}
	for index := 0; index < 64; index++ {
		if _, err := plugin.Service().Enqueue(context.Background(), strings.Repeat("x", 16<<10)); err != nil {
			t.Fatal(err)
		}
	}
	if _, err := plugin.Service().Enqueue(context.Background(), "overflow"); !errors.Is(err, ErrPendingFull) {
		t.Fatal("byte limit bypassed", err)
	}
}
