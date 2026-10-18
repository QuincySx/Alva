// INPUT: context, errors, os/filepath, testing, security Guard and actual Workspace.
// OUTPUT: Real pinned snapshot/hash-conflict/mode/atomic-write/symlink/foreign-snapshot regressions.
// POS: [L4 extension] Native checked replacement is tested without pretending OS-level hash-CAS or executing notebook code.
package builtin

import (
	"context"
	"errors"
	"github.com/QuincySx/Alva/go/sdk/agent/security"
	"os"
	"path/filepath"
	"testing"
)

func TestCheckedSnapshotActualModeAndHashConflict(t *testing.T) {
	ctx := context.Background()
	cwd := t.TempDir()
	path := filepath.Join(cwd, "notes.ipynb")
	if err := os.WriteFile(path, []byte("old🙂"), 0640); err != nil {
		t.Fatal(err)
	}
	if err := os.Chmod(path, 0640); err != nil {
		t.Fatal(err)
	}
	w, err := OpenWorkspace(cwd)
	if err != nil {
		t.Fatal(err)
	}
	defer w.Close()
	guard, _ := security.NewGuard(cwd, nil, w, nil)
	before, err := w.ReadSnapshot(ctx, path, guard)
	if err != nil {
		t.Fatal(err)
	}
	if _, err = w.ReplaceCheckedSnapshot(ctx, path, before, []byte("new🙂"), guard); err != nil {
		t.Fatal(err)
	}
	data, _ := os.ReadFile(path)
	info, _ := os.Stat(path)
	if string(data) != "new🙂" || info.Mode().Perm() != 0640 {
		t.Fatal("actual bytes/mode not preserved")
	}
	if _, err = w.ReplaceCheckedSnapshot(ctx, path, before, []byte("must not replace"), guard); !errors.Is(err, ErrSnapshotChanged) {
		t.Fatal("stale snapshot overwrote actual file", err)
	}
	current, err := w.ReadSnapshot(ctx, path, guard)
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(path, []byte("external writer"), 0640); err != nil {
		t.Fatal(err)
	}
	if _, err = w.ReplaceCheckedSnapshot(ctx, path, current, []byte("must not replace"), guard); !errors.Is(err, ErrSnapshotChanged) {
		t.Fatal("external hash change accepted", err)
	}
}
func TestSnapshotsRejectSymlinkAncestorTargetAndForgedSnapshot(t *testing.T) {
	ctx := context.Background()
	cwd := t.TempDir()
	outside := t.TempDir()
	os.WriteFile(filepath.Join(outside, "private"), []byte("outside"), 0600)
	w, err := OpenWorkspace(cwd)
	if err != nil {
		t.Fatal(err)
	}
	defer w.Close()
	guard, _ := security.NewGuard(cwd, nil, w, nil)
	if err = os.Symlink(outside, filepath.Join(cwd, "link")); err != nil {
		t.Skip(err)
	}
	if _, err = w.ReadSnapshot(ctx, "link/private", guard); err == nil {
		t.Fatal("symlink ancestor accepted")
	}
	file := filepath.Join(cwd, "regular")
	os.WriteFile(file, []byte("original"), 0600)
	before, err := w.ReadSnapshot(ctx, file, guard)
	if err != nil {
		t.Fatal(err)
	}
	os.Remove(file)
	os.Symlink(filepath.Join(outside, "private"), file)
	if _, err = w.ReplaceCheckedSnapshot(ctx, file, before, []byte("must not write"), guard); err == nil {
		t.Fatal("target symlink accepted")
	}
	if _, err = w.ReplaceCheckedSnapshot(ctx, "other", FileSnapshot{Data: []byte("fake"), SHA256: "fake"}, []byte("x"), guard); err == nil {
		t.Fatal("forged detached snapshot accepted")
	}
	data, _ := os.ReadFile(filepath.Join(outside, "private"))
	if string(data) != "outside" {
		t.Fatal("outside target changed")
	}
}

func TestSnapshotUsesOriginalPinnedWorkspaceAfterPathReplacement(t *testing.T) {
	ctx := context.Background()
	base := t.TempDir()
	cwd := filepath.Join(base, "original")
	if err := os.Mkdir(cwd, 0700); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(cwd, "notes.ipynb")
	os.WriteFile(path, []byte("original bytes"), 0600)
	w, err := OpenWorkspace(cwd)
	if err != nil {
		t.Fatal(err)
	}
	defer w.Close()
	guard, _ := security.NewGuard(cwd, nil, w, nil)
	before, err := w.ReadSnapshot(ctx, path, guard)
	if err != nil {
		t.Fatal(err)
	}
	moved := filepath.Join(base, "moved")
	if err = os.Rename(cwd, moved); err != nil {
		t.Fatal(err)
	}
	os.Mkdir(cwd, 0700)
	os.WriteFile(path, []byte("ungranted replacement"), 0600)
	if _, err = w.ReplaceCheckedSnapshot(ctx, path, before, []byte("changed pinned original"), guard); err != nil {
		t.Fatal(err)
	}
	actual, _ := os.ReadFile(filepath.Join(moved, "notes.ipynb"))
	replacement, _ := os.ReadFile(path)
	if string(actual) != "changed pinned original" || string(replacement) != "ungranted replacement" {
		t.Fatal("checked snapshot reopened displayed absolute path instead of pinned authority")
	}
}
