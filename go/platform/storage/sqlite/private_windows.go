// INPUT: Windows private filesystem creation/ownership checks and LockFileEx native locks
// OUTPUT: EnsurePrivateDir, privateFile and real exclusive database owner lock on Windows
// POS: [platform / durable storage] DACL protection replaces Unix mode bits without changing database or session authority.
//go:build windows

package sqlite

import (
	"errors"
	"os"

	"github.com/QuincySx/Alva/go/platform/system/privatefs"
	"golang.org/x/sys/windows"
)

func EnsurePrivateDir(path string) (string, error) { return privatefs.EnsureDir(path) }
func privateFile(path string) (*os.File, error)    { return privatefs.OpenFile(path) }
func lockFile(file *os.File) error {
	err := windows.LockFileEx(windows.Handle(file.Fd()), windows.LOCKFILE_EXCLUSIVE_LOCK|windows.LOCKFILE_FAIL_IMMEDIATELY, 0, 1, 0, &windows.Overlapped{})
	if errors.Is(err, windows.ERROR_LOCK_VIOLATION) {
		return ErrLocked
	}
	return err
}
func unlockFile(file *os.File) error {
	return windows.UnlockFileEx(windows.Handle(file.Fd()), 0, 1, 0, &windows.Overlapped{})
}
