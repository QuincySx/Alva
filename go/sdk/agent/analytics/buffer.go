// INPUT: errors, sync/atomic, agent/abi
// OUTPUT: Buffer, NewBuffer
// POS: [L3 capability library] Bounded nonblocking best-effort telemetry queue with explicit loss accounting and no hidden IO.
package analytics

import (
	"errors"
	"sync/atomic"

	"github.com/QuincySx/Alva/go/sdk/agent/abi"
)

type Buffer struct {
	events  chan abi.AnalyticsEvent
	dropped atomic.Uint64
}

func NewBuffer(capacity int) (*Buffer, error) {
	if capacity < 1 || capacity > 4096 {
		return nil, errors.New("analytics capacity must be 1..4096")
	}
	return &Buffer{events: make(chan abi.AnalyticsEvent, capacity)}, nil
}
func (buffer *Buffer) Record(event abi.AnalyticsEvent) {
	if event.Validate() != nil {
		buffer.dropped.Add(1)
		return
	}
	owned := abi.CloneAnalyticsEvent(event)
	select {
	case buffer.events <- owned:
	default:
		buffer.dropped.Add(1)
	}
}
func (buffer *Buffer) Dropped() uint64 { return buffer.dropped.Load() }
func (buffer *Buffer) Drain(limit int) ([]abi.AnalyticsEvent, error) {
	if limit < 1 || limit > 4096 {
		return nil, errors.New("analytics drain limit must be 1..4096")
	}
	var result []abi.AnalyticsEvent
	for len(result) < limit {
		select {
		case event := <-buffer.events:
			result = append(result, event)
		default:
			return result, nil
		}
	}
	return result, nil
}
