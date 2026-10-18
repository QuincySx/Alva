// INPUT: os, syscall.
// OUTPUT: Native Unix UID/hardlink validation and process lock implementation.
// POS: [platform / durable storage] Platform-specific private database ownership boundary.
//go:build darwin || linux || freebsd || openbsd || netbsd || dragonfly

package sqlite

import (
	"errors"
	"os"
	"syscall"
)

func checkOwner(info os.FileInfo, regular bool) error {
	stat, ok := info.Sys().(*syscall.Stat_t)
	if !ok || stat.Uid != uint32(os.Getuid()) {
		return errors.New("private database path is not owned by the current user")
	}
	if regular && stat.Nlink != 1 {
		return errors.New("hardlinked private database file rejected")
	}
	return nil
}
func lockFile(file *os.File) error {
	if err := syscall.Flock(int(file.Fd()), syscall.LOCK_EX|syscall.LOCK_NB); err != nil {
		if errors.Is(err, syscall.EWOULDBLOCK) || errors.Is(err, syscall.EAGAIN) {
			return ErrLocked
		}
		return err
	}
	return nil
}
func unlockFile(file *os.File) error { return syscall.Flock(int(file.Fd()), syscall.LOCK_UN) }
