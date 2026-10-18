// INPUT: Unix filesystem ownership metadata
// OUTPUT: Snapshot current-user ownership check
// POS: [L4 extension] No world-writable/foreign-owner Plugin staging authority
//go:build unix

package loader

import (
	"errors"
	"os"
	"syscall"
)

func checkSnapshotOwner(_ string, info os.FileInfo) error {
	if info.Mode().Perm()&0077 != 0 {
		return errors.New("Plugin snapshots require an owner-only parent")
	}
	stat, ok := info.Sys().(*syscall.Stat_t)
	if !ok || stat.Uid != uint32(os.Getuid()) {
		return errors.New("Plugin snapshot parent must belong to current user")
	}
	return nil
}
