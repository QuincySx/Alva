// INPUT: context, errors, sync, kernel/abi
// OUTPUT: BoundedEventSink, NewBoundedEventSink
// POS: [L2 kernel runtime] Lossless bounded event delivery with cancellable producers and explicit owner shutdown.
package core

import (
	"context"
	"errors"
	"sync"

	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

var ErrSinkClosed = errors.New("event sink closed")

// Successful Emit calls are lossless. Close unblocks pending Emit calls and
// leaves already accepted events available to the consumer before channel EOF.
type BoundedEventSink struct {
	mu        sync.Mutex
	closed    bool
	inflight  sync.WaitGroup
	closeOnce sync.Once
	events    chan abi.Event
	done      chan struct{}
}

func NewBoundedEventSink(capacity int) (*BoundedEventSink, error) {
	if capacity < 1 {
		return nil, errors.New("event capacity must be positive")
	}
	return &BoundedEventSink{events: make(chan abi.Event, capacity), done: make(chan struct{})}, nil
}

func (s *BoundedEventSink) Events() <-chan abi.Event { return s.events }

func (s *BoundedEventSink) Emit(ctx context.Context, event abi.Event) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	s.mu.Lock()
	if s.closed {
		s.mu.Unlock()
		return ErrSinkClosed
	}
	s.inflight.Add(1)
	s.mu.Unlock()
	defer s.inflight.Done()
	owned := abi.CloneEvent(event)
	select {
	case <-ctx.Done():
		return ctx.Err()
	case <-s.done:
		return ErrSinkClosed
	case s.events <- owned:
		return nil
	}
}

func (s *BoundedEventSink) Close() {
	s.closeOnce.Do(func() {
		s.mu.Lock()
		s.closed = true
		close(s.done)
		s.mu.Unlock()
		s.inflight.Wait()
		close(s.events)
	})
}
