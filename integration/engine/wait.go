// INPUT: A native runner's committed durable checkpoint metadata.
// OUTPUT: WaitCheckpoint and optional WaitingRunner terminal receipt contract.
// POS: [L4.5 engine contract] Execution may end waiting without claiming task success or exposing native wait payloads.
package engine

import "errors"

type WaitCheckpoint struct {
	Revision  uint64 `json:"revision"`
	Requests  int    `json:"requests"`
	ExpiresAt int64  `json:"expires_at"`
}

func (w WaitCheckpoint) Validate() error {
	if w.Revision == 0 || w.Requests < 1 || w.Requests > 16 || w.ExpiresAt <= 0 {
		return errors.New("bounded committed wait checkpoint required")
	}
	return nil
}

// Read only after Run and owned cleanup have finished. Nil means no suspension.
type WaitingRunner interface{ WaitCheckpoint() *WaitCheckpoint }
