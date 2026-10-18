// INPUT: Pure SDK graph transform and checkpoint clone callbacks with cancellation/reentrant observation.
// OUTPUT: No successful late-cancel transform or checkpoint commit, and no callback under checkpoint locks.
// POS: [L3 capability library] Replaceable callbacks keep ownership and cancellation semantics outside internal locks.
package graph

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/QuincySx/Alva/sdk/llm/wire"
)

func TestTransformPipelineJoinsLastCallbackCancellation(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	p := TransformPipeline{Transforms: []ContextTransform{TransformFunc(func(context.Context, []wire.Message) ([]wire.Message, error) {
		cancel()
		return []wire.Message{wire.TextMessage(wire.RoleUser, "late")}, nil
	})}}
	if _, err := p.Apply(ctx, nil); !errors.Is(err, context.Canceled) {
		t.Fatal("last transform returned successful cancelled output", err)
	}
}
func TestCheckpointCloneRunsOutsideStoreLock(t *testing.T) {
	var saver *MemoryCheckpointSaver[int]
	blocked := false
	clone := func(value int) int {
		done := make(chan struct{})
		go func() { _, _ = saver.List(context.Background()); close(done) }()
		select {
		case <-done:
		case <-time.After(100 * time.Millisecond):
			blocked = true
		}
		return value
	}
	var err error
	saver, err = NewMemoryCheckpointSaver(clone, 4)
	if err != nil {
		t.Fatal(err)
	}
	if err := saver.Save(t.Context(), "checkpoint", Checkpoint[int]{State: 1}); err != nil {
		t.Fatal(err)
	}
	if blocked {
		t.Fatal("external clone callback executed while checkpoint lock blocked a reader")
	}
}
func TestCheckpointCloneCancellationDoesNotCommit(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	saver, _ := NewMemoryCheckpointSaver(func(v int) int { cancel(); return v }, 4)
	if err := saver.Save(ctx, "cancelled", Checkpoint[int]{State: 1}); !errors.Is(err, context.Canceled) {
		t.Error("cancelled clone committed checkpoint", err)
	}
	keys, err := saver.List(t.Context())
	if err != nil || len(keys) != 0 {
		t.Fatal("cancelled checkpoint retained", keys, err)
	}
}
func TestChannelCallbacksCanObserveVersion(t *testing.T) {
	for _, mode := range []string{"equal", "reduce"} {
		t.Run(mode, func(t *testing.T) {
			var channel *LastValue[int]
			blocked := false
			observe := func() {
				done := make(chan struct{})
				go func() { channel.Version(); close(done) }()
				select {
				case <-done:
				case <-time.After(100 * time.Millisecond):
					blocked = true
				}
			}
			if mode == "equal" {
				channel = NewLastValue(intClone, func(a, b int) bool { observe(); return a == b })
				channel.Update([]int{1})
				channel.Update([]int{2})
			} else {
				aggregate, err := NewBinaryOperatorAggregate(intClone, nil, func(a, b int) int { observe(); return a + b })
				if err != nil {
					t.Fatal(err)
				}
				channel = aggregate.LastValue
				aggregate.Update([]int{1, 2})
			}
			if blocked {
				t.Fatal("channel callback held the state lock while observing Version")
			}
		})
	}
}

func TestTransformPipelineDetachesCallbackOutput(t *testing.T) {
	returned := []wire.Message{wire.TextMessage(wire.RoleUser, "original")}
	output, err := (TransformPipeline{Transforms: []ContextTransform{TransformFunc(func(context.Context, []wire.Message) ([]wire.Message, error) { return returned, nil })}}).Apply(t.Context(), nil)
	if err != nil {
		t.Fatal(err)
	}
	returned[0].Content[0].Text = "changed after callback"
	if output[0].Text() != "original" {
		t.Fatal("retained callback slice mutated the pipeline result")
	}
}
