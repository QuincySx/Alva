// INPUT: Explicit storage factories, Hub storage contracts and native storage providers.
// OUTPUT: ProductStorage and explicit Stores/Memory/Bindings factories with optional Local recipes.
// POS: [L7 composition] Selects concrete persistence only when the program chooses this recipe; components receive contracts.
package bootstrap

import (
	"context"
	"path/filepath"

	"github.com/QuincySx/Alva/apps/hub"
	productstore "github.com/QuincySx/Alva/apps/hub/sqlite"
	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/platform/storage/bindings"
	memorysqlite "github.com/QuincySx/Alva/platform/storage/memory"
	"github.com/QuincySx/Alva/platform/storage/session"
)

// ProductStorage is the complete current Hub product profile, composed from
// the same consumer contracts used by Runtime, settings, Team and automation.
type ProductStorage interface {
	hub.Store
	hub.ToolSettingsStore
	hub.TeamStore
	hub.WorkerDraftStore
	hub.AutomationStore
	hub.WorktreeStore
	hub.NativeOwnerResolver
	Close() error
}

type Stores struct {
	Product  ProductStorage
	Sessions storage.Repository
}

// Factories are selected by the program/Plugin assembly, never by components.
// A StoresFactory may return partial resources with an error; Application keeps
// their ownership and closes them through the same shutdown sequence.
type StoresFactory func(context.Context, string) (Stores, error)
type MemoryFactory func(context.Context, string) (storage.MemoryStore, error)
type BindingsFactory func(context.Context, string) (storage.BindingStore, error)

func LocalStores(ctx context.Context, directory string) (Stores, error) {
	var result Stores
	product, err := productstore.Open(ctx, filepath.Join(directory, "product"))
	if err != nil {
		return result, err
	}
	result.Product = product
	result.Sessions, err = session.Open(ctx, filepath.Join(directory, "native"))
	return result, err
}

func LocalMemory(ctx context.Context, directory string) (storage.MemoryStore, error) {
	return memorysqlite.Open(ctx, directory)
}
func LocalBindings(ctx context.Context, directory string) (storage.BindingStore, error) {
	return bindings.Open(ctx, directory)
}
