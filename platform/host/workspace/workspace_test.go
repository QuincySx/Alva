// INPUT: Actual pinned Workspace, security Guard and temporary directories
// OUTPUT: Confined directory creation and actual file preservation after callback failure/cancellation
// POS: [host capability] Validates directory ownership independently from model file tools.
package workspace

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"

	"github.com/QuincySx/Alva/sdk/agent/security"
)

func TestMakeDirectoryIsConfinedAndNonRecursive(t *testing.T) {
	w, err := OpenWorkspace(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer w.Close()
	guard, err := security.NewGuard(w.Directory(), nil, w, security.NewPermissionManager(security.AcceptEdits, nil))
	if err != nil {
		t.Fatal(err)
	}
	if err := w.MakeDirectory(context.Background(), "memory", guard); err != nil {
		t.Fatal(err)
	}
	if err := w.MakeDirectory(context.Background(), "memory", guard); err != nil {
		t.Fatal(err)
	}
	if err := w.MakeDirectory(context.Background(), "missing/child", guard); err == nil {
		t.Fatal("unexpected recursive create")
	}
	if err := w.MakeDirectory(context.Background(), "../outside", guard); err == nil {
		t.Fatal("directory escaped workspace")
	}
	if err := os.Symlink(t.TempDir(), filepath.Join(w.Directory(), "alias-dir")); err != nil {
		t.Fatal(err)
	}
	if err := w.MakeDirectory(context.Background(), "alias-dir/child", guard); err == nil {
		t.Fatal("symlink directory mutation accepted")
	}
}

func TestUpdateCallbackFailureAndCancellationKeepOriginal(t *testing.T) {
	w, err := OpenWorkspace(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer w.Close()
	guard, err := security.NewGuard(w.Directory(), nil, w, nil)
	if err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(w.Directory(), "original")
	if err := os.WriteFile(path, []byte("original bytes"), 0600); err != nil {
		t.Fatal(err)
	}
	backupFailure := errors.New("backup unavailable")
	err = w.Update(t.Context(), path, guard, func(previous []byte, exists bool) ([]byte, error) {
		if !exists || string(previous) != "original bytes" {
			t.Fatal("transformation did not receive the actual prior file")
		}
		return []byte("must not write"), backupFailure
	})
	if !errors.Is(err, backupFailure) {
		t.Fatal("callback failure was lost", err)
	}
	ctx, cancel := context.WithCancel(t.Context())
	err = w.Update(ctx, path, guard, func([]byte, bool) ([]byte, error) {
		cancel()
		return []byte("must not write after cancellation"), nil
	})
	if !errors.Is(err, context.Canceled) {
		t.Fatal("cancelled transformation was reported as successful", err)
	}
	actual, err := os.ReadFile(path)
	entries, listErr := os.ReadDir(w.Directory())
	if err != nil || listErr != nil || string(actual) != "original bytes" || len(entries) != 1 {
		t.Fatal("failed update changed the original or retained a staging file", string(actual), err, listErr)
	}
}
