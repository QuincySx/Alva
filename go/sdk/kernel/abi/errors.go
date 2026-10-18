// INPUT: context, fmt
// OUTPUT: SessionError, OnlyCancellation
// POS: [L1 kernel contracts] Typed storage failures and lossless cancellation classification shared by SDK consumers.
package abi

import (
	"context"
	"fmt"
)

// SessionError identifies a failed conversation authority operation. A storage
// timeout is not an ordinary task timeout: even when Err is DeadlineExceeded,
// callers must not turn a possibly failed commit/flush into a successful stop.
type SessionError struct {
	Operation string
	SessionID string
	Err       error
}

func (err *SessionError) Error() string {
	return fmt.Sprintf("session %q %s: %v", err.SessionID, err.Operation, err.Err)
}
func (err *SessionError) Unwrap() error { return err.Err }

// OnlyCancellation reports whether all leaves of a nonempty error tree are
// context cancellation/deadline errors. It deliberately does not use errors.Is:
// Join(context.Canceled, storageFailure) is not an ordinary cancellation.
// Typed session failures always retain their stronger failure meaning.
func OnlyCancellation(err error) bool {
	if err == nil {
		return false
	}
	if _, ok := err.(*SessionError); ok {
		return false
	}
	if joined, ok := err.(interface{ Unwrap() []error }); ok {
		children := joined.Unwrap()
		if len(children) == 0 {
			return false
		}
		for _, child := range children {
			if !OnlyCancellation(child) {
				return false
			}
		}
		return true
	}
	if wrapped, ok := err.(interface{ Unwrap() error }); ok {
		return OnlyCancellation(wrapped.Unwrap())
	}
	return err == context.Canceled || err == context.DeadlineExceeded
}
