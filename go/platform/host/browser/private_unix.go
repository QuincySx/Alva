// INPUT: errors, os, syscall.
// OUTPUT: private browser temporary-root validation on supported native Unix.
// POS: [platform / native execution host] A browser profile root is same-owner private and never an existing user-browser directory.
//go:build darwin || linux

package browser

import (
	"errors"
	"os"
	"syscall"
)

func checkPrivate(path string) error {
	info, err := os.Lstat(path)
	if err != nil {
		return err
	}
	stat, ok := info.Sys().(*syscall.Stat_t)
	if !ok || !info.IsDir() || info.Mode()&os.ModeSymlink != 0 || info.Mode().Perm()&0077 != 0 || stat.Uid != uint32(os.Getuid()) {
		return errors.New("browser temp root must be owner-only, same-user and not a symlink")
	}
	return nil
}
