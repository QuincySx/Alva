// INPUT: Explicit temporary Unix paths, owner-only files and linked/wide-permission counterexamples.
// OUTPUT: Native private-directory and file validation without any SQL dependency.
// POS: [platform / OS primitive acceptance] Guards the relocated shared filesystem boundary.
//go:build darwin || linux || freebsd || openbsd || netbsd || dragonfly

package privatefs_test

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/QuincySx/Alva/platform/system/privatefs"
)

func TestPrivatePathsRejectAliasesAndDoNotRepairPermissions(t *testing.T) {
	dir, err := privatefs.EnsureDir(filepath.Join(t.TempDir(), "private"))
	if err != nil {
		t.Fatal(err)
	}
	file, err := privatefs.OpenFile(filepath.Join(dir, "actual"))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := file.WriteString("retained"); err != nil {
		t.Fatal(err)
	}
	if err := file.Close(); err != nil {
		t.Fatal(err)
	}
	for _, kind := range []string{"symlink", "hardlink", "wide"} {
		t.Run(kind, func(t *testing.T) {
			path := filepath.Join(dir, kind)
			var err error
			switch kind {
			case "symlink":
				err = os.Symlink(filepath.Join(dir, "actual"), path)
			case "hardlink":
				err = os.Link(filepath.Join(dir, "actual"), path)
			case "wide":
				err = os.WriteFile(path, []byte("keep"), 0644)
			}
			if err != nil {
				t.Fatal(err)
			}
			before, err := os.Lstat(path)
			if err != nil {
				t.Fatal(err)
			}
			if f, err := privatefs.OpenFile(path); err == nil {
				f.Close()
				t.Fatal("unsafe path opened")
			}
			after, err := os.Lstat(path)
			if err != nil || after.Mode() != before.Mode() || !os.SameFile(before, after) {
				t.Fatal("existing object was repaired or replaced", err)
			}
		})
	}
	alias := filepath.Join(t.TempDir(), "alias")
	if err := os.Symlink(dir, alias); err != nil {
		t.Fatal(err)
	}
	if _, err := privatefs.EnsureDir(alias + string(os.PathSeparator)); err == nil {
		t.Fatal("trailing slash hid a final symlink")
	}
	data, err := os.ReadFile(filepath.Join(dir, "actual"))
	if err != nil || string(data) != "retained" {
		t.Fatal("private bytes changed", err)
	}
}
