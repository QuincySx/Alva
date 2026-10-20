// INPUT: testing, contexts and the public immutable blob.Store contract.
// OUTPUT: Run reusable immutable-byte conformance for caller-selected implementations.
// POS: [storage contract testing] Backend authors verify publication and bounded reads without importing an adapter.
package blobtest

import (
	"bytes"
	"context"
	"errors"
	"strings"
	"sync"
	"testing"

	"github.com/QuincySx/Alva/platform/storage/blob"
)

// Run opens a fresh empty store per case. The factory owns cleanup and must
// allow at least four objects and 64KiB of bytes. Calls use small public fixtures;
// disk durability, credentials, close behavior and provider outages need separate
// adapter tests. Passing does not prove distributed transactions or retry safety
// after an ambiguous publication error.
func Run(t *testing.T, open func(*testing.T) blob.Store) {
	t.Helper()
	for _, test := range []struct {
		name string
		run  func(*testing.T, blob.Store)
	}{
		{"identity_and_ranges", identityAndRanges},
		{"concurrent_identical_publication", concurrentIdenticalPublication},
		{"concurrent_conflicting_publication", concurrentConflictingPublication},
	} {
		t.Run(test.name, func(t *testing.T) { test.run(t, open(t)) })
	}
}

func identityAndRanges(t *testing.T, s blob.Store) {
	key, err := blob.NewKey()
	if err != nil {
		t.Fatal(err)
	}
	if _, err = s.Read(t.Context(), key, 0, 1); !errors.Is(err, blob.ErrNotFound) {
		t.Fatalf("missing: %v", err)
	}
	data := []byte(strings.Repeat("ab中", 3000))
	original := bytes.Clone(data)
	if err = s.Put(t.Context(), key, data); err != nil {
		t.Fatal(err)
	}
	data[0] = 'X'
	page, err := s.Read(t.Context(), key, 4090, 4096)
	if err != nil || !bytes.Equal(page, original[4090:8186]) {
		t.Fatalf("bounded read: %v", err)
	}
	page[0] = 'X'
	page, err = s.Read(t.Context(), key, 4090, 1)
	if err != nil || page[0] != original[4090] {
		t.Fatal("read buffer aliases store")
	}
	if err = s.Put(t.Context(), key, original); err != nil {
		t.Fatal("identical retry:", err)
	}
	if err = s.Put(t.Context(), key, data); !errors.Is(err, blob.ErrConflict) {
		t.Fatalf("overwrite: %v", err)
	}
	if page, err = s.Read(t.Context(), key, int64(len(original)), 1); err != nil || len(page) != 0 {
		t.Fatalf("EOF: %v", err)
	}
	for _, pair := range []struct {
		offset int64
		limit  int
	}{{-1, 1}, {int64(len(original) + 1), 1}, {0, 0}, {0, blob.MaxReadBytes + 1}} {
		if _, err = s.Read(t.Context(), key, pair.offset, pair.limit); !errors.Is(err, blob.ErrInvalid) {
			t.Fatalf("invalid range: %v", err)
		}
	}
	for _, bad := range []string{"../escape", strings.Repeat("A", 64), strings.Repeat("a", 63)} {
		if err = s.Put(t.Context(), bad, nil); !errors.Is(err, blob.ErrInvalid) {
			t.Fatalf("invalid key: %v", err)
		}
	}
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	other, _ := blob.NewKey()
	if err = s.Put(ctx, other, []byte("cancel")); !errors.Is(err, context.Canceled) {
		t.Fatalf("cancel write: %v", err)
	}
	if _, err = s.Read(ctx, key, 0, 1); !errors.Is(err, context.Canceled) {
		t.Fatalf("cancel read: %v", err)
	}
	if _, err = s.Read(t.Context(), other, 0, 1); !errors.Is(err, blob.ErrNotFound) {
		t.Fatalf("cancel before publish: %v", err)
	}
	empty, _ := blob.NewKey()
	if err = s.Put(t.Context(), empty, nil); err != nil {
		t.Fatal(err)
	}
	if page, err = s.Read(t.Context(), empty, 0, 1); err != nil || len(page) != 0 {
		t.Fatalf("empty: %v", err)
	}
}

func concurrentIdenticalPublication(t *testing.T, s blob.Store) {
	key, _ := blob.NewKey()
	var wg sync.WaitGroup
	for range 12 {
		wg.Go(func() {
			if err := s.Put(t.Context(), key, []byte("same immutable bytes")); err != nil {
				t.Error(err)
			}
		})
	}
	wg.Wait()
	page, err := s.Read(t.Context(), key, 0, 100)
	if err != nil || string(page) != "same immutable bytes" {
		t.Fatalf("publication: %q %v", page, err)
	}
}

func concurrentConflictingPublication(t *testing.T, s blob.Store) {
	key, _ := blob.NewKey()
	type result struct {
		value string
		err   error
	}
	results := make(chan result, 12)
	var wg sync.WaitGroup
	for i := range 12 {
		value := strings.Repeat(string(rune('a'+i%2)), 4097)
		wg.Go(func() { results <- result{value, s.Put(t.Context(), key, []byte(value))} })
	}
	wg.Wait()
	close(results)
	actual, err := s.Read(t.Context(), key, 0, 8192)
	if err != nil {
		t.Fatal(err)
	}
	for r := range results {
		if r.value == string(actual) {
			if r.err != nil {
				t.Fatal("winning immutable value not idempotent", r.err)
			}
		} else if !errors.Is(r.err, blob.ErrConflict) {
			t.Fatal("conflicting creator overwrote/published partial bytes", r.err)
		}
	}
}
