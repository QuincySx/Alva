// INPUT: context, database/sql, errors, testing, ncruces SQLite error codes and the session package's ownCancellation.
// OUTPUT: Regression that a context ending inside a storage call stays the caller's own cancellation.
// POS: [L2 platform storage] Keeps a user Stop or deadline from being misreported as a storage fault by every layer above.
package session

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"testing"

	"github.com/ncruces/go-sqlite3"
)

func TestOwnCancellationOnlyRewritesCancellationSideEffectsOfADoneContext(t *testing.T) {
	done, cancel := context.WithCancel(t.Context())
	cancel()
	for _, effect := range []error{sqlite3.INTERRUPT, sql.ErrTxDone} {
		if got := ownCancellation(done, fmt.Errorf("append typed record: %w", effect)); got != done.Err() {
			t.Fatalf("%v under a done context = %v, want the context error", effect, got)
		}
		if got := ownCancellation(t.Context(), effect); got != effect {
			t.Fatalf("%v under a live context was rewritten to %v", effect, got)
		}
	}
	disk := errors.New("disk full")
	if got := ownCancellation(done, disk); got != disk {
		t.Fatalf("a real storage fault was hidden by cancellation: %v", got)
	}
	if ownCancellation(done, nil) != nil {
		t.Fatal("nil error invented")
	}
}
