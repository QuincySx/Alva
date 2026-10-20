// INPUT: File metadata on platforms without validated ownership primitives.
// OUTPUT: Explicit rejection of unsupported private filesystem authority.
// POS: [platform / OS primitive] Does not assume Unix permissions on unvalidated targets.
//go:build !windows && !darwin && !linux && !freebsd && !openbsd && !netbsd && !dragonfly

package privatefs

import (
	"errors"
	"os"
)

func checkOwner(os.FileInfo, bool) error {
	return errors.New("private filesystem ownership unsupported on this platform")
}
