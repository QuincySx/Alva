// INPUT: context, errors, sync, testing, kernel/abi
// OUTPUT: Bounded event delivery and producer cancellation regressions
// POS: [L2 kernel runtime] Verifies lossless backpressure without detached producer goroutines.
package core

import (
	"context"
	"encoding/json"
	"errors"
	"sync"
	"testing"

	"github.com/QuincySx/Alva/go/sdk/kernel/abi"
	"github.com/QuincySx/Alva/go/sdk/llm/wire"
)

func TestBoundedSinkPreservesEveryAcceptedEvent(t *testing.T) {
	sink, err := NewBoundedEventSink(2)
	if err != nil {
		t.Fatal(err)
	}
	var wait sync.WaitGroup
	wait.Add(1)
	go func() {
		defer wait.Done()
		defer sink.Close()
		for i := 0; i < 1000; i++ {
			if err := sink.Emit(context.Background(), abi.Event{Kind: abi.EventTextDelta, Text: "x"}); err != nil {
				t.Error(err)
				return
			}
		}
	}()
	count := 0
	for range sink.Events() {
		count++
	}
	wait.Wait()
	if count != 1000 {
		t.Fatalf("lost events: %d", count)
	}
}

func TestFullSinkCancellationUnblocksProducer(t *testing.T) {
	sink, _ := NewBoundedEventSink(1)
	defer sink.Close()
	if err := sink.Emit(context.Background(), abi.Event{}); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	started := make(chan struct{})
	result := make(chan error, 1)
	go func() { close(started); result <- sink.Emit(ctx, abi.Event{}) }()
	<-started
	cancel()
	if err := <-result; !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
	if len(sink.events) != 1 {
		t.Fatal("cancelled event accepted")
	}
}

func TestCloseUnblocksAllPendingProducersAndDrainsAccepted(t *testing.T) {
	sink, _ := NewBoundedEventSink(1)
	if err := sink.Emit(context.Background(), abi.Event{Text: "accepted"}); err != nil {
		t.Fatal(err)
	}
	var wait sync.WaitGroup
	wait.Add(32)
	for i := 0; i < 32; i++ {
		go func() {
			defer wait.Done()
			if err := sink.Emit(context.Background(), abi.Event{}); !errors.Is(err, ErrSinkClosed) {
				t.Errorf("err=%v", err)
			}
		}()
	}
	sink.Close()
	wait.Wait()
	sink.Close()
	count := 0
	for event := range sink.Events() {
		count++
		if event.Text != "accepted" {
			t.Fatal("unexpected accepted event")
		}
	}
	if count != 1 {
		t.Fatalf("count=%d", count)
	}
}

func TestInvalidSinkCapacity(t *testing.T) {
	for _, capacity := range []int{0, -1} {
		if _, err := NewBoundedEventSink(capacity); err == nil {
			t.Fatal("invalid capacity accepted")
		}
	}
}

func TestQueuedEventOwnsNestedPayload(t *testing.T) {
	sink, _ := NewBoundedEventSink(1)
	defer sink.Close()
	event := abi.Event{ToolCall: &wire.ToolCall{ID: "original", Name: "tool", Arguments: json.RawMessage(`{}`)}}
	if err := sink.Emit(context.Background(), event); err != nil {
		t.Fatal(err)
	}
	event.ToolCall.ID = "changed"
	event.ToolCall.Arguments[0] = '['
	received := <-sink.Events()
	if received.ToolCall.ID != "original" || string(received.ToolCall.Arguments) != "{}" {
		t.Fatal("producer retained queue payload ownership")
	}
}
