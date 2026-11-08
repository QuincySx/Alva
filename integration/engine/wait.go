// INPUT: A native runner's committed durable checkpoint metadata.
// OUTPUT: WaitCheckpoint (with the yielded tool calls) and optional WaitingRunner terminal receipt contract.
// POS: [L4.5 engine contract] Execution may end waiting without claiming task success or exposing native wait payloads.
package engine

import (
	"errors"
	"strings"
	"unicode/utf8"
)

type WaitCheckpoint struct {
	Revision  uint64 `json:"revision"`
	Requests  int    `json:"requests"`
	ExpiresAt int64  `json:"expires_at"`
	// Calls names the tool calls that yielded into this checkpoint, at most
	// one per request. Their committed tool results only say the call is
	// waiting; they are not failures. Older receipts carry none.
	Calls []string `json:"calls,omitempty"`
}

func (w WaitCheckpoint) Validate() error {
	if w.Revision == 0 || w.Requests < 1 || w.Requests > 16 || w.ExpiresAt <= 0 || len(w.Calls) > w.Requests {
		return errors.New("bounded committed wait checkpoint required")
	}
	for _, call := range w.Calls {
		if call == "" || len(call) > 256 || !utf8.ValidString(call) || strings.ContainsRune(call, 0) {
			return errors.New("bounded committed wait checkpoint required")
		}
	}
	return nil
}

// Read only after Run and owned cleanup have finished. Nil means no suspension.
type WaitingRunner interface{ WaitCheckpoint() *WaitCheckpoint }
