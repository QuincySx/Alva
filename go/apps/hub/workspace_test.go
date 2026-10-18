// INPUT: Pure product workspace leases and cancellable deterministic waiters
// OUTPUT: Shared Alva admission, external exclusion, overlapping paths and writer fairness regressions
// POS: [L7 Hub product] No SDK scheduler or actual filesystem/process execution
package hub

import (
	"context"
	"path/filepath"
	"testing"
	"time"
)

func TestWorkspaceExternalLeaseExcludesOverlappingAlvaAndCancelsWait(t *testing.T) {
	registry := newWorkspaceRegistry()
	root := filepath.Join(t.TempDir(), "project")
	release, err := registry.acquire(context.Background(), root, true)
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
	defer cancel()
	if _, err = registry.acquire(ctx, filepath.Join(root, "child"), false); err == nil {
		t.Fatal("external runtime did not own overlapping workspace")
	}
	other, err := registry.acquire(context.Background(), root+"-sibling", false)
	if err != nil {
		t.Fatal(err)
	}
	other()
	release()
	first, err := registry.acquire(context.Background(), root, false)
	if err != nil {
		t.Fatal(err)
	}
	second, err := registry.acquire(context.Background(), root, false)
	if err != nil {
		t.Fatal(err)
	}
	second()
	first()
}
func TestWorkspacePendingExternalWriterCannotBeStarvedByLaterAlva(t *testing.T) {
	registry := newWorkspaceRegistry()
	root := t.TempDir()
	first, err := registry.acquire(context.Background(), root, false)
	if err != nil {
		t.Fatal(err)
	}
	writer := make(chan func(), 1)
	go func() { release, _ := registry.acquire(context.Background(), root, true); writer <- release }()
	deadline := time.Now().Add(time.Second)
	for {
		registry.mu.Lock()
		waiting := len(registry.waiting)
		registry.mu.Unlock()
		if waiting > 0 {
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("writer did not queue")
		}
		time.Sleep(time.Millisecond)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Millisecond)
	defer cancel()
	if _, err = registry.acquire(ctx, root, false); err == nil {
		t.Fatal("later shared execution bypassed older exclusive waiter")
	}
	first()
	select {
	case release := <-writer:
		release()
	case <-time.After(time.Second):
		t.Fatal("queued writer never acquired")
	}
}

func TestRetainingAdmittedWorkspaceDoesNotDeadlockPendingWriterAndSealsLateCallbacks(t *testing.T) {
	r := newWorkspaceRegistry()
	path := t.TempDir()
	release, err := r.acquire(t.Context(), path, false)
	if err != nil {
		t.Fatal(err)
	}
	seal, retain := retainableWorkspace(release)
	writer := make(chan func(), 1)
	go func() {
		release, err := r.acquire(t.Context(), path, true)
		if err == nil {
			writer <- release
		}
	}()
	deadline := time.Now().Add(time.Second)
	for {
		r.mu.Lock()
		waiting := len(r.waiting)
		r.mu.Unlock()
		if waiting > 0 {
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("writer did not queue")
		}
		time.Sleep(time.Millisecond)
	}
	// Extending the currently admitted reader must not join behind that writer.
	background, err := retain()
	if err != nil {
		t.Fatal(err)
	}
	seal()
	seal()
	if _, err := retain(); err == nil {
		t.Fatal("late factory retained a completed attempt's lease")
	}
	select {
	case release := <-writer:
		release()
		t.Fatal("writer entered while background resource was live")
	default:
	}
	background()
	background()
	select {
	case release := <-writer:
		release()
	case <-time.After(time.Second):
		t.Fatal("physical resource release did not unblock writer")
	}
}
