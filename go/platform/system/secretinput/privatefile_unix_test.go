// INPUT: Actual owner-only files, broad permissions, hardlinks, symlinks and FIFO fixtures
// OUTPUT: Private file input pins read-only bytes and rejects aliases/special files without changes or blocking
// POS: [platform / OS primitive] No existing user credential, global source or file creation is read implicitly.
//go:build darwin || linux

package secretinput

import (
	"golang.org/x/sys/unix"
	"os"
	"path/filepath"
	"testing"
)

func TestPrivateInputFileIsExistingPinnedAndReadOnly(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "input-fixture")
	if err := os.WriteFile(path, []byte("synthetic fixture"), 0600); err != nil {
		t.Fatal(err)
	}
	file, err := OpenPrivateFile(path)
	if err != nil {
		t.Fatal(err)
	}
	data, err := Read(t.Context(), file, 128)
	if err != nil || string(data) != "synthetic fixture" {
		t.Fatal(err)
	}
	if _, err := file.Write([]byte("not permitted")); err == nil {
		t.Fatal("credential file was writable")
	}
	file.Close()
	if _, err := OpenPrivateFile(filepath.Join(dir, "missing")); err == nil {
		t.Fatal("missing input was created")
	}
	if _, err := os.Stat(filepath.Join(dir, "missing")); !os.IsNotExist(err) {
		t.Fatal("missing input changed", err)
	}
	if err := os.Chmod(path, 0644); err != nil {
		t.Fatal(err)
	}
	if _, err := OpenPrivateFile(path); err == nil {
		t.Fatal("wide input accepted")
	}
	info, _ := os.Stat(path)
	if info.Mode().Perm() != 0644 {
		t.Fatal("existing permissions were modified")
	}
	os.Chmod(path, 0600)
	if err := os.Symlink(path, filepath.Join(dir, "symlink")); err != nil {
		t.Fatal(err)
	}
	if _, err := OpenPrivateFile(filepath.Join(dir, "symlink")); err == nil {
		t.Fatal("credential symlink followed")
	}
	if err := os.Link(path, filepath.Join(dir, "hardlink")); err != nil {
		t.Fatal(err)
	}
	if _, err := OpenPrivateFile(path); err == nil {
		t.Fatal("hardlinked credential accepted")
	}
	pipe := filepath.Join(dir, "fifo")
	if err := unix.Mkfifo(pipe, 0600); err != nil {
		t.Fatal(err)
	}
	if file, err := OpenPrivateFile(pipe); err == nil {
		file.Close()
		t.Fatal("special input accepted")
	}
}
