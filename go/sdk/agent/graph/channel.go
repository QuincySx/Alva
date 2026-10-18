// INPUT: reflect, sync
// OUTPUT: Channel, LastValue, EphemeralValue, BinaryOperatorAggregate
// POS: [L3 capability library] Versioned persistent/ephemeral/reducer graph state channels with explicit copy semantics.
package graph

import (
	"errors"
	"reflect"
	"sync"
)

type Channel[T any] interface {
	Get() (T, bool)
	Update([]T) bool
	Reset()
	Version() uint64
}
type LastValue[T any] struct {
	mu       sync.RWMutex
	updateMu sync.Mutex
	value    T
	present  bool
	version  uint64
	clone    func(T) T
	equal    func(T, T) bool
}

func NewLastValue[T any](clone func(T) T, equal func(T, T) bool) *LastValue[T] {
	if clone == nil {
		clone = func(value T) T { return value }
	}
	if equal == nil {
		equal = func(left, right T) bool { return reflect.DeepEqual(left, right) }
	}
	return &LastValue[T]{clone: clone, equal: equal}
}
func (channel *LastValue[T]) Get() (T, bool) {
	channel.mu.RLock()
	value, present := channel.value, channel.present
	channel.mu.RUnlock()
	if !present {
		var zero T
		return zero, false
	}
	return channel.clone(value), true
}
func (channel *LastValue[T]) Update(values []T) bool {
	if len(values) == 0 {
		return false
	}
	channel.updateMu.Lock()
	defer channel.updateMu.Unlock()
	next := channel.clone(values[len(values)-1])
	channel.mu.RLock()
	value, present := channel.value, channel.present
	channel.mu.RUnlock()
	changed := !present || !channel.equal(value, next)
	channel.mu.Lock()
	defer channel.mu.Unlock()
	channel.value, channel.present = next, true
	if changed {
		channel.version++
	}
	return changed
}
func (channel *LastValue[T]) Reset() {}
func (channel *LastValue[T]) Version() uint64 {
	channel.mu.RLock()
	defer channel.mu.RUnlock()
	return channel.version
}

type EphemeralValue[T any] struct{ *LastValue[T] }

func NewEphemeralValue[T any](clone func(T) T, equal func(T, T) bool) *EphemeralValue[T] {
	return &EphemeralValue[T]{NewLastValue(clone, equal)}
}
func (channel *EphemeralValue[T]) Reset() {
	channel.updateMu.Lock()
	defer channel.updateMu.Unlock()
	channel.mu.Lock()
	defer channel.mu.Unlock()
	var zero T
	channel.value, channel.present = zero, false
}

type BinaryOperatorAggregate[T any] struct {
	*LastValue[T]
	reduce func(T, T) T
}

func NewBinaryOperatorAggregate[T any](clone func(T) T, equal func(T, T) bool, reduce func(T, T) T) (*BinaryOperatorAggregate[T], error) {
	if reduce == nil {
		return nil, errors.New("aggregate reducer required")
	}
	return &BinaryOperatorAggregate[T]{LastValue: NewLastValue(clone, equal), reduce: reduce}, nil
}
func (channel *BinaryOperatorAggregate[T]) Update(values []T) bool {
	if len(values) == 0 {
		return false
	}
	channel.updateMu.Lock()
	defer channel.updateMu.Unlock()
	channel.mu.RLock()
	value, present := channel.value, channel.present
	channel.mu.RUnlock()
	var next T
	start := 0
	if present {
		next = channel.clone(value)
	} else {
		next = channel.clone(values[0])
		start = 1
	}
	for _, value := range values[start:] {
		next = channel.reduce(next, channel.clone(value))
	}
	changed := !present || !channel.equal(value, next)
	next = channel.clone(next)
	channel.mu.Lock()
	defer channel.mu.Unlock()
	channel.value, channel.present = next, true
	if changed {
		channel.version++
	}
	return changed
}
