// INPUT: Package-private stream coalescing knobs
// OUTPUT: SetStreamCoalescingForTest hook for external hub_test regressions
// POS: [L7 Hub testing] Test-only timing control; production code never changes the coalescing window
package hub

import "time"

// SetStreamCoalescingForTest shortens the window for timing regressions. Call
// it before starting a Runtime; the returned function restores the defaults.
func SetStreamCoalescingForTest(window, tick time.Duration, limit int) func() {
	previousWindow, previousTick, previousLimit := streamCoalesceWindow, streamCoalesceTick, streamCoalesceBytes
	streamCoalesceWindow, streamCoalesceTick, streamCoalesceBytes = window, tick, limit
	return func() {
		streamCoalesceWindow, streamCoalesceTick, streamCoalesceBytes = previousWindow, previousTick, previousLimit
	}
}
