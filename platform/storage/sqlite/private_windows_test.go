// INPUT: Actual Windows private database, independent lock helper process and disposable fixture roots
// OUTPUT: LockFileEx cross-process exclusion, durable SQLite reopening and DACL inheritance validation
// POS: [platform / durable storage] Native Windows ownership tests complement Unix mode-bit regressions.
//go:build windows

package sqlite

import (
	"context"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"golang.org/x/sys/windows"

	"github.com/QuincySx/Alva/platform/system/privatefs"
)

func TestWindowsLockHelper(t *testing.T) {
	if len(os.Args) < 4 || os.Args[len(os.Args)-3] != "windows-lock-fixture" {
		return
	}
	lock, err := LockDatabase(os.Args[len(os.Args)-2], "fixture.sqlite")
	if os.Args[len(os.Args)-1] == "locked" {
		if !errors.Is(err, ErrLocked) {
			if lock != nil {
				lock.Close()
			}
			t.Fatal("foreign process ownership was not excluded", err)
		}
	} else {
		if err != nil {
			t.Fatal(err)
		}
		if err := lock.Close(); err != nil {
			t.Fatal(err)
		}
	}
}

func TestWindowsDurableDatabaseAndProcessOwner(t *testing.T) {
	dir, err := privatefs.EnsureDir(filepath.Join(t.TempDir(), "actual-private"))
	if err != nil {
		t.Fatal(err)
	}
	lock, err := LockDatabase(dir, "fixture.sqlite")
	if err != nil {
		t.Fatal(err)
	}
	defer lock.Close()
	probe := func(expect string) {
		t.Helper()
		executable, err := os.Executable()
		if err != nil {
			t.Fatal(err)
		}
		ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
		defer cancel()
		command := exec.CommandContext(ctx, executable, "-test.run=^TestWindowsLockHelper$", "--", "windows-lock-fixture", dir, expect)
		root, _ := windows.GetWindowsDirectory()
		command.Env = []string{"SystemRoot=" + root, "GORACE=atexit_sleep_ms=0"}
		if output, err := command.CombinedOutput(); err != nil {
			t.Fatalf("actual process lock: %v / %s", err, output)
		}
	}
	probe("locked")
	db, err := Open(t.Context(), dir, "fixture.sqlite")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := db.ExecContext(t.Context(), "CREATE TABLE facts(value TEXT); INSERT INTO facts VALUES('durable fixture')"); err != nil {
		t.Fatal(err)
	}
	var journal string
	if err := db.QueryRowContext(t.Context(), "PRAGMA journal_mode").Scan(&journal); err != nil || journal != "delete" {
		t.Fatal(journal, err)
	}
	if err := db.Close(); err != nil {
		t.Fatal(err)
	}
	if err := lock.Close(); err != nil {
		t.Fatal(err)
	}
	probe("released")
	db, err = Open(t.Context(), dir, "fixture.sqlite")
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	var value string
	if err := db.QueryRowContext(t.Context(), "SELECT value FROM facts").Scan(&value); err != nil || value != "durable fixture" {
		t.Fatal(value, err)
	}
	for _, name := range []string{"fixture.sqlite", "fixture.sqlite.lock"} {
		if err := privatefs.CheckFile(filepath.Join(dir, name)); err != nil {
			t.Fatal(name, err)
		}
	}
	for _, name := range []string{"../escape.sqlite", "nested/escape.sqlite", "drive:alias.sqlite"} {
		if _, err := Open(t.Context(), dir, name); err == nil || strings.Contains(name, "fixture") {
			t.Fatal("unsafe basename accepted")
		}
	}
}
