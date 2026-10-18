// INPUT: Windows filesystem/security APIs and disposable test directories without user data
// OUTPUT: Atomic private creation, inherited child ACL, unchanged broad ACL and link/path rejection regressions
// POS: [platform / OS primitive] Real Windows security tests never treat cross-compilation or chmod as ACL validation.
//go:build windows

package privatefs

import (
	"os"
	"path/filepath"
	"testing"
	"unsafe"

	"golang.org/x/sys/windows"
)

func TestProtectedCreationAndInheritedFilePrivacy(t *testing.T) {
	dir, err := EnsureDir(filepath.Join(t.TempDir(), "owned", "nested"))
	if err != nil {
		t.Fatal(err)
	}
	if err := CheckDir(dir); err != nil {
		t.Fatal(err)
	}
	file, err := OpenFile(filepath.Join(dir, "actual.sqlite"))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := file.Write([]byte("fixture bytes")); err != nil {
		t.Fatal(err)
	}
	if err := file.Close(); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, "journal"), []byte("inherited private bytes"), 0600); err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"actual.sqlite", "journal"} {
		if err := CheckFile(filepath.Join(dir, name)); err != nil {
			t.Fatalf("%s: %v", name, err)
		}
	}
	read, err := OpenRead(filepath.Join(dir, "actual.sqlite"))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := read.Write([]byte("must not write")); err == nil {
		t.Fatal("private read handle granted write access")
	}
	read.Close()
	if _, err := OpenRead(filepath.Join(dir, "missing")); err == nil {
		t.Fatal("private read created a missing file")
	}
}

func TestExistingBroadDirectoryIsRejectedWithoutChangingDACL(t *testing.T) {
	user, err := windows.GetCurrentProcessToken().GetTokenUser()
	if err != nil {
		t.Fatal(err)
	}
	sd, err := windows.SecurityDescriptorFromString("O:" + user.User.Sid.String() + "D:P(A;OICI;FA;;;" + user.User.Sid.String() + ")(A;OICI;GR;;;WD)")
	if err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(t.TempDir(), "existing-broad")
	p, _ := windows.UTF16PtrFromString(path)
	if err := windows.CreateDirectory(p, &windows.SecurityAttributes{Length: uint32(unsafe.Sizeof(windows.SecurityAttributes{})), SecurityDescriptor: sd}); err != nil {
		t.Fatal(err)
	}
	before, err := windows.GetNamedSecurityInfo(path, windows.SE_FILE_OBJECT, windows.OWNER_SECURITY_INFORMATION|windows.DACL_SECURITY_INFORMATION)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := EnsureDir(path); err == nil {
		t.Fatal("existing broad private root was accepted")
	}
	after, err := windows.GetNamedSecurityInfo(path, windows.SE_FILE_OBJECT, windows.OWNER_SECURITY_INFORMATION|windows.DACL_SECURITY_INFORMATION)
	if err != nil || before.String() != after.String() {
		t.Fatal("existing user ACL was changed", err)
	}
}

func TestHardlinksAndAlternativeNamesAreRejected(t *testing.T) {
	dir, err := EnsureDir(filepath.Join(t.TempDir(), "owned"))
	if err != nil {
		t.Fatal(err)
	}
	file, err := OpenFile(filepath.Join(dir, "original"))
	if err != nil {
		t.Fatal(err)
	}
	file.Close()
	if err := os.Link(filepath.Join(dir, "original"), filepath.Join(dir, "alias")); err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"original", "alias"} {
		if err := CheckFile(filepath.Join(dir, name)); err == nil {
			t.Fatal("hardlinked file accepted")
		}
		if file, err := OpenFile(filepath.Join(dir, name)); err == nil {
			file.Close()
			t.Fatal("hardlinked file opened writable")
		}
	}
	for _, path := range []string{filepath.Join(dir, "file:stream"), filepath.Join(dir, "trailing."), filepath.Join(dir, "NUL.txt"), `\\server\share\private`, `\\?\C:\private`} {
		if _, err := EnsureDir(path); err == nil {
			t.Fatal("alias/device/remote path accepted")
		}
	}
}

func TestReparseAncestorDoesNotCreateDirectoryInTarget(t *testing.T) {
	dir := t.TempDir()
	target := filepath.Join(dir, "target")
	if err := os.Mkdir(target, 0700); err != nil {
		t.Fatal(err)
	}
	alias := filepath.Join(dir, "alias")
	if err := os.Symlink(target, alias); err != nil {
		t.Skipf("this Windows account cannot create symlink fixtures: %v", err)
	}
	if _, err := EnsureDir(filepath.Join(alias, "must-not-create")); err == nil {
		t.Fatal("reparse ancestor accepted")
	}
	if _, err := os.Stat(filepath.Join(target, "must-not-create")); !os.IsNotExist(err) {
		t.Fatal("target was changed before reparse rejection", err)
	}
}
