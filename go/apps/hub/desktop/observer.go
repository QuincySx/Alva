// INPUT: Window observer context, sole UI dispatcher, read-only refresh callback and explicit clock
// OUTPUT: Coalesced observePage loop with no late refresh after observer cancellation
// POS: [L7 Hub product] Product observation progresses independently of redraw or compositor availability.
package hubdesktop

import (
	"context"
	"time"
)

func observePage(ctx context.Context, enqueue func(func()), refresh func(), ticks <-chan time.Time) {
	for {
		select {
		case <-ctx.Done():
			return
		case _, ok := <-ticks:
			if !ok {
				return
			}
		}
		if ctx.Err() != nil {
			return
		}
		applied := make(chan struct{})
		enqueue(func() {
			defer close(applied)
			if ctx.Err() == nil {
				refresh()
			}
		})
		select {
		case <-ctx.Done():
			return
		case <-applied:
		}
	}
}
