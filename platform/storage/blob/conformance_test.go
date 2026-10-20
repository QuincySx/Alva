// INPUT: Real directory and volatile adapters, contexts, goroutines and immutable blob contracts.
// OUTPUT: Shared backend conformance covering identity, ownership, publication, ranges and failures.
// POS: [storage acceptance] Verifies interchangeable byte behavior without cloud accounts or fake transaction claims.
package blob_test

import (
	"bytes"
	"context"
	"errors"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"testing"

	"github.com/QuincySx/Alva/platform/storage/blob"
	"github.com/QuincySx/Alva/platform/storage/blob/directory"
	"github.com/QuincySx/Alva/platform/storage/blob/memory"
)

func adapters(t *testing.T) map[string]func(*testing.T) blob.Store {
	t.Helper()
	result := map[string]func(*testing.T) blob.Store{
		"memory": func(t *testing.T) blob.Store {
			s, err := memory.New(memory.Limits{Bytes: 1 << 20, Objects: 32})
			if err != nil {
				t.Fatal(err)
			}
			return s
		},
	}
	if directorySupported() {
		result["directory"] = func(t *testing.T) blob.Store {
			s, err := directory.Open(t.TempDir())
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() {
				if err := s.Close(); err != nil {
					t.Error(err)
				}
			})
			return s
		}
	}
	return result
}

func directorySupported() bool {
	return runtime.GOOS != "windows" && runtime.GOOS != "js" && runtime.GOOS != "wasip1"
}

func TestAdaptersConform(t *testing.T) {
	for name, open := range adapters(t) {
		t.Run(name, func(t *testing.T) {
			s := open(t)
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
		})
	}
}

func TestConcurrentImmutablePublication(t *testing.T) {
	for name, open := range adapters(t) {
		t.Run(name, func(t *testing.T) {
			s := open(t)
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
		})
	}
}

func TestMemoryLimitsDoNotEvictCommittedObjects(t *testing.T) {
	s, err := memory.New(memory.Limits{Bytes: 4, Objects: 2})
	if err != nil {
		t.Fatal(err)
	}
	keys := []string{strings.Repeat("a", 64), strings.Repeat("b", 64), strings.Repeat("c", 64)}
	if err = s.Put(t.Context(), keys[0], []byte("keep")); err != nil {
		t.Fatal(err)
	}
	if err = s.Put(t.Context(), keys[1], []byte("x")); !errors.Is(err, blob.ErrLimit) {
		t.Fatal("bytes budget", err)
	}
	if err = s.Put(t.Context(), keys[1], nil); err != nil {
		t.Fatal(err)
	}
	if err = s.Put(t.Context(), keys[2], nil); !errors.Is(err, blob.ErrLimit) {
		t.Fatal("object budget", err)
	}
	data, err := s.Read(t.Context(), keys[0], 0, 4)
	if err != nil || string(data) != "keep" {
		t.Fatal("evicted existing", err)
	}
}

func TestCompetingDifferentValuesNeverOverwrite(t *testing.T) {
	for name, open := range adapters(t) {
		t.Run(name, func(t *testing.T) {
			s := open(t)
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
		})
	}
}

func TestDirectoryReopenAndSymlinkRejection(t *testing.T) {
	if !directorySupported() {
		t.Skip("directory Sync adapter requires native filesystem support; volatile adapter is tested separately")
	}
	path := t.TempDir()
	alias := filepath.Join(t.TempDir(), "alias")
	if err := os.Symlink(path, alias); err != nil {
		t.Fatal(err)
	}
	for _, candidate := range []string{alias, alias + string(os.PathSeparator)} {
		unexpected, err := directory.Open(candidate)
		if err == nil {
			unexpected.Close()
			t.Fatal("symbolic root accepted with optional trailing separator")
		}
	}
	s, err := directory.Open(path)
	if err != nil {
		t.Fatal(err)
	}
	key, _ := blob.NewKey()
	if err = s.Put(t.Context(), key, []byte("survives close")); err != nil {
		t.Fatal(err)
	}
	if err = s.Close(); err != nil {
		t.Fatal(err)
	}
	s, err = directory.Open(path)
	if err != nil {
		t.Fatal(err)
	}
	defer s.Close()
	data, err := s.Read(t.Context(), key, 0, 100)
	if err != nil || string(data) != "survives close" {
		t.Fatal("reopen", err)
	}
	bad, _ := blob.NewKey()
	if err = os.Symlink(filepath.Join(path, key), filepath.Join(path, bad)); err != nil {
		t.Fatal(err)
	}
	if _, err = s.Read(t.Context(), bad, 0, 1); !errors.Is(err, blob.ErrInvalid) {
		t.Fatal("symlink read", err)
	}
	if err = s.Put(t.Context(), bad, []byte("unsafe")); !errors.Is(err, blob.ErrInvalid) {
		t.Fatal("symlink write", err)
	}
	if err = s.Close(); err != nil {
		t.Fatal(err)
	}
	if _, err = s.Read(t.Context(), key, 0, 1); !errors.Is(err, os.ErrClosed) {
		t.Fatal("closed", err)
	}
}
