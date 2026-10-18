// INPUT: context, errors, fmt, sync
// OUTPUT: Topic, Subscription, LaggedError, ErrSubscriptionClosed, NewTopic
// POS: [L0 foundation] Typed nonblocking bounded broadcast with explicit lag rather than hidden lossless queues.
package bus

import (
	"context"
	"errors"
	"fmt"
	"sync"
)

var ErrSubscriptionClosed = errors.New("bus subscription closed")

type LaggedError struct{ Skipped uint64 }

func (e LaggedError) Error() string {
	return fmt.Sprintf("bus subscriber lagged by %d events", e.Skipped)
}

// Clone must be pure and own reference-bearing values. nil is appropriate only
// for immutable value types. Publishers never wait for consumer progress.
type Topic[T any] struct {
	mu          sync.Mutex
	cloneMu     sync.Mutex
	capacity    int
	clone       func(T) T
	events      []T
	first, next uint64
	changed     chan struct{}
	subscribers int
}

func NewTopic[T any](capacity int, clone func(T) T) (*Topic[T], error) {
	if capacity < 1 {
		return nil, errors.New("topic capacity must be positive")
	}
	if clone == nil {
		clone = func(value T) T { return value }
	}
	return &Topic[T]{capacity: capacity, clone: clone, events: make([]T, capacity), changed: make(chan struct{})}, nil
}
func (t *Topic[T]) Publish(value T) {
	t.mu.Lock()
	if t.subscribers == 0 {
		t.mu.Unlock()
		return
	}
	t.mu.Unlock()
	value = t.copy(value)
	t.mu.Lock()
	defer t.mu.Unlock()
	if t.subscribers == 0 {
		return
	}
	t.events[t.next%uint64(t.capacity)] = value
	if t.next-t.first >= uint64(t.capacity) {
		t.first++
	}
	t.next++
	close(t.changed)
	t.changed = make(chan struct{})
}

func (t *Topic[T]) copy(value T) T {
	t.cloneMu.Lock()
	defer t.cloneMu.Unlock()
	return t.clone(value)
}
func (t *Topic[T]) HasSubscribers() bool { t.mu.Lock(); defer t.mu.Unlock(); return t.subscribers > 0 }
func (t *Topic[T]) Subscribe() *Subscription[T] {
	t.mu.Lock()
	defer t.mu.Unlock()
	t.subscribers++
	return &Subscription[T]{topic: t, cursor: t.next, done: make(chan struct{})}
}

type Subscription[T any] struct {
	mu     sync.Mutex
	topic  *Topic[T]
	cursor uint64
	closed bool
	done   chan struct{}
}

func (s *Subscription[T]) Close() {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed {
		return
	}
	s.closed = true
	close(s.done)
	s.topic.mu.Lock()
	s.topic.subscribers--
	if s.topic.subscribers == 0 {
		var zero T
		for i := range s.topic.events {
			s.topic.events[i] = zero
		}
		s.topic.first = s.topic.next
	}
	s.topic.mu.Unlock()
}
func (s *Subscription[T]) Recv(ctx context.Context) (T, error) {
	var zero T
	for {
		if err := ctx.Err(); err != nil {
			return zero, err
		}
		s.mu.Lock()
		if s.closed {
			s.mu.Unlock()
			return zero, ErrSubscriptionClosed
		}
		t := s.topic
		t.mu.Lock()
		if s.cursor < t.first {
			skipped := t.first - s.cursor
			s.cursor = t.first
			t.mu.Unlock()
			s.mu.Unlock()
			return zero, LaggedError{Skipped: skipped}
		}
		if s.cursor < t.next {
			value := t.events[s.cursor%uint64(t.capacity)]
			s.cursor++
			t.mu.Unlock()
			s.mu.Unlock()
			return t.copy(value), nil
		}
		changed := t.changed
		t.mu.Unlock()
		done := s.done
		s.mu.Unlock()
		select {
		case <-ctx.Done():
			return zero, ctx.Err()
		case <-done:
			return zero, ErrSubscriptionClosed
		case <-changed:
		}
	}
}
