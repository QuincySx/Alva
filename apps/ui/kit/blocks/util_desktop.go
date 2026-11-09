// INPUT: Nothing but strconv.
// OUTPUT: itoa, the one number formatter blocks need.
// POS: [L7 Kit blocks] Kept apart so view code reads without conversions.
//go:build desktop

package blocks

import "strconv"

func itoa(n int) string { return strconv.Itoa(n) }
