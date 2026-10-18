// INPUT: Non-Unix private-directory security boundary
// OUTPUT: Explicit unsupported platform result until native ACL ownership implementation
// POS: [L4 extension] Never mistake Unix mode bits for Windows snapshot privacy
//go:build !unix && !windows

package loader

import (
	"errors"
	"os"
)

func checkSnapshotOwner(string, os.FileInfo) error {
	return errors.New("Plugin private snapshot ACL ownership is not implemented on this platform")
}
