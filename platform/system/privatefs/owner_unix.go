// INPUT: Unix file metadata and the actual current user identity.
// OUTPUT: Private path owner and single-link file validation.
// POS: [platform / OS primitive] Shares filesystem ownership checks without depending on any database.
//go:build darwin || linux || freebsd || openbsd || netbsd || dragonfly

package privatefs

import (
	"errors"
	"os"
	"syscall"
)

func checkOwner(info os.FileInfo, regular bool) error {
	stat, ok := info.Sys().(*syscall.Stat_t)
	if !ok || stat.Uid != uint32(os.Getuid()) {
		return errors.New("private filesystem path is not owned by the current user")
	}
	if regular && stat.Nlink != 1 {
		return errors.New("hardlinked private filesystem file rejected")
	}
	return nil
}
