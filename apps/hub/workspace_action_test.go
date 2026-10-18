// INPUT: Runtime workspace leases, cooperative mutation callbacks and shutdown barriers
// OUTPUT: Exclusive admission, overlapping-path deduplication and real owner cancellation/join regressions
// POS: [L7 Hub product] No engine or filesystem side effects are simulated as completed product adoption
package hub

import (
	"context"
	"errors"
	"path/filepath"
	"testing"
	"time"
)

func TestWorkspaceMutationIsExclusiveOwnedAndCloseWaitsPhysicalCompletion(t *testing.T) {
	r := New(nil)
	path := t.TempDir()
	release, err := r.workspaces.acquire(t.Context(), path, false)
	if err != nil {
		t.Fatal(err)
	}
	entered := make(chan struct{})
	cancelled := make(chan struct{})
	finish := make(chan struct{})
	done := make(chan error, 1)
	go func() {
		done <- r.WithExclusiveWorkspaces(context.Background(), []string{filepath.Join(path, "child"), path}, func(ctx context.Context) error {
			close(entered)
			<-ctx.Done()
			close(cancelled)
			<-finish
			return ctx.Err()
		})
	}()
	select {
	case <-entered:
		t.Fatal("mutation bypassed existing shared lease")
	case <-time.After(20 * time.Millisecond):
	}
	release()
	select {
	case <-entered:
	case <-time.After(time.Second):
		t.Fatal("overlapping own paths deadlocked")
	}
	short, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
	defer cancel()
	if err = r.Close(short); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatal("Close claimed active callback complete", err)
	}
	<-cancelled
	if !r.HasActive() {
		t.Fatal("Close lost still-owned callback")
	}
	close(finish)
	if err = <-done; !errors.Is(err, context.Canceled) {
		t.Fatal("owner cancellation lost", err)
	}
	if err = r.Close(t.Context()); err != nil || r.HasActive() {
		t.Fatal("retry did not join actual completion", err)
	}
	if err = r.WithExclusiveWorkspaces(t.Context(), []string{path}, func(context.Context) error { return nil }); err == nil {
		t.Fatal("closed Runtime accepted mutation")
	}
}
