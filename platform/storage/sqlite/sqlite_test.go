// INPUT: context, os, filepath, runtime, testing.
// OUTPUT: Private native database files, pragmas and ownership lock regressions.
// POS: [platform / durable storage] Deterministic SQLite host checks without application data or credentials.
package sqlite

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"runtime"
	"testing"

	"github.com/QuincySx/Alva/platform/system/privatefs"
)

func requireUnix(t *testing.T) {
	t.Helper()
	if runtime.GOOS == "windows" {
		t.Skip("Unix mode-bit assertions; Windows has separate native DACL/LockFileEx regressions")
	}
}
func privateTestDir(t *testing.T) string {
	t.Helper()
	dir, err := privatefs.EnsureDir(filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	return dir
}
func TestPrivateDatabaseAndExclusiveOwner(t *testing.T) {
	requireUnix(t)
	dir := privateTestDir(t)
	lock, err := LockDatabase(dir, "fixture.sqlite")
	if err != nil {
		t.Fatal(err)
	}
	if _, err = LockDatabase(dir, "fixture.sqlite"); !errors.Is(err, ErrLocked) {
		t.Fatalf("second process owner accepted: %v", err)
	}
	db, err := Open(context.Background(), dir, "fixture.sqlite")
	if err != nil {
		t.Fatal(err)
	}
	var foreignKeys int
	var journal string
	if err = db.QueryRow("PRAGMA foreign_keys").Scan(&foreignKeys); err != nil {
		t.Fatal(err)
	}
	if err = db.QueryRow("PRAGMA journal_mode").Scan(&journal); err != nil {
		t.Fatal(err)
	}
	if foreignKeys != 1 || journal != "delete" {
		t.Fatalf("unsafe pragmas: %d %s", foreignKeys, journal)
	}
	db.Close()
	lock.Close()
	lock.Close()
	for _, name := range []string{"fixture.sqlite", "fixture.sqlite.lock"} {
		info, err := os.Stat(filepath.Join(dir, name))
		if err != nil || info.Mode().Perm() != 0600 {
			t.Fatalf("wrong private mode: %s %v", name, err)
		}
	}
	next, err := LockDatabase(dir, "fixture.sqlite")
	if err != nil {
		t.Fatal(err)
	}
	next.Close()
}
func TestRejectsWideOrLinkedFilesWithoutChangingThem(t *testing.T) {
	requireUnix(t)
	t.Run("directory", func(t *testing.T) {
		dir := t.TempDir()
		if err := os.Chmod(dir, 0755); err != nil {
			t.Fatal(err)
		}
		if _, err := Open(context.Background(), dir, "fixture.sqlite"); err == nil {
			t.Fatal("wide directory accepted")
		}
	})
	t.Run("symlink", func(t *testing.T) {
		dir := privateTestDir(t)
		target := filepath.Join(t.TempDir(), "target")
		if err := os.WriteFile(target, []byte("keep"), 0600); err != nil {
			t.Fatal(err)
		}
		if err := os.Symlink(target, filepath.Join(dir, "fixture.sqlite")); err != nil {
			t.Fatal(err)
		}
		if _, err := Open(context.Background(), dir, "fixture.sqlite"); err == nil {
			t.Fatal("symlink accepted")
		}
		data, _ := os.ReadFile(target)
		if string(data) != "keep" {
			t.Fatal("symlink target changed")
		}
	})
	t.Run("hardlink", func(t *testing.T) {
		dir := privateTestDir(t)
		target := filepath.Join(dir, "target")
		if err := os.WriteFile(target, []byte("keep"), 0600); err != nil {
			t.Fatal(err)
		}
		if err := os.Link(target, filepath.Join(dir, "fixture.sqlite")); err != nil {
			t.Fatal(err)
		}
		if _, err := Open(context.Background(), dir, "fixture.sqlite"); err == nil {
			t.Fatal("hardlink accepted")
		}
	})
	t.Run("basename", func(t *testing.T) {
		for _, name := range []string{"../outside.sqlite", "/tmp/outside.sqlite", "..", "fixture.db"} {
			if _, err := Open(context.Background(), t.TempDir(), name); err == nil {
				t.Fatal("unsafe filename accepted")
			}
		}
	})
}
