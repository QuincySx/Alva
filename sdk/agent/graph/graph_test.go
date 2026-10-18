// INPUT: context, errors, reflect, sync/atomic, testing
// OUTPUT: Graph BSP, dynamic Send, checkpoint, retry and channel regressions
// POS: [L3 capability library] Executes real graphs without providers, host services or implicit side-effect replay.
package graph

import (
	"context"
	"errors"
	"reflect"
	"sync/atomic"
	"testing"
)

func intClone(value int) int { return value }
func mustCompile(t *testing.T, builder *StateGraph[int]) *CompiledGraph[int] {
	t.Helper()
	graph, err := builder.Compile()
	if err != nil {
		t.Fatal(err)
	}
	return graph
}
func add(t *testing.T, builder *StateGraph[int], name string, node Node[int]) {
	t.Helper()
	if err := builder.AddNode(name, node); err != nil {
		t.Fatal(err)
	}
}
func TestStaticParallelRoutesAfterDeterministicMerge(t *testing.T) {
	b := NewStateGraph(intClone)
	b.SetEntryPoint("start")
	add(t, b, "start", func(_ context.Context, s int) (NodeResult[int], error) { return Update(s), nil })
	add(t, b, "a", func(context.Context, int) (NodeResult[int], error) { return Update(1), nil })
	add(t, b, "b", func(context.Context, int) (NodeResult[int], error) { return Update(2), nil })
	add(t, b, "done", func(_ context.Context, s int) (NodeResult[int], error) { return Update(s + 10), nil })
	b.SetMerge(func(_ int, updates []int) (int, error) {
		if !reflect.DeepEqual(updates, []int{1, 2}) {
			t.Fatal(updates)
		}
		return 3, nil
	})
	b.AddEdge("start", "b")
	b.AddEdge("start", "a")
	for _, name := range []string{"a", "b"} {
		b.AddConditionalEdge(name, func(_ context.Context, s int) (string, error) {
			if s != 3 {
				return "missing", nil
			}
			return "done", nil
		})
	}
	value, err := mustCompile(t, b).Invoke(context.Background(), 0)
	if err != nil || value != 13 {
		t.Fatal(value, err)
	}
}
func TestNestedDynamicSendsAreNotDropped(t *testing.T) {
	b := NewStateGraph(intClone)
	b.SetEntryPoint("start")
	add(t, b, "start", func(context.Context, int) (NodeResult[int], error) {
		return Sends(SendTo[int]{"fan", 1}, SendTo[int]{"fan", 2}), nil
	})
	add(t, b, "fan", func(_ context.Context, s int) (NodeResult[int], error) {
		return Sends(SendTo[int]{"leaf", s * 10}), nil
	})
	add(t, b, "leaf", func(_ context.Context, s int) (NodeResult[int], error) { return Update(s), nil })
	b.SetMerge(func(_ int, updates []int) (int, error) {
		sum := 0
		for _, v := range updates {
			sum += v
		}
		return sum, nil
	})
	value, err := mustCompile(t, b).Invoke(context.Background(), 0)
	if err != nil || value != 30 {
		t.Fatal(value, err)
	}
}
func TestCheckpointResumeFrontierAndCopy(t *testing.T) {
	b := NewStateGraph(intClone)
	b.SetEntryPoint("one")
	var one, two atomic.Int32
	add(t, b, "one", func(_ context.Context, s int) (NodeResult[int], error) { one.Add(1); return Update(s + 1), nil })
	add(t, b, "two", func(_ context.Context, s int) (NodeResult[int], error) { two.Add(1); return Update(s + 2), nil })
	b.AddEdge("one", "two")
	graph := mustCompile(t, b)
	saver, _ := NewMemoryCheckpointSaver(intClone, 4)
	if value, err := graph.InvokeWithConfig(context.Background(), 0, InvokeConfig[int]{Checkpoint: saver, CheckpointID: "run", MaxSteps: 1}); err == nil || value != 1 {
		t.Fatal(value, err)
	}
	checkpoint, found, err := saver.Load(context.Background(), "run-step-1")
	if err != nil || !found {
		t.Fatal(err)
	}
	checkpoint.Pending[0] = "bad"
	value, err := graph.Resume(context.Background(), "run-step-1", InvokeConfig[int]{Checkpoint: saver})
	if err != nil || value != 3 || one.Load() != 1 || two.Load() != 1 {
		t.Fatal(value, err, one.Load(), two.Load())
	}
}
func TestGraphRetryIsExplicitAndCancelIsNotReplayed(t *testing.T) {
	b := NewStateGraph(intClone)
	b.SetEntryPoint("node")
	calls := 0
	transient := errors.New("transient")
	add(t, b, "node", func(_ context.Context, s int) (NodeResult[int], error) {
		calls++
		if calls < 3 {
			return Update(s), transient
		}
		return Update(s + 1), nil
	})
	graph := mustCompile(t, b)
	if _, err := graph.Invoke(context.Background(), 0); !errors.Is(err, transient) || calls != 1 {
		t.Fatal(err, calls)
	}
	calls = 0
	if value, err := graph.InvokeWithConfig(context.Background(), 0, InvokeConfig[int]{Retry: &RetryConfig{MaxRetries: 2, Retryable: func(err error) bool { return errors.Is(err, transient) }}}); err != nil || value != 1 || calls != 3 {
		t.Fatal(value, err, calls)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := graph.Invoke(ctx, 0); !errors.Is(err, context.Canceled) || calls != 3 {
		t.Fatal(err, calls)
	}
}
func TestChannelOwnershipResetAndAggregate(t *testing.T) {
	clone := func(value []int) []int {
		if value == nil {
			panic("must not clone absent value")
		}
		return append([]int(nil), value...)
	}
	channel := NewEphemeralValue(clone, nil)
	if _, found := channel.Get(); found {
		t.Fatal("present before write")
	}
	source := []int{1}
	channel.Update([][]int{source})
	source[0] = 9
	got, _ := channel.Get()
	got[0] = 8
	if value, _ := channel.Get(); value[0] != 1 {
		t.Fatal(value)
	}
	version := channel.Version()
	channel.Reset()
	if _, found := channel.Get(); found || channel.Version() != version {
		t.Fatal("ephemeral reset changed update version")
	}
	aggregate, err := NewBinaryOperatorAggregate(intClone, nil, func(a, b int) int { return a + b })
	if err != nil {
		t.Fatal(err)
	}
	aggregate.Update([]int{1, 2})
	aggregate.Update([]int{3})
	if value, _ := aggregate.Get(); value != 6 {
		t.Fatal(value)
	}
	if _, err := NewBinaryOperatorAggregate[int](nil, nil, nil); err == nil {
		t.Fatal("nil reducer accepted")
	}
}
