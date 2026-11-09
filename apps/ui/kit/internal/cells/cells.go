// INPUT: Gallery pages and their tests asking for state that outlives a frame (typed text, an open menu, a toggle).
// OUTPUT: Slot, Text and Reset over one process-wide map keyed by page, axis values and slot name.
// POS: [L7 Kit] Gallery-only state. It lives in internal so production code can never reach the global map; only kit, kit/blocks and kit/gallery can import it.
package cells

import (
	"strings"
	"sync"
)

var slots sync.Map // string -> any (*T)

func key(page string, sel []string, slot string) string {
	return page + "|" + strings.Join(sel, "|") + "|" + slot
}

// Slot returns the persistent value of one named slot of one gallery cell, created from init on first use.
func Slot[T any](page string, sel []string, slot string, init T) *T {
	k := key(page, sel, slot)
	if v, ok := slots.Load(k); ok {
		return v.(*T)
	}
	v, _ := slots.LoadOrStore(k, &init)
	return v.(*T)
}

// Text reads a string slot, for tests that must see what the user typed.
func Text(page string, sel []string, slot string) (string, bool) {
	if v, ok := slots.Load(key(page, sel, slot)); ok {
		if s, ok := v.(*string); ok {
			return *s, true
		}
	}
	return "", false
}

// Reset forgets every slot.
func Reset() { slots.Clear() }
