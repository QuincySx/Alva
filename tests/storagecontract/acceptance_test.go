// INPUT: Caller-selected local providers, opaque contract wrappers and public storagetest helpers.
// OUTPUT: The reusable repository/binding suites run through the same ports used by applications.
// POS: [storage acceptance] Backend-specific durability tests remain with each implementation.
//go:build !js && !wasip1

package storagecontract_test

import (
	"path/filepath"
	"testing"

	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/platform/storage/bindings"
	"github.com/QuincySx/Alva/platform/storage/session"
	"github.com/QuincySx/Alva/platform/storage/storagetest"
	"github.com/QuincySx/Alva/tests/storagecontract"
)

func TestRepositoryConformance(t *testing.T) {
	storagetest.RunRepository(t, func(t *testing.T) storage.Repository {
		r, err := session.Open(t.Context(), filepath.Join(t.TempDir(), "facts"))
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() {
			if err := r.Close(); err != nil {
				t.Error(err)
			}
		})
		return storagecontract.Wrap(r)
	})
}

func TestBindingConformance(t *testing.T) {
	storagetest.RunBindings(t, func(t *testing.T) storage.BindingStore {
		s, err := bindings.Open(t.Context(), filepath.Join(t.TempDir(), "bindings"))
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() {
			if err := s.Close(); err != nil {
				t.Error(err)
			}
		})
		return s
	})
}
