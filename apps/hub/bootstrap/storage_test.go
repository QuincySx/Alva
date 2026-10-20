// INPUT: Explicit factories, opaque provider contracts and real local demo model/tool execution.
// OUTPUT: End-to-end selected-provider use, missing/error no-fallback and retained failed-close ownership regressions.
// POS: [L7 assembly acceptance] Verifies that plugging a component decides storage throughout the running product.
package bootstrap

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"

	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/tests/storagecontract"
)

func TestProgramSelectedStorageFlowsThroughActualTurn(t *testing.T) {
	var selected *storagecontract.Provider
	calls := 0
	directory := filepath.Join(t.TempDir(), "program")
	factory := func(ctx context.Context, path string) (Stores, error) {
		calls++
		result, err := LocalStores(ctx, filepath.Join(path, "selected-by-program"))
		if err != nil {
			return result, err
		}
		selected = storagecontract.Wrap(result.Sessions)
		result.Sessions = selected
		result.Product = productPorts{result.Product}
		return result, nil
	}
	app, err := Open(t.Context(), Config{OpenStores: factory, DataDirectory: directory, Demo: true})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { closeApp(t, app) })
	if app.Repository != selected || calls != 1 {
		t.Fatal("assembly substituted the supplied component")
	}
	product, err := app.Service.Create(t.Context(), "selected component", "")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = app.Service.Submit(t.Context(), product.ID, "run through the supplied storage slot"); err != nil {
		t.Fatal(err)
	}
	awaitTerminal(t, app, product.ID)
	for _, method := range []string{"create", "open", "append", "references"} {
		if selected.Calls(method) == 0 {
			t.Fatal("component was bypassed", method)
		}
	}
	if _, err = os.Stat(filepath.Join(directory, "native", "native.sqlite")); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("component opened an implicit default fact store", err)
	}
	if _, err = os.Stat(filepath.Join(directory, "product", "product.sqlite")); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("component opened an implicit default product store", err)
	}
}

func TestMissingOrFailedStorageFactoryHasNoFallback(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "unopened")
	if app, err := Open(t.Context(), Config{DataDirectory: directory, Demo: true}); err == nil {
		app.Close(t.Context())
		t.Fatal("missing component silently selected a backend")
	}
	if _, err := os.Stat(directory); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("missing factory performed storage IO", err)
	}
	failure := errors.New("selected provider unavailable")
	calls := 0
	_, err := Open(t.Context(), Config{DataDirectory: directory, Demo: true, OpenStores: func(context.Context, string) (Stores, error) { calls++; return Stores{}, failure }})
	if !errors.Is(err, failure) || calls != 1 {
		t.Fatal("provider error retried or hidden", calls, err)
	}
	if _, err := os.Stat(filepath.Join(directory, "native")); !errors.Is(err, os.ErrNotExist) {
		t.Fatal("provider failure selected a fallback", err)
	}
}

type failedProductClose struct {
	ProductStorage
	failure error
	calls   int
}

func (s *failedProductClose) Close() error {
	s.calls++
	if s.calls == 1 {
		return s.failure
	}
	return s.ProductStorage.Close()
}
func TestProviderCloseFailureRetainsApplicationOwner(t *testing.T) {
	failure := errors.New("selected provider close incomplete")
	var selected *failedProductClose
	app, err := Open(t.Context(), Config{DataDirectory: filepath.Join(t.TempDir(), "private"), Demo: true, OpenStores: func(ctx context.Context, path string) (Stores, error) {
		result, err := testStores(ctx, path)
		if err != nil {
			return result, err
		}
		selected = &failedProductClose{ProductStorage: result.Product, failure: failure}
		result.Product = selected
		return result, nil
	}})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { closeApp(t, app) })
	if err = app.Close(t.Context()); !errors.Is(err, failure) || app.closed {
		t.Fatal("failed provider owner was discarded", err)
	}
	if err = app.Close(t.Context()); err != nil || !app.closed || selected.calls != 2 {
		t.Fatal("actual provider cleanup not retried", err)
	}
}

type failedBindingClose struct {
	storage.BindingStore
	failure error
	calls   int
}

func (s *failedBindingClose) Close() error {
	s.calls++
	if s.calls == 1 {
		return s.failure
	}
	return s.BindingStore.Close()
}

func TestFailedBindingFactoryRetainsExactPartialComponent(t *testing.T) {
	openFailure := errors.New("selected binding provider partially opened")
	closeFailure := errors.New("partial binding cleanup incomplete")
	var selected *failedBindingClose
	app, err := Open(t.Context(), Config{
		DataDirectory: filepath.Join(t.TempDir(), "private"), Demo: true, OpenStores: testStores,
		External: []ExternalEngine{{Vendor: "codex", Command: "/explicit-never-executed", ApprovedHostAccess: true}},
		OpenBindings: func(ctx context.Context, path string) (storage.BindingStore, error) {
			s, err := LocalBindings(ctx, path)
			if err != nil {
				return s, err
			}
			selected = &failedBindingClose{BindingStore: s, failure: closeFailure}
			return selected, openFailure
		},
	})
	if app == nil || !errors.Is(err, openFailure) || !errors.Is(err, closeFailure) || app.closed {
		t.Fatal("constructor lost a partial provider owner", app, err)
	}
	t.Cleanup(func() { closeApp(t, app) })
	if err := app.Close(t.Context()); err != nil || !app.closed || selected.calls != 2 {
		t.Fatal("exact provider cleanup was not retried", err)
	}
}
