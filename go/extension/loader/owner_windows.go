// INPUT: Pinned snapshot parent identity and Windows owner-only protected DACL checks
// OUTPUT: Native Windows Plugin snapshot parent authorization without Unix mode assumptions
// POS: [L4 extension] Private staging remains distinct from process approval and product session authorization.
//go:build windows

package loader

import (
	"errors"
	"os"

	"github.com/QuincySx/Alva/go/platform/system/privatefs"
)

func checkSnapshotOwner(path string, info os.FileInfo) error {
	if err := privatefs.CheckDir(path); err != nil {
		return err
	}
	current, err := os.Lstat(path)
	if err != nil || !os.SameFile(info, current) {
		return errors.New("private Plugin parent changed after pinning")
	}
	return nil
}
