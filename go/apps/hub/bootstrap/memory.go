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

	memorysqlite "github.com/QuincySx/Alva/go/platform/storage/memory"
)

func (a *Application) workspaceMemory(ctx context.Context, workspace string) (*memorysqlite.Backend, error) {
	if a.Service == nil || !filepath.IsAbs(workspace) {
		return nil, errors.New("actual product workspace required for configured memory")
	}
	// Namespace belongs to the product's saved canonical workspace path. The
	// tool/model cannot select it or bind an arbitrary other project's memory.
	digest := sha256.Sum256([]byte("alva.workspace-memory.v1\x00" + filepath.Clean(workspace)))
	namespace := "workspace-" + hex.EncodeToString(digest[:])
	a.memoryMu.Lock()
	defer a.memoryMu.Unlock()
	if a.memory == nil {
		store, err := memorysqlite.Open(ctx, filepath.Join(a.Service.DataDirectory, "memory"))
		if err != nil {
			return nil, err
		}
		a.memory = store
	}
	return a.memory.Bind(namespace)
}
