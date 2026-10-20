// INPUT: Shared identity storage contract and explicitly opened temporary native providers.
// OUTPUT: Atomic create-or-verify, conflicting ownership, reopen and exclusive lifecycle regressions.
// POS: [storage adapter acceptance] Verifies routing facts without starting any external runtime.
package bindings_test

import (
	"errors"
	"path/filepath"
	"sync"
	"testing"

	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/platform/storage/bindings"
)

func TestExactBindingSurvivesReopenAndCannotBeRetargeted(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "explicit")
	s, err := bindings.Open(t.Context(), directory)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = s.Close() })
	if _, err := s.ReadBinding(t.Context(), "missing"); !errors.Is(err, storage.ErrNotFound) {
		t.Fatal(err)
	}
	want := storage.SessionBinding{ID: "actual-native", Owner: "product-a", Workspace: "/actual-workspace"}
	if err := s.PutBinding(t.Context(), want); err != nil {
		t.Fatal(err)
	}
	if err := s.PutBinding(t.Context(), want); err != nil {
		t.Fatal("identical binding is not idempotent", err)
	}
	for _, changed := range []storage.SessionBinding{
		{ID: want.ID, Owner: "product-b", Workspace: want.Workspace},
		{ID: want.ID, Owner: want.Owner, Workspace: "/another-workspace"},
	} {
		if err := s.PutBinding(t.Context(), changed); !errors.Is(err, storage.ErrBindingConflict) {
			t.Fatal("retarget accepted", err)
		}
	}
	if other, err := bindings.Open(t.Context(), directory); err == nil {
		other.Close()
		t.Fatal("second process authority admitted")
	}
	if err := s.Close(); err != nil {
		t.Fatal(err)
	}
	if _, err := s.ReadBinding(t.Context(), want.ID); !errors.Is(err, storage.ErrClosed) {
		t.Fatal(err)
	}
	if err := s.Close(); err != nil {
		t.Fatal("close is not idempotent", err)
	}
	s, err = bindings.Open(t.Context(), directory)
	if err != nil {
		t.Fatal(err)
	}
	if got, err := s.ReadBinding(t.Context(), want.ID); err != nil || got != want {
		t.Fatal("actual identity changed on reopen", got, err)
	}
}

func TestCompetingBindingsHaveOneAtomicWinner(t *testing.T) {
	s, err := bindings.Open(t.Context(), filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = s.Close() })
	start := make(chan struct{})
	results := make(chan error, 2)
	var workers sync.WaitGroup
	for _, owner := range []string{"product-a", "product-b"} {
		workers.Go(func() {
			<-start
			results <- s.PutBinding(t.Context(), storage.SessionBinding{ID: "same-native", Owner: owner, Workspace: "/same-workspace"})
		})
	}
	close(start)
	workers.Wait()
	close(results)
	var success, conflicts int
	for err := range results {
		if err == nil {
			success++
		} else if errors.Is(err, storage.ErrBindingConflict) {
			conflicts++
		} else {
			t.Fatal(err)
		}
	}
	if success != 1 || conflicts != 1 {
		t.Fatal("partial or multiple binding winners", success, conflicts)
	}
}
