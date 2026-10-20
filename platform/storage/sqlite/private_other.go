// INPUT: os.
// OUTPUT: Explicit unsupported private storage error outside validated Unix hosts.
// POS: [platform / durable storage] Fail-closed platform boundary until equivalent ownership/locking is implemented.
//go:build !windows && !darwin && !linux && !freebsd && !openbsd && !netbsd && !dragonfly

package sqlite

import "os"

func lockFile(*os.File) error   { return ErrUnsupported }
func unlockFile(*os.File) error { return nil }
