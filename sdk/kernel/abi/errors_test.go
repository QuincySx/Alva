// INPUT: context, errors, fmt, testing
// OUTPUT: Typed session failure and cancellation-tree regressions
// POS: [L1 kernel contracts] Prevents task cancellation from concealing failed conversation authority operations.
package abi

import (
	"context"
	"errors"
	"fmt"
	"testing"
)

func TestOnlyCancellationPreservesSessionFailure(t *testing.T) {
	storage := &SessionError{Operation: "flush", SessionID: "s", Err: context.DeadlineExceeded}
	for _, err := range []error{nil, errors.New("failure"), storage, fmt.Errorf("child: %w", storage), errors.Join(context.Canceled, storage), errors.Join(context.Canceled, errors.New("lost commit"))} {
		if OnlyCancellation(err) {
			t.Fatalf("misclassified %v", err)
		}
	}
	for _, err := range []error{context.Canceled, context.DeadlineExceeded, fmt.Errorf("child: %w", context.Canceled), errors.Join(context.Canceled, context.DeadlineExceeded)} {
		if !OnlyCancellation(err) {
			t.Fatalf("not cancellation: %v", err)
		}
	}
	if !errors.Is(storage, context.DeadlineExceeded) {
		t.Fatal("typed storage error lost cause")
	}
	var actual *SessionError
	if !errors.As(fmt.Errorf("child: %w", storage), &actual) || actual.Operation != "flush" || actual.SessionID != "s" {
		t.Fatal("typed session evidence unavailable")
	}
}
