// INPUT: Real compiled graphs, explicit cancellation in callbacks and a caller-owned checkpoint recorder.
// OUTPUT: No late node/router/checkpoint work, preserved execution errors and truthful retry/transform cancellation.
// POS: [L3 capability acceptance] Verifies callback admission without providers, host resources or new SDK contracts.
package graph

import (
	"context"
	"errors"
	"math"
	"sync/atomic"
	"testing"
	"time"

	"github.com/QuincySx/Alva/sdk/llm/wire"
)

type recordingCheckpoints struct{ saved []int }

func (s *recordingCheckpoints) Save(_ context.Context, _ string, checkpoint Checkpoint[int]) error {
	s.saved = append(s.saved, checkpoint.Step)
	return nil
}
func (*recordingCheckpoints) Load(context.Context, string) (Checkpoint[int], bool, error) {
	return Checkpoint[int]{}, false, nil
}
func (*recordingCheckpoints) List(context.Context) ([]string, error) { return nil, nil }
func (*recordingCheckpoints) Delete(context.Context, string) error   { return nil }

func TestGraphRouterCancellationStopsFollowingRoutingAndCheckpoint(t *testing.T) {
	for _, withFailure := range []bool{false, true} {
		t.Run(map[bool]string{false: "cancel", true: "cancel-and-error"}[withFailure], func(t *testing.T) {
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			b := NewStateGraph(intClone)
			b.SetEntryPoint("node")
			add(t, b, "node", func(context.Context, int) (NodeResult[int], error) { return Update(1), nil })
			failure := errors.New("router infrastructure failed")
			b.AddConditionalEdge("node", func(context.Context, int) (string, error) {
				cancel()
				if withFailure {
					return "", failure
				}
				return End, nil
			})
			late := 0
			b.AddConditionalEdge("node", func(context.Context, int) (string, error) { late++; return End, nil })
			saver := &recordingCheckpoints{}
			_, err := mustCompile(t, b).InvokeWithConfig(ctx, 0, InvokeConfig[int]{Checkpoint: saver})
			if !errors.Is(err, context.Canceled) || withFailure && !errors.Is(err, failure) || late != 0 || len(saver.saved) != 0 {
				t.Fatal("routing cancellation admitted work or lost the real error", err, late, saver.saved)
			}
		})
	}
}

func TestGraphMergeCancellationStopsRoutersAndCheckpoint(t *testing.T) {
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	b := NewStateGraph(intClone)
	b.SetEntryPoint("start")
	for _, name := range []string{"start", "a", "b"} {
		add(t, b, name, func(_ context.Context, state int) (NodeResult[int], error) { return Update(state + 1), nil })
	}
	b.AddEdge("start", "a")
	b.AddEdge("start", "b")
	b.SetMerge(func(int, []int) (int, error) { cancel(); return 99, nil })
	late := 0
	for _, name := range []string{"a", "b"} {
		b.AddConditionalEdge(name, func(context.Context, int) (string, error) { late++; return End, nil })
	}
	saver := &recordingCheckpoints{}
	_, err := mustCompile(t, b).InvokeWithConfig(ctx, 0, InvokeConfig[int]{Checkpoint: saver})
	if !errors.Is(err, context.Canceled) || late != 0 || len(saver.saved) != 1 || saver.saved[0] != 1 {
		t.Fatal("merge cancellation admitted routing/checkpoint work", err, late, saver.saved)
	}
}

func TestGraphEndEventCancellationDoesNotInvokeCheckpoint(t *testing.T) {
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	b := NewStateGraph(intClone)
	b.SetEntryPoint("node")
	add(t, b, "node", func(context.Context, int) (NodeResult[int], error) { return Update(1), nil })
	saver := &recordingCheckpoints{}
	_, err := mustCompile(t, b).InvokeWithConfig(ctx, 0, InvokeConfig[int]{Checkpoint: saver, Emit: func(event GraphEvent) {
		if event.Kind == "superstep_end" {
			cancel()
		}
	}})
	if !errors.Is(err, context.Canceled) || len(saver.saved) != 0 {
		t.Fatal("late cancellation reached the caller's checkpoint adapter", err, saver.saved)
	}
}

func TestGraphCompletionObserverCancellationCannotReportSuccess(t *testing.T) {
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	b := NewStateGraph(intClone)
	b.SetEntryPoint("node")
	add(t, b, "node", func(context.Context, int) (NodeResult[int], error) { return Update(1), nil })
	_, err := mustCompile(t, b).InvokeWithConfig(ctx, 0, InvokeConfig[int]{Emit: func(event GraphEvent) {
		if event.Kind == "completed" {
			cancel()
		}
	}})
	if !errors.Is(err, context.Canceled) {
		t.Fatal("completion observer cancellation became successful execution", err)
	}
}

func TestGraphFinalNodeCloneCannotAdmitExecutionAfterCancellation(t *testing.T) {
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	var armed atomic.Bool
	var clones, executed atomic.Int32
	b := NewStateGraph(func(value int) int {
		if armed.Load() && clones.Add(1) == 2 {
			cancel()
		}
		return value
	})
	b.SetEntryPoint("node")
	add(t, b, "node", func(context.Context, int) (NodeResult[int], error) { executed.Add(1); return Update(1), nil })
	_, err := mustCompile(t, b).InvokeWithConfig(ctx, 0, InvokeConfig[int]{Emit: func(event GraphEvent) {
		if event.Kind == "node_start" {
			armed.Store(true)
		}
	}})
	if !errors.Is(err, context.Canceled) || executed.Load() != 0 {
		t.Fatal("clone cancellation admitted a real node", err, executed.Load())
	}
}

func TestGraphNodeErrorRetainsConcurrentCancellation(t *testing.T) {
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	failure := errors.New("node operation failed")
	b := NewStateGraph(intClone)
	b.SetEntryPoint("node")
	add(t, b, "node", func(context.Context, int) (NodeResult[int], error) { cancel(); return Update(0), failure })
	_, err := mustCompile(t, b).Invoke(ctx, 0)
	if !errors.Is(err, failure) || !errors.Is(err, context.Canceled) {
		t.Fatal("node failure or cancellation was lost", err)
	}
}

func TestGraphRetryCancellationRetainsLastNodeFailure(t *testing.T) {
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	failure := errors.New("node operation failed before retry")
	b := NewStateGraph(intClone)
	b.SetEntryPoint("node")
	calls := 0
	add(t, b, "node", func(context.Context, int) (NodeResult[int], error) { calls++; return Update(0), failure })
	_, err := mustCompile(t, b).InvokeWithConfig(ctx, 0, InvokeConfig[int]{Retry: &RetryConfig{
		MaxRetries: 1,
		Retryable:  func(error) bool { cancel(); return true },
	}})
	if !errors.Is(err, failure) || !errors.Is(err, context.Canceled) || calls != 1 {
		t.Fatal("retry cancellation lost the real node failure or replayed it", err, calls)
	}
}

func TestGraphCanceledEmptyTransformAndZeroDelayWaitCannotSucceed(t *testing.T) {
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	if _, err := (TransformPipeline{}).Apply(ctx, nil); !errors.Is(err, context.Canceled) {
		t.Error("empty pipeline ignored cancellation", err)
	}
	for i := 0; i < 128; i++ {
		if err := (RetryConfig{}).Wait(ctx, 0); !errors.Is(err, context.Canceled) {
			t.Fatal("zero-delay retry ignored cancellation", err)
		}
	}
}

func TestGraphZeroDelayIsBoundedForArbitraryRetryAttempts(t *testing.T) {
	for _, initial := range []int64{0, -1} {
		config := RetryConfig{InitialDelay: time.Duration(initial)}
		if delay := config.Delay(math.MaxInt); delay != 0 {
			t.Fatal("zero delay changed with the attempt count", delay)
		}
	}
}

type suppliedCheckpoint struct {
	recordingCheckpoints
	checkpoint Checkpoint[int]
	loadError  error
}

func (s *suppliedCheckpoint) Load(context.Context, string) (Checkpoint[int], bool, error) {
	return s.checkpoint, true, s.loadError
}

func TestGraphResumeCannotOverflowItsExecutionBudget(t *testing.T) {
	for _, step := range []int{-1, math.MaxInt} {
		b := NewStateGraph(intClone)
		b.SetEntryPoint("node")
		calls := 0
		add(t, b, "node", func(context.Context, int) (NodeResult[int], error) { calls++; return Update(1), nil })
		saver := &suppliedCheckpoint{checkpoint: Checkpoint[int]{Step: step, Pending: []string{"node"}}}
		if _, err := mustCompile(t, b).Resume(t.Context(), "external", InvokeConfig[int]{Checkpoint: saver}); err == nil || calls != 0 {
			t.Fatal("invalid checkpoint admitted node execution", step, calls, err)
		}
	}
}

type graphCounterFunc func(string) int

func (f graphCounterFunc) CountTokens(text string) int { return f(text) }
func (graphCounterFunc) ContextWindow() int            { return 0 }

func TestGraphCompactionCannotOverflowOrCountWhenDisabled(t *testing.T) {
	message := wire.TextMessage(wire.RoleUser, "real graph input")
	for _, value := range []int{-1, math.MaxInt} {
		config := CompactionConfig{MaxTokens: 12, Counter: graphCounterFunc(func(string) int { return value })}
		if !config.ShouldCompact([]wire.Message{message}) {
			t.Fatal("invalid/overflowing estimate bypassed compaction", value)
		}
	}
	called := false
	config := CompactionConfig{Counter: graphCounterFunc(func(string) int { called = true; return 1 })}
	if config.ShouldCompact([]wire.Message{message}) || called {
		t.Fatal("disabled compaction called the user's estimator")
	}
	config.MaxTokens = 12
	config.Counter = graphCounterFunc(func(string) int { return 2 })
	if config.ShouldCompact([]wire.Message{message, message}) || !config.ShouldCompact([]wire.Message{message, message, message}) {
		t.Fatal("finite estimate no longer respects the exact budget")
	}
}

func TestGraphExplicitRetryReceivesOriginalNodeFailure(t *testing.T) {
	failure := errors.New("retry only this exact failure")
	b := NewStateGraph(intClone)
	b.SetEntryPoint("node")
	calls := 0
	add(t, b, "node", func(context.Context, int) (NodeResult[int], error) {
		calls++
		if calls == 1 {
			return Update(0), failure
		}
		return Update(7), nil
	})
	got, err := mustCompile(t, b).InvokeWithConfig(t.Context(), 0, InvokeConfig[int]{Retry: &RetryConfig{
		MaxRetries: 1, Retryable: func(err error) bool { return err == failure },
	}})
	if err != nil || got != 7 || calls != 2 {
		t.Fatal("retry did not receive the original node error", got, err, calls)
	}
}

func TestGraphRetryRejectionRetainsCallbackCancellation(t *testing.T) {
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	failure := errors.New("node failure before retry rejection")
	b := NewStateGraph(intClone)
	b.SetEntryPoint("node")
	calls := 0
	add(t, b, "node", func(context.Context, int) (NodeResult[int], error) { calls++; return Update(0), failure })
	_, err := mustCompile(t, b).InvokeWithConfig(ctx, 0, InvokeConfig[int]{Retry: &RetryConfig{
		MaxRetries: 1, Retryable: func(error) bool { cancel(); return false },
	}})
	if !errors.Is(err, failure) || !errors.Is(err, context.Canceled) || calls != 1 {
		t.Fatal("retry rejection lost cancellation or the node error", err, calls)
	}
}

func TestGraphCallbackErrorsKeepTheirOriginalIdentity(t *testing.T) {
	for _, stage := range []string{"router", "merge", "load"} {
		t.Run(stage, func(t *testing.T) {
			failure := errors.New("original caller callback failure")
			b := NewStateGraph(intClone)
			b.SetEntryPoint("start")
			for _, name := range []string{"start", "a", "b"} {
				add(t, b, name, func(context.Context, int) (NodeResult[int], error) { return Update(1), nil })
			}
			switch stage {
			case "router":
				b.AddConditionalEdge("start", func(context.Context, int) (string, error) { return "", failure })
			case "merge":
				b.AddEdge("start", "a")
				b.AddEdge("start", "b")
				b.SetMerge(func(int, []int) (int, error) { return 0, failure })
			}
			graph := mustCompile(t, b)
			var err error
			if stage == "load" {
				_, err = graph.Resume(t.Context(), "external", InvokeConfig[int]{Checkpoint: &suppliedCheckpoint{loadError: failure}})
			} else {
				_, err = graph.Invoke(t.Context(), 0)
			}
			if err != failure {
				t.Fatal("uncanceled callback failure lost its original identity", err)
			}
		})
	}
}
