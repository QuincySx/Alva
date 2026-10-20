// INPUT: Explicit next-turn persistent-memory choice and actual canonical saved workspace
// OUTPUT: Lazily opened private memory store and non-model-selectable workspace namespace
// POS: [L7 Hub product] Shared product memory assembly; separate from native execution facts and no automatic scan/embedding
package bootstrap

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"path/filepath"

	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/sdk/agent/memory"
)

func (a *Application) workspaceMemory(ctx context.Context, workspace string) (memory.Backend, error) {
	if a.Service == nil || !filepath.IsAbs(workspace) {
		return nil, errors.New("actual product workspace required for configured memory")
	}
	// Namespace belongs to the product's saved canonical workspace path. The
	// tool/model cannot select it or bind an arbitrary other project's memory.
	digest := sha256.Sum256([]byte("alva.workspace-memory.v1\x00" + filepath.Clean(workspace)))
	namespace := "workspace-" + hex.EncodeToString(digest[:])
	a.memoryMu.Lock()
	defer a.memoryMu.Unlock()
	if a.memoryErr != nil {
		return nil, a.memoryErr
	}
	if a.memory == nil {
		if a.openMemory == nil {
			return nil, errors.New("workspace memory provider was not installed by assembly")
		}
		store, err := a.openMemory(ctx, filepath.Join(a.Service.DataDirectory, "memory"))
		if !storage.Missing(store) {
			a.memory = store
		}
		if err != nil {
			a.memoryErr = err
			return nil, err
		}
		if a.memory == nil {
			return nil, errors.New("memory factory returned a missing or typed-nil provider")
		}
	}
	backend, err := a.memory.Bind(namespace)
	if err != nil {
		return nil, err
	}
	if storage.Missing(backend) {
		return nil, errors.New("memory namespace returned a missing or typed-nil backend")
	}
	return backend, nil
}
