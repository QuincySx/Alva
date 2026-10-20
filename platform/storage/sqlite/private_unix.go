// INPUT: os, syscall.
// OUTPUT: Native Unix exclusive process lock implementation.
// POS: [platform / durable storage] Platform-specific private database ownership boundary.
//go:build darwin || linux || freebsd || openbsd || netbsd || dragonfly

package sqlite

import (
	"errors"
	"os"
	"syscall"
)

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
