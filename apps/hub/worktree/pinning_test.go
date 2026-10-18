// INPUT: Actual pinned directory handles, renamed ancestors and late registered owned process barriers
// OUTPUT: No internal-symlink retargeting and truthful Close retry after a child missed the first snapshot
// POS: [L7 Hub product] Temporary fixtures only; final hash-to-rename races with noncooperating external writers remain documented
package worktree

import (
	"context"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/QuincySx/Alva/platform/system/process"
)

func TestPinnedParentCannotRetargetReadsOrReplacementThroughInternalSymlink(t *testing.T) {
	dir := t.TempDir()
	for _, name := range []string{"original", "other"} {
		if err := os.Mkdir(filepath.Join(dir, name), 0700); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.WriteFile(filepath.Join(dir, "original", "x"), []byte("original"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, "other", "x"), []byte("not approved"), 0600); err != nil {
		t.Fatal(err)
	}
	root, err := os.OpenRoot(dir)
	if err != nil {
		t.Fatal(err)
	}
	defer root.Close()
	parent, name, err := pinParent(root, "original/x", false)
	if err != nil {
		t.Fatal(err)
	}
	defer parent.Close()
	if err = os.Rename(filepath.Join(dir, "original"), filepath.Join(dir, "moved")); err != nil {
		t.Fatal(err)
	}
	if err = os.Symlink("other", filepath.Join(dir, "original")); err != nil {
		t.Fatal(err)
	}
	state, exists, err := readPinned(parent, name)
	if err != nil || !exists || string(state.Data) != "original" {
		t.Fatal("pinned read followed replaced ancestor", err)
	}
	if _, _, err = readLiteral(root, "original/x"); err == nil {
		t.Fatal("fresh symlink ancestor accepted")
	}
	file, err := parent.OpenFile("stage", os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = file.Write([]byte("literal replacement")); err != nil {
		t.Fatal(err)
	}
	if err = file.Close(); err != nil {
		t.Fatal(err)
	}
	if err = parent.Rename("stage", name); err != nil {
		t.Fatal(err)
	}
	data, err := os.ReadFile(filepath.Join(dir, "other", "x"))
	if err != nil || string(data) != "not approved" {
		t.Fatal("pinned basename operation followed root-internal symlink", err)
	}
}
func TestCloseRetainsLateRegisteredActualChildAndRootUntilExplicitRetry(t *testing.T) {
	m, _ := managerFixture(t)
	end, err := m.begin()
	if err != nil {
		t.Fatal(err)
	}
	closed := make(chan error, 1)
	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		closed <- m.Close(ctx)
	}()
	<-m.life.Done()
	child, err := process.Start(process.Config{Command: "/bin/sleep", Args: []string{"30"}, Cwd: m.config.Root})
	if err != nil {
		end()
		t.Fatal(err)
	}
	m.mu.Lock()
	m.children[child] = struct{}{}
	m.mu.Unlock()
	end()
	if err = <-closed; err == nil || m.CleanupComplete() {
		t.Fatal("Close fabricated completion for late owned child", err)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err = m.Close(ctx); err != nil || !m.CleanupComplete() || !child.CleanupComplete() {
		t.Fatal("explicit retry lost exact physical owner", err)
	}
}
