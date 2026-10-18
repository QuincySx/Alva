// INPUT: context, errors, sync, testing
// OUTPUT: Broadcast, lag, ownership regressions
// POS: [L0 foundation] Ensures bus notifications remain bounded/nonblocking and distinct from execution event sinks.
package bus

import (
	"context"
	"errors"
	"sync"
	"testing"
)

func TestBroadcastLagAndResubscribe(t *testing.T) {
	if topic, err := NewTopic[int](0, nil); err == nil || topic != nil {
		t.Fatal("nonpositive capacity accepted")
	}
	topic, err := NewTopic[int](2, nil)
	if err != nil {
		t.Fatal(err)
	}
	a, b := topic.Subscribe(), topic.Subscribe()
	defer a.Close()
	defer b.Close()
	for _, n := range []int{1, 2, 3} {
		topic.Publish(n)
	}
	if _, err := a.Recv(context.Background()); err == nil {
		t.Fatal("lag not reported")
	} else {
		var lag LaggedError
		if !errors.As(err, &lag) || lag.Skipped != 1 {
			t.Fatal(err)
		}
	}
	for _, want := range []int{2, 3} {
		if got, err := a.Recv(context.Background()); err != nil || got != want {
			t.Fatalf("got=%d err=%v", got, err)
		}
	}
	a.Close()
	b.Close()
	if topic.HasSubscribers() {
		t.Fatal("subscriber retained")
	}
	next := topic.Subscribe()
	defer next.Close()
	topic.Publish(4)
	if got, err := next.Recv(context.Background()); err != nil || got != 4 {
		t.Fatalf("resubscribe got=%d err=%v", got, err)
	}
}
func TestTopicCopiesReferenceValuesAndCancelsBlockedRecv(t *testing.T) {
	clone := func(value []int) []int { return append([]int(nil), value...) }
	topic, _ := NewTopic(2, clone)
	sub := topic.Subscribe()
	defer sub.Close()
	value := []int{1}
	topic.Publish(value)
	value[0] = 2
	received, err := sub.Recv(context.Background())
	if err != nil || received[0] != 1 {
		t.Fatal(received, err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := sub.Recv(ctx); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
}
func TestTopicConcurrentCloseAndPublish(t *testing.T) {
	topic, _ := NewTopic[int](4, nil)
	sub := topic.Subscribe()
	var wait sync.WaitGroup
	wait.Add(2)
	go func() {
		defer wait.Done()
		for i := 0; i < 1000; i++ {
			topic.Publish(i)
		}
	}()
	go func() { defer wait.Done(); sub.Close() }()
	wait.Wait()
}
