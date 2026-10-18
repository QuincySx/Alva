// INPUT: Controlled observer clock, paused actual dispatch queue and cancellation barriers
// OUTPUT: At-most-one UI closure, no redraw dependency and no late queries after observer close
// POS: [L7 Hub product] Observation tests neither execute an engine nor turn UI cancellation into Stop.
package hubdesktop

import (
	"context"
	"testing"
	"time"
)

func TestPageObservationDoesNotDependOnBuildAndCoalesces(t *testing.T) {
	ctx, cancel := context.WithCancel(t.Context())
	defer cancel()
	ticks := make(chan time.Time, 8)
	queue := make(chan func(), 2)
	done := make(chan struct{})
	calls := 0
	go func() { observePage(ctx, func(fn func()) { queue <- fn }, func() { calls++ }, ticks); close(done) }()
	ticks <- time.Now()
	var first func()
	select {
	case first = <-queue:
	case <-time.After(time.Second):
		t.Fatal("observer waited for a frame")
	}
	for range 8 {
		ticks <- time.Now()
	}
	select {
	case <-queue:
		t.Fatal("slow UI queued another observation")
	case <-time.After(20 * time.Millisecond):
	}
	first()
	if calls != 1 {
		t.Fatal("actual UI closure did not refresh")
	}
	var second func()
	select {
	case second = <-queue:
	case <-time.After(time.Second):
		t.Fatal("coalesced clock failed to advance")
	}
	cancel()
	select {
	case <-done:
	case <-time.After(time.Second):
		t.Fatal("observer failed to join after cancel")
	}
	second()
	if calls != 1 {
		t.Fatal("closed window ran a late product query")
	}
}
