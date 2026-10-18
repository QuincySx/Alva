// INPUT: Real Windows Root, native no-follow/truncate/append operations and WASI file bridge
// OUTPUT: Native file capability acceptance and device/ADS/traversal/link rejection regressions
// POS: [platform / WASM host] These tests require actual Windows; cross compilation never proves native execution.
//go:build windows

package host

import (
	"io"
	"os"
	"path/filepath"
	"testing"

	expsys "github.com/tetratelabs/wazero/experimental/sys"
)

func TestWindowsWASIRootedReadWriteAndAtomicNoFollow(t *testing.T) {
	directory := t.TempDir()
	root, err := os.OpenRoot(directory)
	if err != nil {
		t.Fatal(err)
	}
	defer root.Close()
	if err := root.Mkdir("nested", 0700); err != nil {
		t.Fatal(err)
	}
	file, err := nativeOpen(root, filepath.Join("nested", "input"), os.O_CREATE|os.O_WRONLY|os.O_EXCL, 0600, true)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := file.WriteString("original"); err != nil {
		t.Fatal(err)
	}
	if err := file.Close(); err != nil {
		t.Fatal(err)
	}
	file, err = nativeOpen(root, filepath.Join("nested", "input"), os.O_WRONLY|os.O_TRUNC|os.O_APPEND, 0600, true)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := file.WriteString("A"); err != nil {
		t.Fatal(err)
	}
	if _, err := file.Seek(0, io.SeekStart); err != nil {
		t.Fatal(err)
	}
	if _, err := file.WriteString("B"); err != nil {
		t.Fatal(err)
	}
	if err := file.Close(); err != nil {
		t.Fatal(err)
	}
	if data, err := root.ReadFile(filepath.Join("nested", "input")); err != nil || string(data) != "AB" {
		t.Fatal("no-follow append/truncate did not bind actual file semantics", string(data), err)
	}
	filesystem := &rootedFS{root: root, budget: &fsBudget{}}
	for _, name := range []string{"../outside", "NUL", "COM1", "input:stream", "C:/outside"} {
		if file, errno := filesystem.OpenFile(name, expsys.O_RDONLY|expsys.O_NOFOLLOW, 0); errno == 0 {
			file.Close()
			t.Fatal("special Windows path accepted", name)
		}
	}
	outside := filepath.Join(t.TempDir(), "outside")
	if err := os.WriteFile(outside, []byte("outside original"), 0600); err != nil {
		t.Fatal(err)
	}
	link := filepath.Join(directory, "link")
	if err := os.Symlink(outside, link); err != nil {
		t.Log("link privilege unavailable; ordinary files/device/ADS checks above ran", err)
		return
	}
	if err := os.Symlink(filepath.Join("nested", "input"), filepath.Join(directory, "in-root-link")); err != nil {
		t.Fatal(err)
	}
	if file, err := nativeOpen(root, "in-root-link", os.O_WRONLY|os.O_TRUNC, 0600, true); err == nil {
		file.Close()
		t.Fatal("no-follow opened a permitted in-root link")
	}
	if data, err := root.ReadFile(filepath.Join("nested", "input")); err != nil || string(data) != "AB" {
		t.Fatal("no-follow rejection truncated an in-root link target", err)
	}
	for _, follow := range []bool{false, true} {
		if file, err := nativeOpen(root, "link", os.O_WRONLY|os.O_TRUNC, 0600, !follow); err == nil {
			file.Close()
			t.Fatal("opened an escaping or no-follow link")
		}
	}
	if data, err := os.ReadFile(outside); err != nil || string(data) != "outside original" {
		t.Fatal("rejected link open truncated its target", err)
	}
}
