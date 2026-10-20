// INPUT: Real directory and volatile adapters, public blobtest and local filesystem fixtures.
// OUTPUT: Shared backend conformance covering identity, ownership, publication, ranges and failures.
// POS: [storage acceptance] Verifies interchangeable byte behavior without cloud accounts or fake transaction claims.
package blob_test

import (
	"errors"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"

	"github.com/QuincySx/Alva/platform/storage/blob"
	"github.com/QuincySx/Alva/platform/storage/blob/blobtest"
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
		t.Run(name, func(t *testing.T) { blobtest.Run(t, open) })
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
