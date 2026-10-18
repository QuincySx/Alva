// INPUT: context, testing, time
// OUTPUT: Topic callback lock boundary regression
// POS: [L0 foundation] Verifies copy callbacks can observe topic metadata without a state-lock cycle.
package bus

import (
	"context"
	"testing"
	"time"
)

func TestTopicCloneCanObserveTopic(t *testing.T) {
	var topic *Topic[int]
	topic, _ = NewTopic(2, func(value int) int {
		done := make(chan struct{})
		go func() { topic.HasSubscribers(); close(done) }()
		select {
		case <-done:
		case <-time.After(time.Second):
			t.Error("clone callback holds topic lock")
		}
		return value
	})
	sub := topic.Subscribe()
	defer sub.Close()
	topic.Publish(7)
	if got, err := sub.Recv(context.Background()); err != nil || got != 7 {
		t.Fatal(got, err)
	}
}
